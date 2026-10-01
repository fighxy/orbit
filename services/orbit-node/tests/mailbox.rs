//! End-to-end tests: a real node and real clients over QUIC on localhost.

use ed25519_dalek::SigningKey;
use orbit_node::{Config, MailboxConfig, Node};
use orbit_protocol::mailbox::{
    DepositToken, ErrorCode, MAX_ENVELOPE_BYTES, MailboxId, Request, Response, SignatureBytes,
};
use orbit_transport::{MailboxClient, client_endpoint};

const CODE: &str = "orbit-test-registration";

fn config(dir: &std::path::Path, mailbox: MailboxConfig) -> Config {
    Config {
        data_dir: dir.to_path_buf(),
        listen: vec!["127.0.0.1:0".parse().unwrap()],
        public_addrs: vec![],
        mailbox,
    }
}

fn limits() -> MailboxConfig {
    MailboxConfig {
        registration_code: Some(CODE.into()),
        ..MailboxConfig::default()
    }
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn trace() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
}

async fn connect(node: &Node) -> MailboxClient {
    trace();
    let endpoint = client_endpoint().await.unwrap();
    MailboxClient::connect(&endpoint, node.address()).await.unwrap()
}

fn remote_code<T: std::fmt::Debug>(result: orbit_transport::Result<T>) -> ErrorCode {
    result.unwrap_err().remote_code().expect("node error")
}

#[tokio::test(flavor = "multi_thread")]
async fn offline_recipient_receives_deposits_after_reconnecting() {
    let dir = tempfile::tempdir().unwrap();
    let node = Node::start(config(dir.path(), limits())).await.unwrap();
    let recipient = key(1);
    let mailbox = MailboxId(recipient.verifying_key().to_bytes());
    let token = DepositToken::generate().unwrap();

    {
        let mut owner = connect(&node).await;
        assert!(owner.authenticate(&recipient, Some(CODE)).await.unwrap());
        owner.add_deposit_token(&token).await.unwrap();
        owner.close();
    }

    // The recipient is offline; a sender deposits, including a retry.
    let sender = connect(&node).await;
    let first = sender.deposit(&mailbox, &token, b"envelope-1").await.unwrap();
    assert!(!first.duplicate);
    let retry = sender.deposit(&mailbox, &token, b"envelope-1").await.unwrap();
    assert!(retry.duplicate);
    assert_eq!(retry.id, first.id);
    sender.deposit(&mailbox, &token, b"envelope-2").await.unwrap();

    let mut owner = connect(&node).await;
    assert!(!owner.authenticate(&recipient, None).await.unwrap());
    let (items, more) = owner.fetch(0, 10).await.unwrap();
    assert!(!more);
    let envelopes: Vec<_> = items.iter().map(|i| i.envelope.as_slice()).collect();
    assert_eq!(envelopes, vec![b"envelope-1".as_slice(), b"envelope-2".as_slice()]);
    assert!(items[0].seq < items[1].seq);

    let ids: Vec<_> = items.iter().map(|i| i.id).collect();
    assert_eq!(owner.ack(&ids).await.unwrap(), 2);
    assert!(owner.fetch(0, 10).await.unwrap().0.is_empty());
    assert_eq!(owner.status().await.unwrap().items, 0);
    node.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn new_mailboxes_require_the_registration_code() {
    let dir = tempfile::tempdir().unwrap();
    let node = Node::start(config(dir.path(), limits())).await.unwrap();

    let mut client = connect(&node).await;
    assert_eq!(
        remote_code(client.authenticate(&key(2), None).await),
        ErrorCode::Forbidden
    );
    let mut client = connect(&node).await;
    assert_eq!(
        remote_code(client.authenticate(&key(2), Some("wrong-code-but-long")).await),
        ErrorCode::Forbidden
    );
    let mut client = connect(&node).await;
    assert!(client.authenticate(&key(2), Some(CODE)).await.unwrap());
    node.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unauthorized_requests_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let node = Node::start(config(dir.path(), limits())).await.unwrap();
    let owner_key = key(3);
    let mailbox = MailboxId(owner_key.verifying_key().to_bytes());
    let token = DepositToken::generate().unwrap();

    let stranger = connect(&node).await;
    assert_eq!(remote_code(stranger.fetch(0, 10).await), ErrorCode::Unauthenticated);
    assert_eq!(remote_code(stranger.status().await), ErrorCode::Unauthenticated);
    // Unknown mailboxes and unknown tokens are indistinguishable.
    assert_eq!(
        remote_code(stranger.deposit(&mailbox, &token, b"x").await),
        ErrorCode::Forbidden
    );

    let mut owner = connect(&node).await;
    owner.authenticate(&owner_key, Some(CODE)).await.unwrap();
    owner.add_deposit_token(&token).await.unwrap();
    let wrong = DepositToken::generate().unwrap();
    assert_eq!(
        remote_code(stranger.deposit(&mailbox, &wrong, b"x").await),
        ErrorCode::Forbidden
    );
    stranger.deposit(&mailbox, &token, b"x").await.unwrap();

    owner.remove_deposit_token(&token).await.unwrap();
    assert_eq!(
        remote_code(stranger.deposit(&mailbox, &token, b"y").await),
        ErrorCode::Forbidden
    );
    let too_big = vec![0u8; MAX_ENVELOPE_BYTES + 1];
    owner.add_deposit_token(&token).await.unwrap();
    assert_eq!(
        remote_code(stranger.deposit(&mailbox, &token, &too_big).await),
        ErrorCode::TooLarge
    );
    node.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn challenges_are_single_use_and_bound_to_the_connection() {
    let dir = tempfile::tempdir().unwrap();
    let node = Node::start(config(dir.path(), limits())).await.unwrap();
    let owner_key = key(4);
    let mailbox = MailboxId(owner_key.verifying_key().to_bytes());
    let client = connect(&node).await;

    // Authenticating without a challenge fails.
    let bogus = Request::Authenticate {
        mailbox,
        signature: SignatureBytes(vec![0; 64]),
        registration_code: Some(CODE.into()),
    };
    assert!(matches!(
        client.request(&bogus).await.unwrap(),
        Response::Error {
            code: ErrorCode::BadRequest,
            ..
        }
    ));

    // A signature made for another client's connection does not verify.
    let Response::Challenge { nonce } = client.request(&Request::Challenge).await.unwrap() else {
        panic!("expected a challenge");
    };
    let node_id = node.address().endpoint_id;
    let foreign = orbit_protocol::mailbox::sign_auth(&owner_key, &node_id, &[9u8; 32], &nonce);
    let request = Request::Authenticate {
        mailbox,
        signature: foreign,
        registration_code: Some(CODE.into()),
    };
    assert!(matches!(
        client.request(&request).await.unwrap(),
        Response::Error {
            code: ErrorCode::Forbidden,
            ..
        }
    ));
    // The nonce was consumed by the failed attempt.
    assert!(matches!(
        client.request(&request).await.unwrap(),
        Response::Error {
            code: ErrorCode::BadRequest,
            ..
        }
    ));
    node.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn items_and_node_identity_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let owner_key = key(5);
    let mailbox = MailboxId(owner_key.verifying_key().to_bytes());
    let token = DepositToken::generate().unwrap();

    let node = Node::start(config(dir.path(), limits())).await.unwrap();
    let node_id = node.address().endpoint_id;
    let mut owner = connect(&node).await;
    owner.authenticate(&owner_key, Some(CODE)).await.unwrap();
    owner.add_deposit_token(&token).await.unwrap();
    connect(&node).await.deposit(&mailbox, &token, b"kept").await.unwrap();
    node.shutdown().await;

    let node = Node::start(config(dir.path(), limits())).await.unwrap();
    assert_eq!(node.address().endpoint_id, node_id);
    let mut owner = connect(&node).await;
    owner.authenticate(&owner_key, None).await.unwrap();
    let (items, _) = owner.fetch(0, 10).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].envelope, b"kept");
    node.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn request_rate_is_limited_per_connection() {
    let dir = tempfile::tempdir().unwrap();
    let node = Node::start(config(
        dir.path(),
        MailboxConfig {
            max_requests_per_minute: 3,
            ..limits()
        },
    ))
    .await
    .unwrap();
    let client = connect(&node).await;
    for _ in 0..3 {
        assert!(matches!(
            client.request(&Request::Challenge).await.unwrap(),
            Response::Challenge { .. }
        ));
    }
    assert!(matches!(
        client.request(&Request::Challenge).await.unwrap(),
        Response::Error {
            code: ErrorCode::RateLimited,
            ..
        }
    ));
    node.shutdown().await;
}

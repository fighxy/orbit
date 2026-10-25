//! The real application engine on two devices, through a real QUIC mailbox.
use orbit_core::domain::{ConversationId, MessageState};
use orbit_core::engine::{Command, CommandResult, Engine, EngineConfig, Event};
use orbit_core::identity::IdentitySecret;
use orbit_node::{Config, MailboxConfig, Node};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CODE: &str = "vertical-chat-registration";

async fn call(engine: &Arc<Engine>, command: Command) -> CommandResult {
    let id = engine.submit(command).unwrap();
    let engine = engine.clone();
    tokio::task::spawn_blocking(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            assert!(Instant::now() < deadline, "command {id} timed out");
            for event in engine.wait_events(Duration::from_secs(1)).unwrap() {
                match event.event {
                    Event::CommandSucceeded { request_id, result } if request_id == id => return result,
                    Event::CommandFailed { request_id, error } if request_id == id => {
                        panic!("command failed: {error:?}")
                    }
                    _ => {}
                }
            }
        }
    })
    .await
    .unwrap()
}

async fn await_ready(engine: &Arc<Engine>) -> ConversationId {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let CommandResult::Snapshot { conversations, .. } = call(engine, Command::GetSnapshot).await
            && let Some(conversation) = conversations
                .iter()
                .find(|c| c.contact.as_ref().is_some_and(|c| c.ready))
        {
            return conversation.id;
        }
        assert!(Instant::now() < deadline, "contact exchange did not complete");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn await_message(engine: &Arc<Engine>, conversation: ConversationId, text: &str, state: MessageState) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let CommandResult::Messages { page } = call(
            engine,
            Command::ListMessages {
                conversation_id: conversation,
                before_seq: None,
                limit: 50,
            },
        )
        .await
        {
            let matching: Vec<_> = page
                .messages
                .iter()
                .filter(|m| m.body == (orbit_core::domain::MessageBody::Text { text: text.into() }))
                .collect();
            assert!(matching.len() <= 1, "duplicate application message");
            if matching.first().is_some_and(|m| m.state == state) {
                return;
            }
        }
        assert!(Instant::now() < deadline, "message did not reach {state:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn open(dir: &std::path::Path, secret: &IdentitySecret) -> Arc<Engine> {
    Arc::new(
        Engine::open(
            EngineConfig {
                data_dir: dir.to_path_buf(),
            },
            secret,
        )
        .unwrap(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_accounts_register_exchange_contacts_send_and_receive_after_restart() {
    let node_dir = tempfile::tempdir().unwrap();
    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let node = Node::start(Config {
        data_dir: node_dir.path().to_path_buf(),
        listen: vec!["127.0.0.1:0".parse().unwrap()],
        public_addrs: vec![],
        mailbox: MailboxConfig {
            registration_code: Some(CODE.into()),
            ..Default::default()
        },
    })
    .await
    .unwrap();
    let alice_secret = IdentitySecret::generate().unwrap();
    let bob_secret = IdentitySecret::generate().unwrap();
    let alice = open(alice_dir.path(), &alice_secret);
    let bob = open(bob_dir.path(), &bob_secret);
    for (engine, name) in [(&alice, "Алиса"), (&bob, "Борис")] {
        call(
            engine,
            Command::UpdateProfile {
                display_name: name.into(),
                about: "".into(),
            },
        )
        .await;
        call(
            engine,
            Command::RegisterNode {
                node: node.address().to_string(),
                registration_code: Some(CODE.into()),
            },
        )
        .await;
    }
    let text = match call(&alice, Command::CreateInvite).await {
        CommandResult::InviteCreated { text } => text,
        _ => panic!(),
    };
    let preview = call(&bob, Command::InspectInvite { text: text.clone() }).await;
    assert!(matches!(preview, CommandResult::InviteInspected { preview } if preview.display_name == "Алиса"));
    let bob_conversation = match call(&bob, Command::AcceptInvite { text }).await {
        CommandResult::ContactAdded { contact } => contact.conversation_id,
        _ => panic!(),
    };
    // A send accepted before handshake confirmation must stay queued, not be lost.
    call(
        &bob,
        Command::SendText {
            conversation_id: bob_conversation,
            text: "первое сообщение".into(),
        },
    )
    .await;
    let alice_conversation = await_ready(&alice).await;
    assert_eq!(await_ready(&bob).await, bob_conversation);
    assert_eq!(alice_conversation, bob_conversation);
    await_message(&alice, alice_conversation, "первое сообщение", MessageState::Received).await;
    await_message(&bob, bob_conversation, "первое сообщение", MessageState::Delivered).await;

    bob.close();
    drop(bob);
    call(
        &alice,
        Command::SendText {
            conversation_id: alice_conversation,
            text: "сообщение офлайн".into(),
        },
    )
    .await;
    await_message(&alice, alice_conversation, "сообщение офлайн", MessageState::Mailbox).await;
    let bob = open(bob_dir.path(), &bob_secret);
    await_ready(&bob).await;
    await_message(&bob, bob_conversation, "сообщение офлайн", MessageState::Received).await;
    await_message(&alice, alice_conversation, "сообщение офлайн", MessageState::Delivered).await;
    call(
        &bob,
        Command::SendText {
            conversation_id: bob_conversation,
            text: "ответ после перезапуска".into(),
        },
    )
    .await;
    await_message(
        &alice,
        alice_conversation,
        "ответ после перезапуска",
        MessageState::Received,
    )
    .await;
    let before_close = Instant::now();
    alice.close();
    bob.close();
    assert!(
        before_close.elapsed() < Duration::from_secs(5),
        "close waited for a long poll"
    );
    node.shutdown().await;
}

//! Client side of Orbit's network protocols over Iroh (QUIC + TLS 1.3).
//!
//! The QUIC handshake authenticates the node by the endpoint ID in its
//! [`NodeAddress`]; the mailbox owner then authenticates with a signature over
//! a node challenge (see `orbit_protocol::mailbox`).

use ed25519_dalek::SigningKey;
use iroh::endpoint::{Connection, presets};
use iroh::{Endpoint, EndpointAddr, PublicKey, RelayMode};
use orbit_protocol::mailbox::{
    ALPN, DepositToken, ErrorCode, Item, ItemId, MAX_RESPONSE_BYTES, MailboxId, MailboxStatus, Request, Response,
    sign_auth,
};
use orbit_protocol::{NodeAddress, decode, encode};

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("cannot create the network endpoint: {0}")]
    Endpoint(String),
    #[error("cannot connect to the node: {0}")]
    Connect(String),
    #[error("connection failed: {0}")]
    Connection(String),
    #[error("protocol violation: {0}")]
    Protocol(&'static str),
    #[error("node refused the request ({code:?}): {message}")]
    Remote { code: ErrorCode, message: String },
}

impl TransportError {
    /// Error code returned by the node, if the node answered.
    pub fn remote_code(&self) -> Option<ErrorCode> {
        match self {
            TransportError::Remote { code, .. } => Some(*code),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, TransportError>;

/// Endpoint for talking to Orbit nodes: no third-party relays or address
/// lookup, a fresh transport key per process.
pub async fn client_endpoint() -> Result<Endpoint> {
    Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .map_err(|e| TransportError::Endpoint(e.to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepositReceipt {
    pub id: ItemId,
    pub duplicate: bool,
    pub expires_at_ms: i64,
}

/// One connection to a node's mailbox service.
#[derive(Debug)]
pub struct MailboxClient {
    // Keeps the endpoint alive: dropping its last handle aborts connections.
    endpoint: Endpoint,
    connection: Connection,
    node_id: [u8; 32],
    client_id: [u8; 32],
    mailbox: Option<MailboxId>,
}

impl MailboxClient {
    pub async fn connect(endpoint: &Endpoint, node: &NodeAddress) -> Result<Self> {
        let id = PublicKey::from_bytes(&node.endpoint_id).map_err(|_| TransportError::Connect("bad node id".into()))?;
        let addr = node
            .addrs
            .iter()
            .fold(EndpointAddr::new(id), |addr, socket| addr.with_ip_addr(*socket));
        let connection = endpoint
            .connect(addr, ALPN)
            .await
            .map_err(|e| TransportError::Connect(e.to_string()))?;
        Ok(Self {
            endpoint: endpoint.clone(),
            connection,
            node_id: node.endpoint_id,
            client_id: *endpoint.id().as_bytes(),
            mailbox: None,
        })
    }

    /// Mailbox this connection is authenticated for.
    pub fn mailbox(&self) -> Option<MailboxId> {
        self.mailbox
    }

    /// Proves ownership of the mailbox of `key`. Returns true when the node
    /// created the mailbox now.
    pub async fn authenticate(&mut self, key: &SigningKey, registration_code: Option<&str>) -> Result<bool> {
        let nonce = match self.call(&Request::Challenge).await? {
            Response::Challenge { nonce } => nonce,
            _ => return Err(TransportError::Protocol("expected a challenge")),
        };
        let mailbox = MailboxId(key.verifying_key().to_bytes());
        let request = Request::Authenticate {
            mailbox,
            signature: sign_auth(key, &self.node_id, &self.client_id, &nonce),
            registration_code: registration_code.map(str::to_owned),
        };
        match self.call(&request).await? {
            Response::Authenticated { created } => {
                self.mailbox = Some(mailbox);
                Ok(created)
            }
            _ => Err(TransportError::Protocol("expected authentication result")),
        }
    }

    pub async fn add_deposit_token(&self, token: &DepositToken) -> Result<()> {
        self.expect_ok(&Request::AddDepositToken {
            token_hash: token.hash(),
        })
        .await
    }

    pub async fn remove_deposit_token(&self, token: &DepositToken) -> Result<()> {
        self.expect_ok(&Request::RemoveDepositToken {
            token_hash: token.hash(),
        })
        .await
    }

    /// Stores an envelope in someone's mailbox. Retrying the same bytes is safe.
    pub async fn deposit(&self, mailbox: &MailboxId, token: &DepositToken, envelope: &[u8]) -> Result<DepositReceipt> {
        let request = Request::Deposit {
            mailbox: *mailbox,
            token: token.clone(),
            envelope: envelope.to_vec(),
        };
        match self.call(&request).await? {
            Response::Deposited {
                id,
                duplicate,
                expires_at_ms,
            } => Ok(DepositReceipt {
                id,
                duplicate,
                expires_at_ms,
            }),
            _ => Err(TransportError::Protocol("expected a deposit receipt")),
        }
    }

    /// Items after `after_seq`, oldest first, and whether more remain.
    pub async fn fetch(&self, after_seq: u64, limit: u32) -> Result<(Vec<Item>, bool)> {
        match self.call(&Request::Fetch { after_seq, limit }).await? {
            Response::Items { items, more } => Ok((items, more)),
            _ => Err(TransportError::Protocol("expected items")),
        }
    }

    /// Deletes items the client has stored durably.
    pub async fn ack(&self, ids: &[ItemId]) -> Result<u32> {
        match self.call(&Request::Ack { ids: ids.to_vec() }).await? {
            Response::Acked { removed } => Ok(removed),
            _ => Err(TransportError::Protocol("expected an acknowledgement")),
        }
    }

    /// Long poll: true as soon as the mailbox holds an item after
    /// `after_seq`, false after `timeout_ms`. Other requests on this client
    /// may run concurrently.
    pub async fn wait(&self, after_seq: u64, timeout_ms: u32) -> Result<bool> {
        match self.call(&Request::Wait { after_seq, timeout_ms }).await? {
            Response::Waited { ready } => Ok(ready),
            _ => Err(TransportError::Protocol("expected a wait result")),
        }
    }

    pub async fn status(&self) -> Result<MailboxStatus> {
        match self.call(&Request::Status).await? {
            Response::Status(status) => Ok(status),
            _ => Err(TransportError::Protocol("expected status")),
        }
    }

    /// Closes the connection gracefully.
    pub fn close(&self) {
        self.connection.close(0u32.into(), b"bye");
    }

    /// The endpoint this client uses; close it when the process stops using
    /// the network so peers see a clean shutdown.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    async fn expect_ok(&self, request: &Request) -> Result<()> {
        match self.call(request).await? {
            Response::Ok => Ok(()),
            _ => Err(TransportError::Protocol("expected ok")),
        }
    }

    /// Sends one raw request and returns the raw response (node errors are
    /// returned as responses, not as [`TransportError::Remote`]). For tools
    /// and protocol tests.
    pub async fn request(&self, request: &Request) -> Result<Response> {
        match self.call(request).await {
            Err(TransportError::Remote { code, message }) => Ok(Response::Error { code, message }),
            other => other,
        }
    }

    /// One request per bidirectional stream.
    async fn call(&self, request: &Request) -> Result<Response> {
        let bytes = encode(request).map_err(|_| TransportError::Protocol("request encoding failed"))?;
        let (mut send, mut recv) = self
            .connection
            .open_bi()
            .await
            .map_err(|e| TransportError::Connection(e.to_string()))?;
        send.write_all(&bytes)
            .await
            .map_err(|e| TransportError::Connection(e.to_string()))?;
        send.finish().map_err(|e| TransportError::Connection(e.to_string()))?;
        let reply = recv
            .read_to_end(MAX_RESPONSE_BYTES)
            .await
            .map_err(|e| TransportError::Connection(e.to_string()))?;
        match decode::<Response>(&reply).map_err(|_| TransportError::Protocol("malformed response"))? {
            Response::Error { code, message } => Err(TransportError::Remote { code, message }),
            response => Ok(response),
        }
    }
}

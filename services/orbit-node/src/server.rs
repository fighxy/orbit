//! QUIC server for the mailbox protocol.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iroh::endpoint::presets;
use iroh::{Endpoint, RelayMode, SecretKey};
use orbit_protocol::mailbox::{
    ALPN, ErrorCode, MAX_ACK_ITEMS, MAX_ENVELOPE_BYTES, MAX_FETCH_ITEMS, MAX_REQUEST_BYTES, MAX_WAIT_MS, MailboxId,
    Request, Response, verify_auth,
};
use orbit_protocol::{NodeAddress, decode, encode};
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::config::{Config, MailboxConfig};
use crate::store::{Store, StoreError};

#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("i/o failure: {0}")]
    Io(#[from] std::io::Error),
    #[error("node key file is invalid: {0}")]
    Key(&'static str),
    #[error("storage failure: {0}")]
    Store(#[from] StoreError),
    #[error("network setup failed: {0}")]
    Network(String),
}

/// A running node. Dropping it without [`Node::shutdown`] leaves tasks running.
#[derive(Debug)]
pub struct Node {
    endpoint: Endpoint,
    address: NodeAddress,
    tasks: Vec<JoinHandle<()>>,
}

struct Context {
    node_id: [u8; 32],
    config: MailboxConfig,
    registration_code_hash: Option<[u8; 32]>,
    store: Arc<Mutex<Store>>,
    waiters: Waiters,
}

/// Wakes long polls of a mailbox when something is deposited into it.
#[derive(Default)]
struct Waiters(Mutex<HashMap<MailboxId, Weak<Notify>>>);

impl Waiters {
    fn subscribe(&self, mailbox: &MailboxId) -> Arc<Notify> {
        let mut map = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(notify) = map.get(mailbox).and_then(Weak::upgrade) {
            return notify;
        }
        // Entries of finished waits are dropped lazily, on insertion.
        map.retain(|_, notify| notify.strong_count() > 0);
        let notify = Arc::new(Notify::new());
        map.insert(*mailbox, Arc::downgrade(&notify));
        notify
    }

    fn wake(&self, mailbox: &MailboxId) {
        let map = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(notify) = map.get(mailbox).and_then(Weak::upgrade) {
            notify.notify_waiters();
        }
    }
}

impl Node {
    pub async fn start(config: Config) -> Result<Self, NodeError> {
        create_private_dir(&config.data_dir)?;
        let secret = load_or_create_key(&config.data_dir.join("node.key"))?;
        let store = Store::open(&config.data_dir.join("mailbox.sqlite"), config.mailbox.clone())?;

        let mut builder = Endpoint::builder(presets::Minimal)
            .secret_key(secret)
            .alpns(vec![ALPN.to_vec()])
            // Clients dial the node directly; relaying for peer-to-peer
            // sessions is a separate, later service.
            .relay_mode(RelayMode::Disabled)
            .clear_ip_transports();
        for addr in &config.listen {
            builder = builder
                .bind_addr(*addr)
                .map_err(|e| NodeError::Network(format!("{addr}: {e}")))?;
        }
        let endpoint = builder.bind().await.map_err(|e| NodeError::Network(e.to_string()))?;

        let node_id = *endpoint.id().as_bytes();
        let bound = endpoint.bound_sockets();
        let addrs = if config.public_addrs.is_empty() {
            if bound.iter().any(|a| a.ip().is_unspecified()) {
                warn!("public_addrs is not set; the advertised address uses the wildcard listen address");
            }
            bound
        } else {
            config.public_addrs.clone()
        };
        let address = NodeAddress {
            endpoint_id: node_id,
            addrs,
        };

        let context = Arc::new(Context {
            node_id,
            registration_code_hash: config.mailbox.registration_code.as_deref().map(code_hash),
            config: config.mailbox.clone(),
            store: Arc::new(Mutex::new(store)),
            waiters: Waiters::default(),
        });

        let accept = tokio::spawn(accept_loop(endpoint.clone(), context.clone()));
        let sweeper = tokio::spawn(sweep_loop(context));
        info!(%address, "orbit-node is listening");
        Ok(Self {
            endpoint,
            address,
            tasks: vec![accept, sweeper],
        })
    }

    pub fn address(&self) -> &NodeAddress {
        &self.address
    }

    /// Addresses the sockets are actually bound to (useful with port 0).
    pub fn bound_addrs(&self) -> Vec<SocketAddr> {
        self.endpoint.bound_sockets()
    }

    /// Stops accepting connections and closes the endpoint. Peers that went
    /// away without closing are not waited for longer than a few seconds.
    pub async fn shutdown(self) {
        for task in &self.tasks {
            task.abort();
        }
        if tokio::time::timeout(Duration::from_secs(3), self.endpoint.close())
            .await
            .is_err()
        {
            warn!("endpoint did not close in time");
        }
    }
}

async fn accept_loop(endpoint: Endpoint, context: Arc<Context>) {
    let slots = Arc::new(Semaphore::new(context.config.max_connections));
    while let Some(incoming) = endpoint.accept().await {
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            warn!("connection limit reached; refusing a connection");
            incoming.refuse();
            continue;
        };
        let context = context.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let connection = match incoming.await {
                Ok(connection) => connection,
                Err(error) => {
                    debug!(%error, "handshake failed");
                    return;
                }
            };
            serve_connection(connection, context).await;
        });
    }
}

struct ConnectionState {
    client_id: [u8; 32],
    nonce: Option<[u8; 32]>,
    mailbox: Option<MailboxId>,
    window_start: Instant,
    window_requests: u32,
    waiting: Arc<AtomicBool>,
}

async fn serve_connection(connection: iroh::endpoint::Connection, context: Arc<Context>) {
    let mut state = ConnectionState {
        client_id: *connection.remote_id().as_bytes(),
        nonce: None,
        mailbox: None,
        window_start: Instant::now(),
        window_requests: 0,
        waiting: Arc::new(AtomicBool::new(false)),
    };
    // Streams are served one at a time, so requests on a connection apply in
    // order. The only exception is `Wait`, which changes nothing.
    while let Ok((mut send, mut recv)) = connection.accept_bi().await {
        let response = match recv.read_to_end(MAX_REQUEST_BYTES).await {
            Ok(bytes) => match decode::<Request>(&bytes) {
                Ok(Request::Wait { after_seq, timeout_ms }) => {
                    match start_wait(&context, &mut state, after_seq, timeout_ms) {
                        Ok(wait) => {
                            let connection = connection.clone();
                            tokio::spawn(async move {
                                tokio::select! {
                                    response = wait => { let _ = reply(&mut send, &response).await; }
                                    _ = connection.closed() => {}
                                }
                            });
                            continue;
                        }
                        Err(response) => response,
                    }
                }
                Ok(request) => handle(&context, &mut state, request).await,
                Err(_) => error(ErrorCode::BadRequest, "malformed request"),
            },
            Err(iroh::endpoint::ReadToEndError::TooLong) => error(ErrorCode::TooLarge, "request too large"),
            Err(_) => break,
        };
        if !reply(&mut send, &response).await {
            break;
        }
    }
}

async fn reply(send: &mut iroh::endpoint::SendStream, response: &Response) -> bool {
    let Ok(bytes) = encode(response) else { return false };
    send.write_all(&bytes).await.is_ok() && send.finish().is_ok()
}

/// Validates a long poll and returns the future that answers it.
fn start_wait(
    context: &Arc<Context>,
    state: &mut ConnectionState,
    after_seq: u64,
    timeout_ms: u32,
) -> Result<impl Future<Output = Response> + Send + 'static, Response> {
    if !allow(state, context.config.max_requests_per_minute) {
        return Err(error(ErrorCode::RateLimited, "too many requests; slow down"));
    }
    let Some(mailbox) = state.mailbox else {
        return Err(unauthenticated());
    };
    if timeout_ms == 0 || timeout_ms > MAX_WAIT_MS {
        return Err(error(ErrorCode::BadRequest, "timeout out of range"));
    }
    if state.waiting.swap(true, Ordering::AcqRel) {
        return Err(error(ErrorCode::BadRequest, "another wait is in progress"));
    }
    let guard = WaitGuard(state.waiting.clone());
    let context = context.clone();
    Ok(async move {
        let _guard = guard;
        wait_for_items(
            &context,
            mailbox,
            after_seq,
            Duration::from_millis(u64::from(timeout_ms)),
        )
        .await
    })
}

struct WaitGuard(Arc<AtomicBool>);

impl Drop for WaitGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

async fn wait_for_items(context: &Arc<Context>, mailbox: MailboxId, after_seq: u64, timeout: Duration) -> Response {
    let notify = context.waiters.subscribe(&mailbox);
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        // Registered before the check so a deposit in between is not missed.
        let notified = notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        match blocking(context, move |s| s.fetch(&mailbox, after_seq, 1, now_ms())).await {
            Ok((items, _)) if !items.is_empty() => return Response::Waited { ready: true },
            Ok(_) => {}
            Err(response) => return response,
        }
        if tokio::time::timeout_at(deadline, notified).await.is_err() {
            return Response::Waited { ready: false };
        }
    }
}

async fn handle(context: &Arc<Context>, state: &mut ConnectionState, request: Request) -> Response {
    if !allow(state, context.config.max_requests_per_minute) {
        return error(ErrorCode::RateLimited, "too many requests; slow down");
    }
    match request {
        Request::Challenge => {
            let mut nonce = [0u8; 32];
            if getrandom::fill(&mut nonce).is_err() {
                return error(ErrorCode::Internal, "random number generator failed");
            }
            state.nonce = Some(nonce);
            Response::Challenge { nonce }
        }
        Request::Authenticate {
            mailbox,
            signature,
            registration_code,
        } => {
            // A nonce authenticates at most once.
            let Some(nonce) = state.nonce.take() else {
                return error(ErrorCode::BadRequest, "request a challenge first");
            };
            if !verify_auth(&context.node_id, &state.client_id, &mailbox, &nonce, &signature) {
                return error(ErrorCode::Forbidden, "invalid signature");
            }
            let exists = match blocking(context, move |s| s.mailbox_exists(&mailbox)).await {
                Ok(exists) => exists,
                Err(response) => return response,
            };
            if !exists && let Some(expected) = context.registration_code_hash {
                // Comparing hashes keeps the comparison independent of the code's bytes.
                let given = registration_code.as_deref().map(code_hash);
                if given != Some(expected) {
                    return error(ErrorCode::Forbidden, "a valid registration code is required");
                }
            }
            match blocking(context, move |s| s.ensure_mailbox(&mailbox, now_ms())).await {
                Ok(created) => {
                    state.mailbox = Some(mailbox);
                    if created {
                        info!(mailbox = ?mailbox, "mailbox created");
                    }
                    Response::Authenticated { created }
                }
                Err(response) => response,
            }
        }
        Request::AddDepositToken { token_hash } => {
            let Some(mailbox) = state.mailbox else {
                return unauthenticated();
            };
            ok(blocking(context, move |s| s.add_token(&mailbox, &token_hash, now_ms())).await)
        }
        Request::RemoveDepositToken { token_hash } => {
            let Some(mailbox) = state.mailbox else {
                return unauthenticated();
            };
            ok(blocking(context, move |s| s.remove_token(&mailbox, &token_hash)).await)
        }
        Request::Deposit {
            mailbox,
            token,
            envelope,
        } => {
            if envelope.is_empty() {
                return error(ErrorCode::BadRequest, "envelope is empty");
            }
            if envelope.len() > MAX_ENVELOPE_BYTES {
                return error(ErrorCode::TooLarge, "envelope too large");
            }
            let token_hash = token.hash();
            // Unknown mailboxes and unknown tokens look the same to senders.
            match blocking(context, move |s| s.token_valid(&mailbox, &token_hash)).await {
                Ok(true) => {}
                Ok(false) => return error(ErrorCode::Forbidden, "deposit is not allowed"),
                Err(response) => return response,
            }
            match blocking(context, move |s| s.deposit(&mailbox, &envelope, now_ms())).await {
                Ok((id, deposited)) => {
                    if !deposited.duplicate {
                        context.waiters.wake(&mailbox);
                    }
                    Response::Deposited {
                        id,
                        duplicate: deposited.duplicate,
                        expires_at_ms: deposited.expires_at_ms,
                    }
                }
                Err(response) => response,
            }
        }
        Request::Fetch { after_seq, limit } => {
            let Some(mailbox) = state.mailbox else {
                return unauthenticated();
            };
            if limit == 0 || limit > MAX_FETCH_ITEMS {
                return error(ErrorCode::BadRequest, "limit out of range");
            }
            match blocking(context, move |s| s.fetch(&mailbox, after_seq, limit, now_ms())).await {
                Ok((items, more)) => Response::Items { items, more },
                Err(response) => response,
            }
        }
        Request::Ack { ids } => {
            let Some(mailbox) = state.mailbox else {
                return unauthenticated();
            };
            if ids.len() > MAX_ACK_ITEMS {
                return error(ErrorCode::BadRequest, "too many ids");
            }
            match blocking(context, move |s| s.ack(&mailbox, &ids)).await {
                Ok(removed) => Response::Acked { removed },
                Err(response) => response,
            }
        }
        Request::Status => {
            let Some(mailbox) = state.mailbox else {
                return unauthenticated();
            };
            match blocking(context, move |s| s.status(&mailbox, now_ms())).await {
                Ok(status) => Response::Status(status),
                Err(response) => response,
            }
        }
        // Served by `start_wait`; reaching this arm is a server bug.
        Request::Wait { .. } => error(ErrorCode::Internal, "wait was not dispatched"),
    }
}

fn allow(state: &mut ConnectionState, per_minute: u32) -> bool {
    if state.window_start.elapsed() >= Duration::from_secs(60) {
        state.window_start = Instant::now();
        state.window_requests = 0;
    }
    state.window_requests += 1;
    state.window_requests <= per_minute
}

/// Runs a store operation on the blocking pool, mapping failures to responses.
async fn blocking<T: Send + 'static>(
    context: &Arc<Context>,
    operation: impl FnOnce(&mut Store) -> Result<T, StoreError> + Send + 'static,
) -> Result<T, Response> {
    let store = context.store.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut store = store.lock().unwrap_or_else(PoisonError::into_inner);
        operation(&mut store)
    })
    .await;
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(StoreError::NotFound)) => Err(error(ErrorCode::NotFound, "mailbox not found")),
        Ok(Err(StoreError::QuotaExceeded(reason))) => Err(error(ErrorCode::QuotaExceeded, reason)),
        Ok(Err(StoreError::Database(e))) => {
            warn!(error = %e, "database failure");
            Err(error(ErrorCode::Internal, "storage failure"))
        }
        Err(_) => Err(error(ErrorCode::Internal, "storage task failed")),
    }
}

async fn sweep_loop(context: Arc<Context>) {
    let mut interval = tokio::time::interval(Duration::from_secs(context.config.sweep_interval_seconds));
    loop {
        interval.tick().await;
        match blocking(&context, |s| s.sweep(now_ms())).await {
            Ok(0) => {}
            Ok(removed) => info!(removed, "expired items removed"),
            Err(_) => warn!("sweep failed"),
        }
    }
}

fn ok(result: Result<(), Response>) -> Response {
    result.map(|()| Response::Ok).unwrap_or_else(|response| response)
}

fn unauthenticated() -> Response {
    error(ErrorCode::Unauthenticated, "authenticate the connection first")
}

fn error(code: ErrorCode, message: &str) -> Response {
    Response::Error {
        code,
        message: message.to_owned(),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn code_hash(code: &str) -> [u8; 32] {
    *blake3::hash(code.as_bytes()).as_bytes()
}

fn load_or_create_key(path: &Path) -> Result<SecretKey, NodeError> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let mut bytes = [0u8; 32];
            hex::decode_to_slice(text.trim(), &mut bytes).map_err(|_| NodeError::Key("expected 64 hex characters"))?;
            Ok(SecretKey::from_bytes(&bytes))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(|_| NodeError::Key("random number generator failed"))?;
            write_private_file(path, hex::encode(bytes).as_bytes())?;
            info!(path = %path.display(), "generated a new node key");
            Ok(SecretKey::from_bytes(&bytes))
        }
        Err(e) => Err(e.into()),
    }
}

fn write_private_file(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.sync_all()
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

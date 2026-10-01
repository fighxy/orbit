//! Network actor. SQLite is touched only by the engine worker, never here.
//! Incoming batches wait for a durable core decision before the peer ACK.
//!
//! A mailbox node (`hex@ip:port`) keeps the original store-and-forward path.
//! A bare endpoint id is this device: it listens and dials on one Iroh
//! endpoint, and the invite carries that id instead of an IP.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ed25519_dalek::SigningKey;
use orbit_protocol::NodeAddress;
use orbit_protocol::mailbox::{Item, ItemId};
use orbit_transport::{
    DirectEndpoint, DirectEnvelope, MailboxClient, TransportError, bind_direct, client_endpoint, next_envelope,
    send_envelope,
};
use tokio::sync::{mpsc as async_mpsc, oneshot, watch};

use crate::domain::{ConnectionState, MessageId};
use crate::error::{Error, Result};
use crate::identity::LocalIdentity;
use crate::storage::{DeliveryConfig, OutboxItem};

use super::RequestId;

pub(super) enum NetworkEvent {
    Registered {
        request_id: Option<RequestId>,
        node: String,
        result: std::result::Result<(), &'static str>,
    },
    Status {
        node: String,
        state: ConnectionState,
        error: Option<&'static str>,
    },
    Incoming {
        items: Vec<Item>,
        decision: oneshot::Sender<Option<Vec<ItemId>>>,
    },
    Deposited {
        id: MessageId,
        result: std::result::Result<(), &'static str>,
    },
}

enum NetworkCommand {
    Register {
        request_id: RequestId,
        config: DeliveryConfig,
        code: Option<String>,
    },
    HostDirect {
        config: DeliveryConfig,
    },
    Deposit(OutboxItem),
}

struct DirectState {
    bound: DirectEndpoint,
    announced: bool,
    accept: tokio::task::JoinHandle<()>,
}

enum Wake {
    // Boxed: a deposit carries the envelope, and the other variants are small.
    Command(Box<Option<NetworkCommand>>),
    Inbox(std::result::Result<std::result::Result<(), &'static str>, tokio::task::JoinError>),
    Retry,
}

#[derive(Debug)]
pub(super) struct Network {
    commands: async_mpsc::Sender<NetworkCommand>,
    pub events: Receiver<NetworkEvent>,
    stop: watch::Sender<bool>,
    worker: Option<JoinHandle<()>>,
}

impl Network {
    pub fn start(identity: &LocalIdentity, peer: iroh::SecretKey, config: Option<DeliveryConfig>) -> Result<Self> {
        let key = SigningKey::from_bytes(&identity.mailbox_key().to_bytes());
        let (commands, receiver) = async_mpsc::channel(32);
        let (events, event_receiver) = mpsc::channel();
        let (stop, mut stopped) = watch::channel(false);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let worker = thread::Builder::new().name("orbit-delivery".into()).spawn(move || {
            runtime.block_on(async move {
                // Cancelling the actor interrupts connect/deposit/long-poll; close
                // never waits for the node's 30-second long-poll timeout.
                tokio::select! {
                    _ = stopped.changed() => {},
                    _ = run(key, peer, config, receiver, events) => {},
                }
            });
            runtime.shutdown_timeout(Duration::from_secs(2));
        })?;
        Ok(Self {
            commands,
            events: event_receiver,
            stop,
            worker: Some(worker),
        })
    }

    pub fn register(&self, request_id: RequestId, config: DeliveryConfig, code: Option<String>) -> Result<()> {
        self.commands
            .try_send(NetworkCommand::Register {
                request_id,
                config,
                code,
            })
            .map_err(|_| Error::Busy)
    }

    pub fn host_direct(&self, config: DeliveryConfig) -> Result<()> {
        self.commands
            .try_send(NetworkCommand::HostDirect { config })
            .map_err(|_| Error::Busy)
    }

    pub fn deposit(&self, item: OutboxItem) -> Result<()> {
        self.commands
            .try_send(NetworkCommand::Deposit(item))
            .map_err(|_| Error::Busy)
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Stable transport key for this account directory. The public half is the
/// endpoint id carried in invites, so it must survive restarts.
pub(super) fn peer_secret(dir: &Path, device_seed: &[u8; 32]) -> Result<iroh::SecretKey> {
    let path = dir.join("peer.key");
    if path.is_file() {
        return read_peer_key(&path);
    }
    // Derived, not random, so a phrase restored into an empty directory
    // publishes the same endpoint id the old invites already carry.
    let bytes = blake3::derive_key(crate::identity::DIRECT_ENDPOINT_CONTEXT, device_seed);
    let key = iroh::SecretKey::from_bytes(&bytes);
    let tmp = dir.join(".peer.key.tmp");
    std::fs::write(&tmp, key.to_bytes())?;
    if std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        if path.is_file() {
            return read_peer_key(&path);
        }
        return Err(Error::Io(std::io::Error::other("could not store the direct key")));
    }
    Ok(key)
}

fn read_peer_key(path: &Path) -> Result<iroh::SecretKey> {
    let bytes = std::fs::read(path)?;
    let array: [u8; 32] = bytes.as_slice().try_into().map_err(|_| Error::Corrupted("peer key"))?;
    Ok(iroh::SecretKey::from_bytes(&array))
}

async fn run(
    key: SigningKey,
    peer: iroh::SecretKey,
    mut config: Option<DeliveryConfig>,
    mut commands: async_mpsc::Receiver<NetworkCommand>,
    events: Sender<NetworkEvent>,
) {
    // Opening local notes alone must not bind sockets or trigger LAN permission.
    let mut deferred = if config.is_none() { commands.recv().await } else { None };
    if config.is_none() && deferred.is_none() {
        return;
    }
    let mut direct: Option<DirectState> = None;
    let mut mailbox_endpoint: Option<iroh::Endpoint> = None;
    let mut owner: Option<std::sync::Arc<MailboxClient>> = None;
    // Deposits to a mailbox never reuse an authenticated owner connection or
    // its transport identity. IP/timing correlation is still possible.
    let mut sender_endpoint: Option<iroh::Endpoint> = None;
    let mut inbox: Option<tokio::task::JoinHandle<std::result::Result<(), &'static str>>> = None;
    if config.as_ref().is_some_and(|item| item.is_direct()) {
        let node = config.as_ref().unwrap().node.clone();
        if let Err(error) = ensure_direct(&peer, &mut direct, Some(node.clone()), &events).await {
            let _ = events.send(NetworkEvent::Status {
                node,
                state: ConnectionState::Offline,
                error: Some(error),
            });
        }
    }
    let mut retry = tokio::time::interval(Duration::from_secs(5));
    retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let direct_down = config.as_ref().is_some_and(DeliveryConfig::is_direct)
            && direct.as_ref().is_none_or(|state| !state.announced);
        let mailbox_down = owner.is_none() && config.as_ref().is_some_and(|item| !item.is_direct());
        // Accept runs on its own task. Waiting for a dial here must not stop
        // us reading the peer, or two simultaneous sends deadlock.
        let wake = tokio::select! {
            command = async { if let Some(command) = deferred.take() { Some(command) } else { commands.recv().await } } => {
                Wake::Command(Box::new(command))
            }
            outcome = async { inbox.as_mut().unwrap().await }, if inbox.is_some() => Wake::Inbox(outcome),
            _ = retry.tick(), if direct_down || mailbox_down => Wake::Retry,
        };
        match wake {
            Wake::Command(command) => {
                let Some(command) = *command else { break };
                match command {
                    NetworkCommand::Register {
                        request_id,
                        config: new_config,
                        code,
                    } => {
                        close_direct(&mut direct).await;
                        if let Some(handle) = inbox.take() {
                            handle.abort();
                        }
                        if let Some(client) = owner.take() {
                            client.close();
                        }
                        let node = new_config.node.clone();
                        let _ = events.send(NetworkEvent::Status {
                            node: node.clone(),
                            state: ConnectionState::Connecting,
                            error: None,
                        });
                        config = Some(new_config);
                        let result = async {
                            open_mailbox_endpoint(&mut mailbox_endpoint).await?;
                            connect_owner(
                                mailbox_endpoint.as_ref().unwrap(),
                                &key,
                                config.as_ref().unwrap(),
                                code.as_deref(),
                            )
                            .await
                        }
                        .await;
                        let report = result.as_ref().map(|_| ()).map_err(|error| *error);
                        let _ = events.send(NetworkEvent::Registered {
                            request_id: Some(request_id),
                            node: node.clone(),
                            result: report,
                        });
                        if let Ok(client) = result {
                            inbox = Some(spawn_inbox(client.clone(), events.clone()));
                            owner = Some(client);
                        }
                        let _ = events.send(NetworkEvent::Status {
                            node,
                            state: if owner.is_some() {
                                ConnectionState::Online
                            } else {
                                ConnectionState::Offline
                            },
                            error: report.err(),
                        });
                    }
                    NetworkCommand::HostDirect { config: new_config } => {
                        if config.as_ref().is_some_and(|item| !item.is_direct()) {
                            continue;
                        }
                        let node = new_config.node.clone();
                        config = Some(new_config);
                        if let Err(error) = ensure_direct(&peer, &mut direct, Some(node.clone()), &events).await {
                            let _ = events.send(NetworkEvent::Status {
                                node,
                                state: ConnectionState::Offline,
                                error: Some(error),
                            });
                        }
                    }
                    NetworkCommand::Deposit(item) => {
                        let id = item.id;
                        let result = deliver_outbox(&peer, &mut direct, &mut sender_endpoint, &events, item).await;
                        let _ = events.send(NetworkEvent::Deposited { id, result });
                    }
                }
            }
            Wake::Inbox(outcome) => {
                inbox = None;
                if let Some(client) = owner.take() {
                    client.close();
                }
                if let Some(active) = config.as_ref().filter(|item| !item.is_direct()) {
                    let error = outcome
                        .ok()
                        .and_then(|result| result.err())
                        .unwrap_or("inbox task stopped");
                    let _ = events.send(NetworkEvent::Status {
                        node: active.node.clone(),
                        state: ConnectionState::Offline,
                        error: Some(error),
                    });
                }
            }
            Wake::Retry => {
                if direct_down {
                    let node = config.as_ref().unwrap().node.clone();
                    if let Err(error) = ensure_direct(&peer, &mut direct, Some(node.clone()), &events).await {
                        let _ = events.send(NetworkEvent::Status {
                            node,
                            state: ConnectionState::Offline,
                            error: Some(error),
                        });
                    }
                } else if let Some(active) = config.clone().filter(|item| !item.is_direct()) {
                    match async {
                        open_mailbox_endpoint(&mut mailbox_endpoint).await?;
                        connect_owner(mailbox_endpoint.as_ref().unwrap(), &key, &active, None).await
                    }
                    .await
                    {
                        Ok(client) => {
                            let _ = events.send(NetworkEvent::Registered {
                                request_id: None,
                                node: active.node.clone(),
                                result: Ok(()),
                            });
                            let _ = events.send(NetworkEvent::Status {
                                node: active.node.clone(),
                                state: ConnectionState::Online,
                                error: None,
                            });
                            inbox = Some(spawn_inbox(client.clone(), events.clone()));
                            owner = Some(client);
                        }
                        Err(error) => {
                            let _ = events.send(NetworkEvent::Status {
                                node: active.node,
                                state: ConnectionState::Offline,
                                error: Some(error),
                            });
                        }
                    }
                }
            }
        }
    }
    if let Some(handle) = inbox {
        handle.abort();
    }
    close_direct(&mut direct).await;
    if let Some(endpoint) = mailbox_endpoint {
        endpoint.close().await;
    }
    if let Some(endpoint) = sender_endpoint {
        endpoint.close().await;
    }
}

async fn ensure_direct(
    peer: &iroh::SecretKey,
    slot: &mut Option<DirectState>,
    announce: Option<String>,
    events: &Sender<NetworkEvent>,
) -> std::result::Result<(), &'static str> {
    if slot.is_none() {
        let bound = bind_direct(peer.clone())
            .await
            .map_err(|_| "cannot open network endpoint")?;
        let accept = spawn_direct_accept(bound.endpoint.clone(), events.clone());
        *slot = Some(DirectState {
            bound,
            announced: false,
            accept,
        });
    }
    if let Some(node) = announce
        && let Some(state) = slot.as_mut()
        && !state.announced
    {
        let _ = events.send(NetworkEvent::Registered {
            request_id: None,
            node: node.clone(),
            result: Ok(()),
        });
        let _ = events.send(NetworkEvent::Status {
            node,
            state: ConnectionState::Online,
            error: None,
        });
        state.announced = true;
    }
    Ok(())
}

async fn close_direct(slot: &mut Option<DirectState>) {
    if let Some(state) = slot.take() {
        state.accept.abort();
        state.bound.endpoint.close().await;
    }
}

fn spawn_direct_accept(endpoint: iroh::Endpoint, events: Sender<NetworkEvent>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match next_envelope(&endpoint).await {
                Ok(Some(delivery)) => finish_delivery(delivery, events.clone()),
                Ok(None) => break,
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
    })
}

async fn deliver_outbox(
    peer: &iroh::SecretKey,
    direct: &mut Option<DirectState>,
    sender_endpoint: &mut Option<iroh::Endpoint>,
    events: &Sender<NetworkEvent>,
    item: OutboxItem,
) -> std::result::Result<(), &'static str> {
    let address: NodeAddress = item.route.node.parse().map_err(|_| "invalid destination node")?;
    if address.addrs.is_empty() {
        ensure_direct(peer, direct, None, events).await?;
        let endpoint = &direct.as_ref().unwrap().bound.endpoint;
        timed_direct(send_envelope(endpoint, &address, &item.envelope)).await?;
        return Ok(());
    }
    if sender_endpoint.is_none() {
        *sender_endpoint = Some(
            client_endpoint()
                .await
                .map_err(|_| "cannot open outgoing network endpoint")?,
        );
    }
    let client = timed(MailboxClient::connect(sender_endpoint.as_ref().unwrap(), &address)).await?;
    let deposited = timed(client.deposit(&item.route.mailbox, &item.route.token, &item.envelope)).await;
    client.close();
    deposited?;
    Ok(())
}

fn finish_delivery(delivery: DirectEnvelope, events: Sender<NetworkEvent>) {
    tokio::spawn(async move {
        let item = Item {
            id: ItemId::of(&delivery.bytes),
            seq: 0,
            received_at_ms: 0,
            expires_at_ms: 0,
            envelope: delivery.bytes.clone(),
        };
        let (decision, receiver) = oneshot::channel();
        if events
            .send(NetworkEvent::Incoming {
                items: vec![item],
                decision,
            })
            .is_err()
        {
            return;
        }
        let stored = matches!(
            tokio::time::timeout(Duration::from_secs(30), receiver).await,
            Ok(Ok(Some(_)))
        );
        let _ = delivery.ack(stored).await;
    });
}

async fn open_mailbox_endpoint(slot: &mut Option<iroh::Endpoint>) -> std::result::Result<(), &'static str> {
    if slot.is_none() {
        *slot = Some(client_endpoint().await.map_err(|_| "cannot open network endpoint")?);
    }
    Ok(())
}

async fn connect_owner(
    endpoint: &iroh::Endpoint,
    key: &SigningKey,
    config: &DeliveryConfig,
    code: Option<&str>,
) -> std::result::Result<std::sync::Arc<MailboxClient>, &'static str> {
    let address: NodeAddress = config.node.parse().map_err(|_| "invalid node address")?;
    let mut client = timed(MailboxClient::connect(endpoint, &address)).await?;
    timed(client.authenticate(key, code)).await?;
    timed(client.add_deposit_token(&config.token)).await?;
    Ok(std::sync::Arc::new(client))
}

fn spawn_inbox(
    client: std::sync::Arc<MailboxClient>,
    events: Sender<NetworkEvent>,
) -> tokio::task::JoinHandle<std::result::Result<(), &'static str>> {
    tokio::spawn(async move {
        loop {
            // The reader's wait future is never cancelled by outgoing traffic.
            // It has its own task and one outstanding request per connection.
            let ready = tokio::time::timeout(Duration::from_secs(35), client.wait(0, 30_000))
                .await
                .map_err(|_| "inbox wait timed out")?
                .map_err(network_error)?;
            if !ready {
                continue;
            }
            let (items, _) = timed(client.fetch(0, 32)).await?;
            if items.is_empty() {
                continue;
            }
            let (decision, receiver) = oneshot::channel();
            events
                .send(NetworkEvent::Incoming { items, decision })
                .map_err(|_| "engine closed")?;
            let ids = receiver
                .await
                .map_err(|_| "engine closed")?
                .ok_or("local storage could not commit incoming messages")?;
            timed(client.ack(&ids)).await?;
        }
    })
}

async fn timed<T>(
    future: impl std::future::Future<Output = orbit_transport::Result<T>>,
) -> std::result::Result<T, &'static str> {
    tokio::time::timeout(Duration::from_secs(8), future)
        .await
        .map_err(|_| "delivery server timed out")?
        .map_err(network_error)
}

async fn timed_direct<T>(
    future: impl std::future::Future<Output = orbit_transport::Result<T>>,
) -> std::result::Result<T, &'static str> {
    tokio::time::timeout(Duration::from_secs(20), future)
        .await
        .map_err(|_| "cannot reach the contact")?
        .map_err(direct_error)
}

fn direct_error(error: TransportError) -> &'static str {
    match error {
        TransportError::Protocol("peer did not store the envelope") => "contact did not accept the message",
        _ => "cannot reach the contact",
    }
}

fn network_error(error: TransportError) -> &'static str {
    match error {
        TransportError::Remote {
            code: orbit_protocol::mailbox::ErrorCode::Forbidden,
            ..
        } => "server rejected registration or delivery access",
        TransportError::Remote { .. } => "delivery server refused the request",
        _ => "cannot reach delivery server",
    }
}

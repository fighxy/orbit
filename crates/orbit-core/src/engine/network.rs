//! Network actor. SQLite is touched only by the engine worker, never here.
//! Incoming batches wait for a durable core decision before server ACK.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ed25519_dalek::SigningKey;
use orbit_protocol::NodeAddress;
use orbit_protocol::mailbox::{Item, ItemId};
use orbit_transport::{MailboxClient, TransportError, client_endpoint};
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
    Deposit(OutboxItem),
}

#[derive(Debug)]
pub(super) struct Network {
    commands: async_mpsc::Sender<NetworkCommand>,
    pub events: Receiver<NetworkEvent>,
    stop: watch::Sender<bool>,
    worker: Option<JoinHandle<()>>,
}

impl Network {
    pub fn start(identity: &LocalIdentity, config: Option<DeliveryConfig>) -> Result<Self> {
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
                    _ = run(key, config, receiver, events) => {},
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

async fn run(
    key: SigningKey,
    mut config: Option<DeliveryConfig>,
    mut commands: async_mpsc::Receiver<NetworkCommand>,
    events: Sender<NetworkEvent>,
) {
    // Opening local notes alone must not bind sockets or trigger LAN permission.
    let mut deferred = if config.is_none() { commands.recv().await } else { None };
    if config.is_none() && deferred.is_none() {
        return;
    }
    let endpoint = match client_endpoint().await {
        Ok(endpoint) => endpoint,
        Err(_) => {
            // Keep accepting registration requests so failures remain typed.
            loop {
                let command = if let Some(command) = deferred.take() {
                    Some(command)
                } else {
                    commands.recv().await
                };
                let Some(command) = command else {
                    break;
                };
                if let NetworkCommand::Register { request_id, config, .. } = command {
                    let _ = events.send(NetworkEvent::Registered {
                        request_id: Some(request_id),
                        node: config.node,
                        result: Err("cannot open network endpoint"),
                    });
                }
            }
            return;
        }
    };
    let mut owner: Option<std::sync::Arc<MailboxClient>> = None;
    // Deposits never reuse an authenticated owner connection or its transport
    // identity. IP/timing correlation is still possible; this is not anonymity.
    let mut sender_endpoint: Option<iroh::Endpoint> = None;
    let mut inbox: Option<tokio::task::JoinHandle<std::result::Result<(), &'static str>>> = None;
    let mut retry = tokio::time::interval(Duration::from_secs(5));
    retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            command = async { if let Some(command) = deferred.take() { Some(command) } else { commands.recv().await } } => {
                match command {
                    None => break,
                    Some(NetworkCommand::Register { request_id, config: new_config, code }) => {
                        if let Some(handle) = inbox.take() { handle.abort(); }
                        if let Some(client) = owner.take() { client.close(); }
                        let node = new_config.node.clone();
                        let _ = events.send(NetworkEvent::Status { node: node.clone(), state: ConnectionState::Connecting, error: None });
                        config = Some(new_config);
                        let result = connect_owner(&endpoint, &key, config.as_ref().unwrap(), code.as_deref()).await;
                        let report = result.as_ref().map(|_| ()).map_err(|e| *e);
                        let _ = events.send(NetworkEvent::Registered { request_id: Some(request_id), node: node.clone(), result: report });
                        if let Ok(client) = result {
                            inbox = Some(spawn_inbox(client.clone(), events.clone()));
                            owner = Some(client);
                        }
                        let _ = events.send(NetworkEvent::Status { node, state: if owner.is_some() { ConnectionState::Online } else { ConnectionState::Offline }, error: report.err() });
                    }
                    Some(NetworkCommand::Deposit(item)) => {
                        let result = async {
                            let address: NodeAddress = item.route.node.parse().map_err(|_| "invalid destination node")?;
                            if sender_endpoint.is_none() {
                                sender_endpoint = Some(client_endpoint().await.map_err(|_| "cannot open outgoing network endpoint")?);
                            }
                            let client = timed(MailboxClient::connect(sender_endpoint.as_ref().unwrap(), &address)).await?;
                            let deposited = timed(client.deposit(&item.route.mailbox, &item.route.token, &item.envelope)).await;
                            client.close();
                            deposited?;
                            Ok(())
                        }.await;
                        let _ = events.send(NetworkEvent::Deposited { id: item.id, result });
                    }
                }
            }
            outcome = async { inbox.as_mut().unwrap().await }, if inbox.is_some() => {
                inbox = None;
                if let Some(client) = owner.take() { client.close(); }
                if let Some(config) = &config {
                    let error = outcome.ok().and_then(|r| r.err()).unwrap_or("inbox task stopped");
                    let _ = events.send(NetworkEvent::Status { node: config.node.clone(), state: ConnectionState::Offline, error: Some(error) });
                }
            }
            _ = retry.tick(), if owner.is_none() => {
                if let Some(config) = &config {
                    match connect_owner(&endpoint, &key, config, None).await {
                        Ok(client) => {
                            let _ = events.send(NetworkEvent::Registered { request_id: None, node: config.node.clone(), result: Ok(()) });
                            let _ = events.send(NetworkEvent::Status { node: config.node.clone(), state: ConnectionState::Online, error: None });
                            inbox = Some(spawn_inbox(client.clone(), events.clone()));
                            owner = Some(client);
                        }
                        Err(error) => {
                            let _ = events.send(NetworkEvent::Status { node: config.node.clone(), state: ConnectionState::Offline, error: Some(error) });
                        }
                    }
                }
            }
        }
    }
    if let Some(handle) = inbox {
        handle.abort();
    }
    endpoint.close().await;
    if let Some(endpoint) = sender_endpoint {
        endpoint.close().await;
    }
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

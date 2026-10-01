//! Engine facade: one worker thread per opened account, commands in, events out.
//!
//! * [`Engine::submit`] validates nothing but capacity and returns a request
//!   ID immediately; the result arrives as exactly one `command_succeeded` or
//!   `command_failed` event.
//! * [`Engine::wait_events`] blocks a client worker thread until events
//!   arrive, the timeout passes, [`Engine::cancel_wait`] or [`Engine::close`].
//! * [`Engine::close`] is idempotent, wakes all waiters and returns only after
//!   the worker has stopped and the storage lock is released.

mod network;
mod protocol;
mod queue;

use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

pub use protocol::{Command, CommandResult, Event, RequestId, SequencedEvent};
use queue::EventQueue;

use crate::domain::{ConnectionState, ConversationKind, MessageId, NetworkStatus, normalize_profile, normalize_text};
use crate::error::{Error, Result};
use crate::identity::{IdentitySecret, LocalIdentity, PublicIdentity};
use crate::limits::{EVENT_QUEUE_CAPACITY, MAX_EVENT_BATCH, MAX_IN_FLIGHT_COMMANDS};
use crate::storage::Store;
use network::{Network, NetworkEvent};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineConfig {
    /// Absolute path to an application-private directory.
    pub data_dir: PathBuf,
}

#[derive(Debug)]
struct Shared {
    queue: EventQueue,
    closed: AtomicBool,
    in_flight: AtomicUsize,
}

struct Job {
    request_id: RequestId,
    command: Command,
}

#[derive(Debug)]
pub struct Engine {
    shared: Arc<Shared>,
    commands: Mutex<Option<SyncSender<Job>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    identity: PublicIdentity,
    next_request_id: AtomicU64,
}

impl Engine {
    pub fn open(config: EngineConfig, secret: &IdentitySecret) -> Result<Self> {
        if !config.data_dir.is_absolute() {
            return Err(Error::InvalidArgument("data_dir must be an absolute path".into()));
        }
        let identity = LocalIdentity::from_secret(secret);
        let public = identity.public().clone();
        let store = Store::open(&config.data_dir, &identity)?;
        let delivery_config = store.delivery_config()?;
        let network_status = NetworkStatus {
            node: delivery_config.as_ref().map(|c| c.node.clone()),
            state: if delivery_config.is_some() {
                ConnectionState::Connecting
            } else {
                ConnectionState::Unconfigured
            },
            error: None,
        };
        let network = Network::start(&identity, delivery_config)?;

        let shared = Arc::new(Shared {
            queue: EventQueue::new(EVENT_QUEUE_CAPACITY),
            closed: AtomicBool::new(false),
            in_flight: AtomicUsize::new(0),
        });
        let (sender, receiver) = sync_channel(MAX_IN_FLIGHT_COMMANDS);
        let worker = {
            let shared = shared.clone();
            thread::Builder::new()
                .name("orbit-engine".into())
                .spawn(move || run_worker(store, receiver, shared, identity, network, network_status))?
        };

        Ok(Self {
            shared,
            commands: Mutex::new(Some(sender)),
            worker: Mutex::new(Some(worker)),
            identity: public,
            next_request_id: AtomicU64::new(1),
        })
    }

    pub fn identity(&self) -> &PublicIdentity {
        &self.identity
    }

    pub fn submit(&self, command: Command) -> Result<RequestId> {
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        if self.shared.in_flight.fetch_add(1, Ordering::AcqRel) >= MAX_IN_FLIGHT_COMMANDS {
            self.shared.in_flight.fetch_sub(1, Ordering::AcqRel);
            return Err(Error::Busy);
        }
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let sent = match self.commands.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
            None => Err(Error::Closed),
            Some(sender) => sender.try_send(Job { request_id, command }).map_err(|e| match e {
                TrySendError::Full(_) => Error::Busy,
                TrySendError::Disconnected(_) => Error::Closed,
            }),
        };
        match sent {
            Ok(()) => Ok(request_id),
            Err(error) => {
                self.shared.in_flight.fetch_sub(1, Ordering::AcqRel);
                Err(error)
            }
        }
    }

    /// Returns up to [`MAX_EVENT_BATCH`] events, or an empty batch on timeout
    /// or [`Engine::cancel_wait`]. Fails with [`Error::Closed`] after close.
    pub fn wait_events(&self, timeout: Duration) -> Result<Vec<SequencedEvent>> {
        let events = self.shared.queue.wait(MAX_EVENT_BATCH, timeout)?;
        let results = events.iter().filter(|e| e.event.is_command_result()).count();
        if results > 0 {
            self.shared.in_flight.fetch_sub(results, Ordering::AcqRel);
        }
        Ok(events)
    }

    pub fn cancel_wait(&self) {
        self.shared.queue.interrupt();
    }

    pub fn close(&self) {
        self.shared.closed.store(true, Ordering::Release);
        self.shared.queue.close();
        drop(self.commands.lock().unwrap_or_else(PoisonError::into_inner).take());
        // Holding the mutex while joining makes concurrent close calls wait
        // until the worker has stopped and released the storage lock.
        let mut worker = self.worker.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(handle) = worker.take() {
            // A worker panic is already contained per command; nothing to report.
            let _ = handle.join();
        }
    }

    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Acquire)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.close();
    }
}

fn run_worker(
    mut store: Store,
    receiver: Receiver<Job>,
    shared: Arc<Shared>,
    identity: LocalIdentity,
    network: Network,
    mut status: NetworkStatus,
) {
    let mut registration = None;
    let mut in_flight = HashSet::<MessageId>::new();
    let mut retry_at = HashMap::<MessageId, Instant>::new();
    let mut next_flush = Instant::now();
    loop {
        if shared.closed.load(Ordering::Acquire) {
            break;
        }
        while let Ok(event) = network.events.try_recv() {
            match event {
                NetworkEvent::Registered {
                    request_id,
                    node,
                    result,
                } => {
                    let result = result
                        .map_err(Error::Network)
                        .and_then(|()| store.mark_registered(&node));
                    status = NetworkStatus {
                        node: Some(node),
                        state: if result.is_ok() {
                            ConnectionState::Online
                        } else {
                            ConnectionState::Offline
                        },
                        error: result.as_ref().err().map(ToString::to_string),
                    };
                    shared.queue.push(Event::NetworkChanged {
                        network: status.clone(),
                    });
                    if let Some(id) = request_id {
                        registration = None;
                        finish_command(
                            &shared.queue,
                            id,
                            result.map(|()| CommandResult::NodeRegistered {
                                network: status.clone(),
                            }),
                        );
                    }
                    next_flush = Instant::now();
                }
                NetworkEvent::Status { node, state, error } => {
                    let updated = NetworkStatus {
                        node: Some(node),
                        state,
                        error: error.map(str::to_owned),
                    };
                    if status != updated {
                        status = updated;
                        shared.queue.push(Event::NetworkChanged {
                            network: status.clone(),
                        });
                    }
                }
                NetworkEvent::Deposited { id, result } => {
                    in_flight.remove(&id);
                    match result.and_then(|()| store.deposited(&id).map_err(|_| "could not commit delivery status")) {
                        Ok(message) => {
                            retry_at.remove(&id);
                            if let Some(message) = message {
                                shared.queue.push(Event::MessageAdded { message });
                            }
                        }
                        Err(error) => {
                            retry_at.insert(id, Instant::now() + Duration::from_secs(5));
                            status.error = Some(error.into());
                            shared.queue.push(Event::NetworkChanged {
                                network: status.clone(),
                            });
                        }
                    }
                }
                NetworkEvent::Incoming { items, decision } => {
                    let mut ids = Vec::new();
                    let mut failed = false;
                    for item in items {
                        let outcome = store.receive_item(&identity, &item, now_ms());
                        match outcome {
                            Ok(changes) => {
                                for message in changes.messages {
                                    shared.queue.push(Event::MessageAdded { message });
                                }
                                if changes.contacts_changed {
                                    shared.queue.push(Event::ContactsChanged);
                                }
                                ids.push(item.id);
                            }
                            Err(Error::InvalidArgument(_) | Error::InvalidInvite(_) | Error::NotFound(_)) => {
                                // Malformed/unauthorized items are durably quarantined,
                                // so one poison envelope cannot hold up the mailbox.
                                if store.reject_item(&item.id).is_err() {
                                    failed = true;
                                    break;
                                }
                                ids.push(item.id);
                            }
                            Err(_) => {
                                failed = true;
                                break;
                            }
                        }
                    }
                    let _ = decision.send(if failed { None } else { Some(ids) });
                    next_flush = Instant::now();
                }
            }
        }
        if status.state == ConnectionState::Online && Instant::now() >= next_flush {
            next_flush = Instant::now() + Duration::from_secs(1);
            match store.outbox(32) {
                Ok(items) => {
                    for item in items {
                        if in_flight.contains(&item.id) || retry_at.get(&item.id).is_some_and(|t| *t > Instant::now()) {
                            continue;
                        }
                        let id = item.id;
                        if network.deposit(item).is_ok() {
                            in_flight.insert(id);
                        }
                    }
                }
                Err(_) => {
                    status.error = Some("could not read the durable outbox".into());
                    shared.queue.push(Event::NetworkChanged {
                        network: status.clone(),
                    });
                }
            }
        }
        let job = match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(job) => job,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if shared.closed.load(Ordering::Acquire) {
            break;
        }
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            if let Command::RegisterNode {
                node,
                registration_code,
            } = job.command
            {
                if registration.is_some() {
                    return Err(Error::Busy);
                }
                if registration_code.as_ref().is_some_and(|code| code.len() > 1024) {
                    return Err(Error::InvalidArgument("registration code is too long".into()));
                }
                let config = store.configure_delivery(&node)?;
                network.register(job.request_id, config.clone(), registration_code)?;
                status = NetworkStatus {
                    node: Some(config.node),
                    state: ConnectionState::Connecting,
                    error: None,
                };
                shared.queue.push(Event::NetworkChanged {
                    network: status.clone(),
                });
                registration = Some(job.request_id);
                Ok(None)
            } else {
                execute(&mut store, &shared.queue, &identity, &status, job.command).map(Some)
            }
        }));
        match outcome {
            Ok(Ok(None)) => {}
            Ok(Ok(Some(result))) => finish_command(&shared.queue, job.request_id, Ok(result)),
            Ok(Err(error)) => finish_command(&shared.queue, job.request_id, Err(error)),
            Err(_) => finish_command(
                &shared.queue,
                job.request_id,
                Err(Error::Internal("command handler panicked")),
            ),
        }
        next_flush = Instant::now();
    }
}

fn finish_command(queue: &EventQueue, request_id: RequestId, result: Result<CommandResult>) {
    queue.push(match result {
        Ok(result) => Event::CommandSucceeded { request_id, result },
        Err(error) => Event::CommandFailed {
            request_id,
            error: error.info(),
        },
    });
}

fn execute(
    store: &mut Store,
    queue: &EventQueue,
    identity: &LocalIdentity,
    network: &NetworkStatus,
    command: Command,
) -> Result<CommandResult> {
    match command {
        Command::RegisterNode { .. } => Err(Error::Internal("registration must be asynchronous")),
        Command::CreateInvite => Ok(CommandResult::InviteCreated {
            text: store.create_invite(identity, now_ms())?,
        }),
        Command::InspectInvite { text } => Ok(CommandResult::InviteInspected {
            preview: store.inspect_invite(&text, now_ms())?,
        }),
        Command::AcceptInvite { text } => {
            let contact = store.accept_invite(identity, &text, now_ms())?;
            queue.push(Event::ContactsChanged);
            Ok(CommandResult::ContactAdded { contact })
        }
        Command::GetSnapshot => Ok(CommandResult::Snapshot {
            identity: identity.public().clone(),
            profile: store.profile()?,
            conversations: store.conversations()?,
            network: network.clone(),
        }),
        Command::ListMessages {
            conversation_id,
            before_seq,
            limit,
        } => Ok(CommandResult::Messages {
            page: store.messages(&conversation_id, before_seq, limit)?,
        }),
        Command::SendText { conversation_id, text } => {
            let text = normalize_text(&text)?;
            let message = match store.conversation(&conversation_id)?.kind {
                ConversationKind::SavedMessages => store.insert_text(&conversation_id, text, now_ms())?,
                ConversationKind::Direct => store.queue_text(identity, &conversation_id, text, now_ms())?,
            };
            queue.push(Event::MessageAdded {
                message: message.clone(),
            });
            Ok(CommandResult::MessageSaved { message })
        }
        Command::UpdateProfile { display_name, about } => {
            let (display_name, about) = normalize_profile(&display_name, &about)?;
            let profile = store.update_profile(display_name, about, now_ms())?;
            queue.push(Event::ProfileChanged {
                profile: profile.clone(),
            });
            Ok(CommandResult::ProfileUpdated { profile })
        }
    }
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;

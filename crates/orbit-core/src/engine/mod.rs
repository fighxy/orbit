//! Engine facade: one worker thread per opened account, commands in, events out.
//!
//! * [`Engine::submit`] validates nothing but capacity and returns a request
//!   ID immediately; the result arrives as exactly one `command_succeeded` or
//!   `command_failed` event.
//! * [`Engine::wait_events`] blocks a client worker thread until events
//!   arrive, the timeout passes, [`Engine::cancel_wait`] or [`Engine::close`].
//! * [`Engine::close`] is idempotent, wakes all waiters and returns only after
//!   the worker has stopped and the storage lock is released.

mod protocol;
mod queue;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

pub use protocol::{Command, CommandResult, Event, RequestId, SequencedEvent};
use queue::EventQueue;

use crate::domain::{normalize_profile, normalize_text};
use crate::error::{Error, Result};
use crate::identity::{IdentitySecret, LocalIdentity, PublicIdentity};
use crate::limits::{EVENT_QUEUE_CAPACITY, MAX_EVENT_BATCH, MAX_IN_FLIGHT_COMMANDS};
use crate::storage::Store;

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
        drop(identity);

        let shared = Arc::new(Shared {
            queue: EventQueue::new(EVENT_QUEUE_CAPACITY),
            closed: AtomicBool::new(false),
            in_flight: AtomicUsize::new(0),
        });
        let (sender, receiver) = sync_channel(MAX_IN_FLIGHT_COMMANDS);
        let worker = {
            let shared = shared.clone();
            let public = public.clone();
            thread::Builder::new()
                .name("orbit-engine".into())
                .spawn(move || run_worker(store, receiver, shared, public))?
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

fn run_worker(mut store: Store, receiver: Receiver<Job>, shared: Arc<Shared>, identity: PublicIdentity) {
    while let Ok(job) = receiver.recv() {
        if shared.closed.load(Ordering::Acquire) {
            break;
        }
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            execute(&mut store, &shared.queue, &identity, job.command)
        }));
        let event = match outcome {
            Ok(Ok(result)) => Event::CommandSucceeded {
                request_id: job.request_id,
                result,
            },
            Ok(Err(error)) => Event::CommandFailed {
                request_id: job.request_id,
                error: error.info(),
            },
            Err(_) => Event::CommandFailed {
                request_id: job.request_id,
                error: Error::Internal("command handler panicked").info(),
            },
        };
        shared.queue.push(event);
    }
}

fn execute(
    store: &mut Store,
    queue: &EventQueue,
    identity: &PublicIdentity,
    command: Command,
) -> Result<CommandResult> {
    match command {
        Command::GetSnapshot => Ok(CommandResult::Snapshot {
            identity: identity.clone(),
            profile: store.profile()?,
            conversations: store.conversations()?,
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
            let message = store.insert_text(&conversation_id, text, now_ms())?;
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

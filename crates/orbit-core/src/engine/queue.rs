//! Bounded event queue with blocking, cancellable waits.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use super::protocol::{Event, SequencedEvent};
use crate::error::{Error, Result};

#[derive(Debug)]
pub(crate) struct EventQueue {
    state: Mutex<State>,
    ready: Condvar,
    capacity: usize,
}

#[derive(Debug)]
struct State {
    events: VecDeque<SequencedEvent>,
    next_seq: u64,
    droppable: usize,
    resync_pending: bool,
    wake_generation: u64,
    closed: bool,
}

impl EventQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Mutex::new(State {
                events: VecDeque::new(),
                next_seq: 1,
                droppable: 0,
                resync_pending: false,
                wake_generation: 0,
                closed: false,
            }),
            ready: Condvar::new(),
            capacity,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn push(&self, event: Event) {
        let mut state = self.lock();
        if state.closed {
            return;
        }
        if event.is_droppable() {
            if state.droppable >= self.capacity {
                // Drop every queued notification and this one; keep results.
                state.events.retain(|e| !e.event.is_droppable());
                state.droppable = 0;
                if !state.resync_pending {
                    state.resync_pending = true;
                    append(&mut state, Event::ResyncRequired);
                }
                self.ready.notify_all();
                return;
            }
            state.droppable += 1;
        }
        append(&mut state, event);
        self.ready.notify_all();
    }

    /// Waits until events are available, `timeout` passes, [`interrupt`] is
    /// called or the queue is closed. Returns at most `max` events.
    ///
    /// [`interrupt`]: EventQueue::interrupt
    pub fn wait(&self, max: usize, timeout: Duration) -> Result<Vec<SequencedEvent>> {
        let deadline = Instant::now().checked_add(timeout);
        let mut state = self.lock();
        let generation = state.wake_generation;
        loop {
            if state.closed {
                return Err(Error::Closed);
            }
            if !state.events.is_empty() {
                let count = state.events.len().min(max.max(1));
                let batch: Vec<_> = state.events.drain(..count).collect();
                for event in &batch {
                    match event.event {
                        Event::ResyncRequired => state.resync_pending = false,
                        ref e if e.is_droppable() => state.droppable -= 1,
                        _ => {}
                    }
                }
                return Ok(batch);
            }
            if state.wake_generation != generation {
                return Ok(Vec::new());
            }
            let wait_for = match deadline {
                Some(deadline) => {
                    let now = Instant::now();
                    if now >= deadline {
                        return Ok(Vec::new());
                    }
                    deadline - now
                }
                None => Duration::from_secs(3600),
            };
            state = self
                .ready
                .wait_timeout(state, wait_for)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Makes current waiters return immediately with an empty batch.
    pub fn interrupt(&self) {
        let mut state = self.lock();
        state.wake_generation = state.wake_generation.wrapping_add(1);
        self.ready.notify_all();
    }

    pub fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        state.events.clear();
        self.ready.notify_all();
    }
}

fn append(state: &mut State, event: Event) {
    let seq = state.next_seq;
    state.next_seq += 1;
    state.events.push_back(SequencedEvent { seq, event });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::*;
    use crate::error::ErrorCode;
    use crate::error::ErrorInfo;

    fn result(request_id: u64) -> Event {
        Event::CommandFailed {
            request_id,
            error: ErrorInfo {
                code: ErrorCode::Internal,
                message: String::new(),
            },
        }
    }

    fn notification() -> Event {
        Event::MessageAdded {
            message: crate::domain::Message {
                id: crate::domain::MessageId::from_bytes([0; 16]),
                conversation_id: crate::domain::ConversationId::from_bytes([0; 16]),
                seq: 1,
                author_account: crate::domain::AccountId::from_bytes([0; 32]),
                author_device: crate::domain::DeviceId::from_bytes([0; 32]),
                created_at_ms: 0,
                body: crate::domain::MessageBody::Text { text: "x".into() },
                state: crate::domain::MessageState::SavedLocally,
            },
        }
    }

    #[test]
    fn sequence_numbers_increase() {
        let queue = EventQueue::new(8);
        queue.push(result(1));
        queue.push(notification());
        let batch = queue.wait(10, Duration::ZERO).unwrap();
        assert_eq!(batch.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn wait_respects_max_batch() {
        let queue = EventQueue::new(8);
        for i in 0..5 {
            queue.push(result(i));
        }
        assert_eq!(queue.wait(2, Duration::ZERO).unwrap().len(), 2);
        assert_eq!(queue.wait(10, Duration::ZERO).unwrap().len(), 3);
    }

    #[test]
    fn overflow_drops_notifications_but_keeps_results() {
        let queue = EventQueue::new(2);
        queue.push(notification());
        queue.push(result(1));
        queue.push(notification());
        queue.push(notification()); // overflow
        queue.push(notification()); // already resyncing; queued normally

        let batch = queue.wait(10, Duration::ZERO).unwrap();
        let kinds: Vec<_> = batch
            .iter()
            .map(|e| match e.event {
                Event::CommandFailed { .. } => "result",
                Event::ResyncRequired => "resync",
                Event::MessageAdded { .. } => "message",
                Event::ProfileChanged { .. } => "profile",
                Event::CommandSucceeded { .. } => "ok",
            })
            .collect();
        assert_eq!(kinds, vec!["result", "resync", "message"]);
        // Dropped events leave a gap before the resync marker.
        assert!(batch[1].seq > batch[0].seq + 1);
    }

    #[test]
    fn only_one_resync_marker_until_consumed() {
        let queue = EventQueue::new(1);
        for _ in 0..10 {
            queue.push(notification());
        }
        let batch = queue.wait(100, Duration::ZERO).unwrap();
        let resyncs = batch.iter().filter(|e| e.event == Event::ResyncRequired).count();
        assert_eq!(resyncs, 1);
    }

    #[test]
    fn wait_times_out_with_empty_batch() {
        let queue = EventQueue::new(8);
        let started = Instant::now();
        assert!(queue.wait(10, Duration::from_millis(30)).unwrap().is_empty());
        assert!(started.elapsed() >= Duration::from_millis(30));
    }

    #[test]
    fn push_wakes_waiter() {
        let queue = Arc::new(EventQueue::new(8));
        let waiter = {
            let queue = queue.clone();
            thread::spawn(move || queue.wait(10, Duration::from_secs(30)))
        };
        thread::sleep(Duration::from_millis(20));
        queue.push(result(1));
        assert_eq!(waiter.join().unwrap().unwrap().len(), 1);
    }

    #[test]
    fn interrupt_and_close_wake_waiters() {
        let queue = Arc::new(EventQueue::new(8));
        let waiter = {
            let queue = queue.clone();
            thread::spawn(move || queue.wait(10, Duration::from_secs(30)))
        };
        thread::sleep(Duration::from_millis(20));
        queue.interrupt();
        assert!(waiter.join().unwrap().unwrap().is_empty());

        let waiter = {
            let queue = queue.clone();
            thread::spawn(move || queue.wait(10, Duration::from_secs(30)))
        };
        thread::sleep(Duration::from_millis(20));
        queue.close();
        assert!(matches!(waiter.join().unwrap(), Err(Error::Closed)));
        // Closed is sticky and pushes are ignored.
        queue.push(result(2));
        assert!(matches!(queue.wait(10, Duration::ZERO), Err(Error::Closed)));
    }

    #[test]
    fn close_before_wait_is_not_lost() {
        // A close that happens before the waiter starts must still be seen.
        let queue = EventQueue::new(8);
        queue.close();
        assert!(matches!(queue.wait(10, Duration::from_secs(30)), Err(Error::Closed)));
    }
}

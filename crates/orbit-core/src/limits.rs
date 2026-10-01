//! Hard limits of the current build. Inputs above these values are rejected
//! instead of being truncated.

/// Maximum UTF-8 size of a text message body after trimming.
pub const MAX_TEXT_BYTES: usize = 16 * 1024;

/// Maximum size of one serialized command crossing the FFI boundary.
pub const MAX_COMMAND_BYTES: usize = 64 * 1024;

/// Maximum number of messages returned by one history page.
pub const MAX_PAGE_SIZE: u32 = 200;

/// Commands accepted but not yet answered by a result event.
pub const MAX_IN_FLIGHT_COMMANDS: usize = 256;

/// Droppable events kept for a client that does not read them. On overflow
/// they are replaced by a single `resync_required` event.
pub const EVENT_QUEUE_CAPACITY: usize = 1024;

/// Maximum number of events returned by one wait.
pub const MAX_EVENT_BATCH: usize = 256;

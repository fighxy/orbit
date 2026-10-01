//! C ABI. Consumed by Kotlin/Native through `include/orbit.h`.
//!
//! Conventions:
//! * Every fallible function returns `ORBIT_OK` or an `ORBIT_ERR_*` code and
//!   records a message for `orbit_last_error_message` on the calling thread.
//! * Output buffers are owned by the caller after a successful call and must
//!   be released exactly once with `orbit_buffer_free`, which also wipes them.
//! * Input pointers may be null only when the matching length is zero.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use orbit_core::{Error, ErrorCode};
use zeroize::Zeroize;

use crate::ops;

pub const ORBIT_OK: i32 = 0;
pub const ORBIT_ERR_INVALID_ARGUMENT: i32 = 1;
pub const ORBIT_ERR_INVALID_IDENTITY: i32 = 2;
pub const ORBIT_ERR_STORAGE_LOCKED: i32 = 3;
pub const ORBIT_ERR_IDENTITY_MISMATCH: i32 = 4;
pub const ORBIT_ERR_STORAGE_KEY_MISMATCH: i32 = 5;
pub const ORBIT_ERR_CORRUPTED: i32 = 6;
pub const ORBIT_ERR_UNSUPPORTED_STORAGE_VERSION: i32 = 7;
pub const ORBIT_ERR_NOT_FOUND: i32 = 8;
pub const ORBIT_ERR_STORAGE: i32 = 9;
pub const ORBIT_ERR_IO: i32 = 10;
pub const ORBIT_ERR_RANDOM: i32 = 11;
pub const ORBIT_ERR_CLOSED: i32 = 12;
pub const ORBIT_ERR_BUSY: i32 = 13;
pub const ORBIT_ERR_INTERNAL: i32 = 14;

/// Byte buffer allocated by Rust.
#[repr(C)]
#[derive(Debug)]
pub struct OrbitBuffer {
    pub data: *mut u8,
    pub len: usize,
}

impl OrbitBuffer {
    fn from_slice(bytes: &[u8]) -> Self {
        let boxed: Box<[u8]> = Box::from(bytes);
        let len = boxed.len();
        Self {
            data: Box::into_raw(boxed).cast::<u8>(),
            len,
        }
    }
}

thread_local! {
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

fn set_last_error(message: String) {
    LAST_ERROR.with(|cell| *cell.borrow_mut() = message);
}

/// Runs `body`, converting errors and panics into status codes.
fn guard(body: impl FnOnce() -> Result<(), Error>) -> i32 {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => ORBIT_OK,
        Ok(Err(error)) => {
            set_last_error(error.to_string());
            error.code() as i32
        }
        Err(_) => {
            set_last_error("internal error: native panic".into());
            ErrorCode::Internal as i32
        }
    }
}

/// # Safety
/// `data` must be null with `len == 0`, or point to `len` readable bytes.
unsafe fn input<'a>(data: *const u8, len: usize, what: &str) -> Result<&'a [u8], Error> {
    if data.is_null() {
        return if len == 0 {
            Ok(&[])
        } else {
            Err(Error::InvalidArgument(format!("{what} is null")))
        };
    }
    // SAFETY: guaranteed by the caller contract above.
    Ok(unsafe { std::slice::from_raw_parts(data, len) })
}

fn output<T>(out: *mut T, what: &str) -> Result<*mut T, Error> {
    if out.is_null() {
        Err(Error::InvalidArgument(format!("{what} is null")))
    } else {
        Ok(out)
    }
}

/// Version of the native contract. See `ORBIT_ABI_VERSION`.
#[unsafe(no_mangle)]
pub extern "C" fn orbit_abi_version() -> u32 {
    crate::ORBIT_ABI_VERSION
}

/// Generates a new identity secret for the platform secure store.
///
/// # Safety
/// `out_secret` must be a valid pointer to writable `OrbitBuffer` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orbit_identity_generate(out_secret: *mut OrbitBuffer) -> i32 {
    guard(|| {
        let out = output(out_secret, "out_secret")?;
        let secret = ops::generate_identity()?;
        // SAFETY: `out` is non-null and writable per the contract.
        unsafe { out.write(OrbitBuffer::from_slice(&secret)) };
        Ok(())
    })
}

/// Opens the account described by `secret` in the directory from
/// `config_json` (`{"data_dir": "/absolute/path"}`).
///
/// # Safety
/// Input pointers follow the module conventions; `out_engine` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orbit_engine_open(
    config_json: *const u8,
    config_len: usize,
    secret: *const u8,
    secret_len: usize,
    out_engine: *mut u64,
) -> i32 {
    guard(|| {
        let out = output(out_engine, "out_engine")?;
        // SAFETY: caller contract.
        let config = unsafe { input(config_json, config_len, "config_json") }?;
        // SAFETY: caller contract.
        let secret = unsafe { input(secret, secret_len, "secret") }?;
        let handle = ops::open(config, secret)?;
        // SAFETY: `out` is non-null and writable per the contract.
        unsafe { out.write(handle) };
        Ok(())
    })
}

/// Queues a JSON command. The result arrives later as an event carrying the
/// returned request ID.
///
/// # Safety
/// Input pointers follow the module conventions; `out_request_id` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orbit_engine_submit(
    engine: u64,
    command_json: *const u8,
    command_len: usize,
    out_request_id: *mut u64,
) -> i32 {
    guard(|| {
        let out = output(out_request_id, "out_request_id")?;
        // SAFETY: caller contract.
        let command = unsafe { input(command_json, command_len, "command_json") }?;
        let request_id = ops::submit(engine, command)?;
        // SAFETY: `out` is non-null and writable per the contract.
        unsafe { out.write(request_id) };
        Ok(())
    })
}

/// Blocks up to `timeout_ms` (capped at 60 s) for events and writes
/// `{"events":[...]}`. Returns `ORBIT_ERR_CLOSED` once the engine is closed.
///
/// # Safety
/// `out_events` must be a valid pointer to writable `OrbitBuffer` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orbit_engine_wait_events(engine: u64, timeout_ms: u32, out_events: *mut OrbitBuffer) -> i32 {
    guard(|| {
        let out = output(out_events, "out_events")?;
        let json = ops::wait_events(engine, timeout_ms)?;
        // SAFETY: `out` is non-null and writable per the contract.
        unsafe { out.write(OrbitBuffer::from_slice(&json)) };
        Ok(())
    })
}

/// Makes current `orbit_engine_wait_events` calls return an empty batch.
#[unsafe(no_mangle)]
pub extern "C" fn orbit_engine_cancel_wait(engine: u64) -> i32 {
    guard(|| ops::cancel_wait(engine))
}

/// Closes the engine, wakes waiters and returns after the worker stopped.
/// Closing an already closed handle succeeds.
#[unsafe(no_mangle)]
pub extern "C" fn orbit_engine_close(engine: u64) -> i32 {
    guard(|| ops::close(engine))
}

/// Wipes and releases a buffer returned by this library. Null data is ignored.
///
/// # Safety
/// `buffer` must come from this library and must not be used or freed again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orbit_buffer_free(buffer: OrbitBuffer) {
    if buffer.data.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the buffer was produced by `OrbitBuffer::from_slice` from a
        // boxed slice of exactly `len` bytes and ownership returns here once.
        let mut boxed = unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(buffer.data, buffer.len)) };
        boxed.zeroize();
    }));
}

/// Copies the message of the last failed call on this thread.
///
/// # Safety
/// `out_message` must be a valid pointer to writable `OrbitBuffer` storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orbit_last_error_message(out_message: *mut OrbitBuffer) -> i32 {
    if out_message.is_null() {
        return ORBIT_ERR_INVALID_ARGUMENT;
    }
    let message = LAST_ERROR.with(|cell| cell.borrow().clone());
    // SAFETY: checked for null above; writable per the contract.
    unsafe { out_message.write(OrbitBuffer::from_slice(message.as_bytes())) };
    ORBIT_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(buffer: OrbitBuffer) -> Vec<u8> {
        // SAFETY: buffer comes from this library and is freed once below.
        let bytes = unsafe { std::slice::from_raw_parts(buffer.data, buffer.len) }.to_vec();
        // SAFETY: freed exactly once.
        unsafe { orbit_buffer_free(buffer) };
        bytes
    }

    fn last_error() -> String {
        let mut buffer = OrbitBuffer {
            data: ptr::null_mut(),
            len: 0,
        };
        // SAFETY: valid out pointer.
        assert_eq!(unsafe { orbit_last_error_message(&mut buffer) }, ORBIT_OK);
        String::from_utf8(take(buffer)).unwrap()
    }

    #[test]
    fn status_codes_match_core_error_codes() {
        let pairs = [
            (ORBIT_ERR_INVALID_ARGUMENT, ErrorCode::InvalidArgument),
            (ORBIT_ERR_INVALID_IDENTITY, ErrorCode::InvalidIdentity),
            (ORBIT_ERR_STORAGE_LOCKED, ErrorCode::StorageLocked),
            (ORBIT_ERR_IDENTITY_MISMATCH, ErrorCode::IdentityMismatch),
            (ORBIT_ERR_STORAGE_KEY_MISMATCH, ErrorCode::StorageKeyMismatch),
            (ORBIT_ERR_CORRUPTED, ErrorCode::Corrupted),
            (
                ORBIT_ERR_UNSUPPORTED_STORAGE_VERSION,
                ErrorCode::UnsupportedStorageVersion,
            ),
            (ORBIT_ERR_NOT_FOUND, ErrorCode::NotFound),
            (ORBIT_ERR_STORAGE, ErrorCode::Storage),
            (ORBIT_ERR_IO, ErrorCode::Io),
            (ORBIT_ERR_RANDOM, ErrorCode::Random),
            (ORBIT_ERR_CLOSED, ErrorCode::Closed),
            (ORBIT_ERR_BUSY, ErrorCode::Busy),
            (ORBIT_ERR_INTERNAL, ErrorCode::Internal),
        ];
        for (constant, code) in pairs {
            assert_eq!(constant, code as i32, "{code:?}");
        }
    }

    #[test]
    fn full_round_trip_through_c_abi() {
        let dir = tempfile::tempdir().unwrap();
        let mut secret = OrbitBuffer {
            data: ptr::null_mut(),
            len: 0,
        };
        // SAFETY: valid out pointer.
        assert_eq!(unsafe { orbit_identity_generate(&mut secret) }, ORBIT_OK);
        let secret = take(secret);

        let config = serde_json::to_vec(&serde_json::json!({ "data_dir": dir.path() })).unwrap();
        let mut engine = 0u64;
        // SAFETY: pointers and lengths describe live slices.
        let status = unsafe {
            orbit_engine_open(
                config.as_ptr(),
                config.len(),
                secret.as_ptr(),
                secret.len(),
                &mut engine,
            )
        };
        assert_eq!(status, ORBIT_OK, "{}", last_error());

        let command = br#"{"type":"get_snapshot"}"#;
        let mut request = 0u64;
        // SAFETY: pointers and lengths describe live slices.
        let status = unsafe { orbit_engine_submit(engine, command.as_ptr(), command.len(), &mut request) };
        assert_eq!(status, ORBIT_OK);

        let mut events = OrbitBuffer {
            data: ptr::null_mut(),
            len: 0,
        };
        // SAFETY: valid out pointer.
        let status = unsafe { orbit_engine_wait_events(engine, 5_000, &mut events) };
        assert_eq!(status, ORBIT_OK);
        let batch: serde_json::Value = serde_json::from_slice(&take(events)).unwrap();
        assert_eq!(batch["events"][0]["event"]["request_id"], request);

        assert_eq!(orbit_engine_cancel_wait(engine), ORBIT_OK);
        assert_eq!(orbit_engine_close(engine), ORBIT_OK);
        assert_eq!(orbit_engine_close(engine), ORBIT_OK);
        let mut events = OrbitBuffer {
            data: ptr::null_mut(),
            len: 0,
        };
        // SAFETY: valid out pointer.
        let status = unsafe { orbit_engine_wait_events(engine, 0, &mut events) };
        assert_eq!(status, ORBIT_ERR_CLOSED);
        assert!(last_error().contains("closed"));
    }

    #[test]
    fn null_pointers_are_rejected() {
        // SAFETY: null out pointers are part of the tested contract.
        unsafe {
            assert_eq!(orbit_identity_generate(ptr::null_mut()), ORBIT_ERR_INVALID_ARGUMENT);
            assert_eq!(
                orbit_engine_open(ptr::null(), 5, ptr::null(), 0, &mut 0),
                ORBIT_ERR_INVALID_ARGUMENT
            );
            assert_eq!(
                orbit_engine_submit(1, ptr::null(), 0, ptr::null_mut()),
                ORBIT_ERR_INVALID_ARGUMENT
            );
            assert_eq!(orbit_last_error_message(ptr::null_mut()), ORBIT_ERR_INVALID_ARGUMENT);
            orbit_buffer_free(OrbitBuffer {
                data: ptr::null_mut(),
                len: 0,
            });
        }
    }

    #[test]
    fn empty_buffers_round_trip() {
        let buffer = OrbitBuffer::from_slice(&[]);
        assert!(!buffer.data.is_null());
        assert!(take(buffer).is_empty());
    }
}

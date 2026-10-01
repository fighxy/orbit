//! Safe operations shared by the C and JNI adapters.

use std::time::Duration;

use orbit_core::limits::MAX_COMMAND_BYTES;
use orbit_core::{Command, Engine, EngineConfig, Error, IdentitySecret, Result, SequencedEvent};
use serde::Serialize;
use zeroize::Zeroizing;

use crate::handles;

/// Upper bound for one blocking wait; clients loop for longer waits.
const MAX_WAIT: Duration = Duration::from_secs(60);

pub(crate) fn generate_identity() -> Result<Zeroizing<Vec<u8>>> {
    Ok(IdentitySecret::generate()?.to_bytes())
}

pub(crate) fn open(config_json: &[u8], secret: &[u8]) -> Result<u64> {
    let config: EngineConfig = parse_json(config_json, "engine config")?;
    let secret = IdentitySecret::from_bytes(secret)?;
    let engine = Engine::open(config, &secret)?;
    Ok(handles::insert(engine))
}

pub(crate) fn submit(handle: u64, command_json: &[u8]) -> Result<u64> {
    let engine = handles::get(handle)?;
    let command: Command = parse_json(command_json, "command")?;
    engine.submit(command)
}

#[derive(Serialize)]
struct EventBatch<'a> {
    events: &'a [SequencedEvent],
}

/// Returns `{"events":[...]}`; an empty list means timeout or cancellation.
pub(crate) fn wait_events(handle: u64, timeout_ms: u32) -> Result<Vec<u8>> {
    let engine = handles::get(handle)?;
    let timeout = Duration::from_millis(u64::from(timeout_ms)).min(MAX_WAIT);
    let events = engine.wait_events(timeout)?;
    serde_json::to_vec(&EventBatch { events: &events }).map_err(|_| Error::Internal("event serialization failed"))
}

pub(crate) fn cancel_wait(handle: u64) -> Result<()> {
    handles::get(handle)?.cancel_wait();
    Ok(())
}

/// Closes the engine and returns after its worker stopped. Idempotent.
pub(crate) fn close(handle: u64) -> Result<()> {
    if let Some(engine) = handles::remove(handle)? {
        // Other threads may still hold the Arc (for example inside a wait);
        // close wakes them and they fail with `closed`.
        engine.close();
    }
    Ok(())
}

/// Parses JSON without echoing input in the error: commands carry message text.
fn parse_json<T: serde::de::DeserializeOwned>(bytes: &[u8], what: &str) -> Result<T> {
    if bytes.len() > MAX_COMMAND_BYTES {
        return Err(Error::InvalidArgument(format!(
            "{what} exceeds {MAX_COMMAND_BYTES} bytes"
        )));
    }
    serde_json::from_slice(bytes).map_err(|e| {
        Error::InvalidArgument(format!(
            "invalid {what} JSON ({:?} error at line {}, column {})",
            e.classify(),
            e.line(),
            e.column()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(dir: &std::path::Path) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({ "data_dir": dir })).unwrap()
    }

    #[test]
    fn parse_errors_do_not_echo_input() {
        let secret_text = "very private words";
        let json = format!(r#"{{"type":"list_messages","conversation_id":"{secret_text}","limit":1}}"#);
        let error = parse_json::<Command>(json.as_bytes(), "command").unwrap_err();
        assert!(!error.to_string().contains(secret_text), "{error}");
    }

    #[test]
    fn oversized_command_is_rejected() {
        let big = vec![b' '; MAX_COMMAND_BYTES + 1];
        assert!(matches!(
            parse_json::<Command>(&big, "command"),
            Err(Error::InvalidArgument(_))
        ));
    }

    #[test]
    fn handle_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let secret = generate_identity().unwrap();
        let handle = open(&config(dir.path()), &secret).unwrap();

        let request = submit(handle, br#"{"type":"get_snapshot"}"#).unwrap();
        let batch: serde_json::Value = serde_json::from_slice(&wait_events(handle, 5_000).unwrap()).unwrap();
        assert_eq!(batch["events"][0]["event"]["request_id"], request);
        assert_eq!(batch["events"][0]["event"]["type"], "command_succeeded");

        close(handle).unwrap();
        close(handle).unwrap();
        assert!(matches!(
            submit(handle, br#"{"type":"get_snapshot"}"#),
            Err(Error::Closed)
        ));
        assert!(matches!(wait_events(handle, 0), Err(Error::Closed)));
        assert!(matches!(cancel_wait(handle), Err(Error::Closed)));
        assert!(matches!(submit(0, b"{}"), Err(Error::InvalidArgument(_))));
        assert!(matches!(close(u64::MAX), Err(Error::InvalidArgument(_))));

        // Handles are never reused.
        let reopened = open(&config(dir.path()), &secret).unwrap();
        assert_ne!(reopened, handle);
        close(reopened).unwrap();
    }

    #[test]
    fn rejects_bad_config_and_secret() {
        let dir = tempfile::tempdir().unwrap();
        let secret = generate_identity().unwrap();
        assert!(matches!(open(b"{}", &secret), Err(Error::InvalidArgument(_))));
        assert!(matches!(
            open(br#"{"data_dir":"relative"}"#, &secret),
            Err(Error::InvalidArgument(_))
        ));
        assert!(matches!(
            open(&config(dir.path()), &secret[1..]),
            Err(Error::InvalidIdentity)
        ));
    }
}

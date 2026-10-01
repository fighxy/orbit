//! Registry of open engines addressed by never-reused handles.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};

use orbit_core::{Engine, Error, Result};

struct Registry {
    engines: HashMap<u64, Arc<Engine>>,
    next: u64,
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| {
    Mutex::new(Registry {
        engines: HashMap::new(),
        next: 1,
    })
});

fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn insert(engine: Engine) -> u64 {
    let mut registry = registry();
    let handle = registry.next;
    registry.next += 1;
    registry.engines.insert(handle, Arc::new(engine));
    handle
}

/// Returns the engine for `handle`. Handles that were issued and later closed
/// fail with [`Error::Closed`]; values never issued are invalid arguments.
pub(crate) fn get(handle: u64) -> Result<Arc<Engine>> {
    let registry = registry();
    match registry.engines.get(&handle) {
        Some(engine) => Ok(engine.clone()),
        None => Err(missing(handle, registry.next)),
    }
}

/// Removes `handle` from the registry. Closing an already closed handle is
/// allowed and returns `None`.
pub(crate) fn remove(handle: u64) -> Result<Option<Arc<Engine>>> {
    let mut registry = registry();
    match registry.engines.remove(&handle) {
        Some(engine) => Ok(Some(engine)),
        None => match missing(handle, registry.next) {
            Error::Closed => Ok(None),
            other => Err(other),
        },
    }
}

fn missing(handle: u64, next: u64) -> Error {
    if handle == 0 || handle >= next {
        Error::InvalidArgument("unknown engine handle".into())
    } else {
        Error::Closed
    }
}

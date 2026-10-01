//! Native boundary of the Orbit engine.
//!
//! Two thin adapters share one set of safe operations ([`ops`]):
//!
//! * `c_api`: C ABI for Kotlin/Native (iOS) via cinterop; the header is
//!   generated into `include/orbit.h` by cbindgen.
//! * `jni_api`: JNI for Android and the JVM desktop client.
//!
//! Commands, results and events cross the boundary as UTF-8 JSON defined by
//! `orbit_core::engine`. Engines are addressed by opaque `u64` handles that
//! are never reused, so a stale handle fails with `closed` instead of touching
//! freed memory. No panic crosses the boundary.

pub mod c_api;
mod handles;
#[cfg(not(target_os = "ios"))]
mod jni_api;
mod ops;

/// Version of the native contract: C functions, JNI methods and the JSON
/// protocol. Bumped on any incompatible change; the SDK refuses to start on a
/// mismatch.
pub const ORBIT_ABI_VERSION: u32 = 1;

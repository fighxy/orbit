//! JNI adapter for `com.orbit.sdk.bridge.OrbitJni` (Android and JVM desktop).
//!
//! Failures throw `com.orbit.sdk.bridge.OrbitNativeException(int code,
//! String message)` with the same codes as the C ABI.

use jni::errors::ErrorPolicy;
use jni::objects::{JByteArray, JClass, JThrowable, JValue};
use jni::strings::JNIString;
use jni::sys::{jint, jlong};
use jni::{Env, EnvUnowned, jni_sig, jni_str};
use orbit_core::{Error, ErrorCode};
use zeroize::Zeroizing;

use crate::ops;

enum Failure {
    Orbit(Error),
    Jni(jni::errors::Error),
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Failure::Orbit(error)
    }
}

impl From<jni::errors::Error> for Failure {
    fn from(error: jni::errors::Error) -> Self {
        Failure::Jni(error)
    }
}

/// Throws `OrbitNativeException` unless a Java exception is already pending.
struct ThrowOrbitException;

impl<T: Default> ErrorPolicy<T, Failure> for ThrowOrbitException {
    type Captures<'unowned_env_local: 'native_method, 'native_method> = ();

    fn on_error<'unowned_env_local: 'native_method, 'native_method>(
        env: &mut Env<'unowned_env_local>,
        _captures: &mut Self::Captures<'unowned_env_local, 'native_method>,
        failure: Failure,
    ) -> jni::errors::Result<T> {
        if !env.exception_check() {
            let (code, message) = match failure {
                Failure::Orbit(error) => (error.code(), error.to_string()),
                Failure::Jni(error) => (ErrorCode::Internal, format!("internal error: JNI failure: {error}")),
            };
            throw(env, code, &message);
        }
        Ok(T::default())
    }

    fn on_panic<'unowned_env_local: 'native_method, 'native_method>(
        env: &mut Env<'unowned_env_local>,
        _captures: &mut Self::Captures<'unowned_env_local, 'native_method>,
        _payload: Box<dyn std::any::Any + Send + 'static>,
    ) -> jni::errors::Result<T> {
        if !env.exception_check() {
            throw(env, ErrorCode::Internal, "internal error: native panic");
        }
        Ok(T::default())
    }
}

fn throw(env: &mut Env<'_>, code: ErrorCode, message: &str) {
    let thrown = (|| -> jni::errors::Result<()> {
        let message = env.new_string(message)?;
        let exception = env.new_object(
            jni_str!("com/orbit/sdk/bridge/OrbitNativeException"),
            jni_sig!("(ILjava/lang/String;)V"),
            &[JValue::Int(code as jint), JValue::Object(&message)],
        )?;
        let exception = env.cast_local::<JThrowable>(exception)?;
        env.throw(exception)
    })();
    if thrown.is_err() && !env.exception_check() {
        let _ = env.throw_new(jni_str!("java/lang/IllegalStateException"), JNIString::from(message));
    }
}

fn read_bytes(env: &Env<'_>, array: &JByteArray<'_>) -> Result<Vec<u8>, Failure> {
    if array.is_null() {
        return Err(Error::InvalidArgument("byte array is null".into()).into());
    }
    Ok(env.convert_byte_array(array)?)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_orbit_sdk_bridge_OrbitJni_nativeAbiVersion<'caller>(
    _env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> jint {
    crate::ORBIT_ABI_VERSION as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_orbit_sdk_bridge_OrbitJni_nativeGenerateIdentity<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JByteArray<'caller> {
    unowned_env
        .with_env(|env| -> Result<_, Failure> {
            let secret = ops::generate_identity()?;
            Ok(env.byte_array_from_slice(&secret)?)
        })
        .resolve::<ThrowOrbitException>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_orbit_sdk_bridge_OrbitJni_nativeLockIdentity<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    secret: JByteArray<'caller>,
    passcode: JByteArray<'caller>,
) -> JByteArray<'caller> {
    unowned_env
        .with_env(|env| -> Result<_, Failure> {
            let secret = Zeroizing::new(read_bytes(env, &secret)?);
            let passcode = Zeroizing::new(read_bytes(env, &passcode)?);
            let locked = ops::lock_identity(&secret, &passcode)?;
            Ok(env.byte_array_from_slice(&locked)?)
        })
        .resolve::<ThrowOrbitException>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_orbit_sdk_bridge_OrbitJni_nativeUnlockIdentity<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    locked: JByteArray<'caller>,
    passcode: JByteArray<'caller>,
) -> JByteArray<'caller> {
    unowned_env
        .with_env(|env| -> Result<_, Failure> {
            let locked = read_bytes(env, &locked)?;
            let passcode = Zeroizing::new(read_bytes(env, &passcode)?);
            let secret = ops::unlock_identity(&locked, &passcode)?;
            Ok(env.byte_array_from_slice(&secret)?)
        })
        .resolve::<ThrowOrbitException>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_orbit_sdk_bridge_OrbitJni_nativeOpen<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    config_json: JByteArray<'caller>,
    secret: JByteArray<'caller>,
) -> jlong {
    unowned_env
        .with_env(|env| -> Result<_, Failure> {
            let config = read_bytes(env, &config_json)?;
            let secret = Zeroizing::new(read_bytes(env, &secret)?);
            Ok(ops::open(&config, &secret)? as jlong)
        })
        .resolve::<ThrowOrbitException>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_orbit_sdk_bridge_OrbitJni_nativeSubmit<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    engine: jlong,
    command_json: JByteArray<'caller>,
) -> jlong {
    unowned_env
        .with_env(|env| -> Result<_, Failure> {
            let command = Zeroizing::new(read_bytes(env, &command_json)?);
            Ok(ops::submit(engine as u64, &command)? as jlong)
        })
        .resolve::<ThrowOrbitException>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_orbit_sdk_bridge_OrbitJni_nativeWaitEvents<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    engine: jlong,
    timeout_ms: jint,
) -> JByteArray<'caller> {
    unowned_env
        .with_env(|env| -> Result<_, Failure> {
            let timeout_ms =
                u32::try_from(timeout_ms).map_err(|_| Error::InvalidArgument("timeout must not be negative".into()))?;
            let json = Zeroizing::new(ops::wait_events(engine as u64, timeout_ms)?);
            Ok(env.byte_array_from_slice(&json)?)
        })
        .resolve::<ThrowOrbitException>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_orbit_sdk_bridge_OrbitJni_nativeCancelWait<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    engine: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<_, Failure> { Ok(ops::cancel_wait(engine as u64)?) })
        .resolve::<ThrowOrbitException>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_orbit_sdk_bridge_OrbitJni_nativeClose<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    engine: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<_, Failure> { Ok(ops::close(engine as u64)?) })
        .resolve::<ThrowOrbitException>()
}

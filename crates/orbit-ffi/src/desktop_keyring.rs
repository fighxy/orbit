//! OS keyring for `com.orbit.sdk.platform.DesktopSecretStore` (JVM desktop).
//!
//! Backends: macOS Keychain, Windows Credential Manager, Secret Service on
//! Linux/BSD. There is no file fallback: when the keyring is unavailable the
//! call throws `SecureStorageException` and nothing is written elsewhere.

use jni::objects::{JByteArray, JClass, JString};
use jni::strings::JNIString;
use jni::{Env, EnvUnowned, jni_str};
use keyring::Entry;
use zeroize::Zeroizing;

const MAX_SECRET_BYTES: usize = 4096;

enum Failure {
    Keyring(String),
    Jni(jni::errors::Error),
}

impl From<jni::errors::Error> for Failure {
    fn from(error: jni::errors::Error) -> Self {
        Failure::Jni(error)
    }
}

impl From<keyring::Error> for Failure {
    fn from(error: keyring::Error) -> Self {
        // keyring messages describe the platform failure, never secret bytes.
        Failure::Keyring(error.to_string())
    }
}

struct ThrowSecureStorageException;

impl<T: Default> jni::errors::ErrorPolicy<T, Failure> for ThrowSecureStorageException {
    type Captures<'unowned_env_local: 'native_method, 'native_method> = ();

    fn on_error<'unowned_env_local: 'native_method, 'native_method>(
        env: &mut Env<'unowned_env_local>,
        _captures: &mut Self::Captures<'unowned_env_local, 'native_method>,
        failure: Failure,
    ) -> jni::errors::Result<T> {
        if !env.exception_check() {
            let message = match failure {
                Failure::Keyring(message) => format!("OS keyring is unavailable: {message}"),
                Failure::Jni(error) => format!("OS keyring call failed: {error}"),
            };
            throw(env, &message);
        }
        Ok(T::default())
    }

    fn on_panic<'unowned_env_local: 'native_method, 'native_method>(
        env: &mut Env<'unowned_env_local>,
        _captures: &mut Self::Captures<'unowned_env_local, 'native_method>,
        _payload: Box<dyn std::any::Any + Send + 'static>,
    ) -> jni::errors::Result<T> {
        if !env.exception_check() {
            throw(env, "OS keyring call panicked");
        }
        Ok(T::default())
    }
}

fn throw(env: &mut Env<'_>, message: &str) {
    let message = JNIString::from(message);
    if env
        .throw_new(jni_str!("com/orbit/sdk/platform/SecureStorageException"), &message)
        .is_err()
        && !env.exception_check()
    {
        let _ = env.throw_new(jni_str!("java/lang/IllegalStateException"), &message);
    }
}

fn entry(env: &Env<'_>, service: &JString<'_>, account: &JString<'_>) -> Result<Entry, Failure> {
    // Report why the platform store failed to initialize instead of the
    // generic "no default store" error from Entry::new.
    if let Err(error) = Entry::store_status() {
        return Err(Failure::Keyring(error.to_string()));
    }
    let service = service.try_to_string(env)?;
    let account = account.try_to_string(env)?;
    Ok(Entry::new(&service, &account)?)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_orbit_sdk_platform_DesktopKeyring_nativeRead<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    service: JString<'caller>,
    account: JString<'caller>,
) -> JByteArray<'caller> {
    unowned_env
        .with_env(|env| -> Result<_, Failure> {
            match entry(env, &service, &account)?.get_secret() {
                Ok(secret) => {
                    let secret = Zeroizing::new(secret);
                    Ok(env.byte_array_from_slice(&secret)?)
                }
                Err(keyring::Error::NoEntry) => Ok(JByteArray::default()),
                Err(error) => Err(error.into()),
            }
        })
        .resolve::<ThrowSecureStorageException>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_orbit_sdk_platform_DesktopKeyring_nativeWrite<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    service: JString<'caller>,
    account: JString<'caller>,
    secret: JByteArray<'caller>,
) {
    unowned_env
        .with_env(|env| -> Result<_, Failure> {
            if secret.is_null() {
                return Err(Failure::Keyring("secret is null".into()));
            }
            let secret = Zeroizing::new(env.convert_byte_array(&secret)?);
            if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
                return Err(Failure::Keyring("secret size is out of range".into()));
            }
            Ok(entry(env, &service, &account)?.set_secret(&secret)?)
        })
        .resolve::<ThrowSecureStorageException>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_orbit_sdk_platform_DesktopKeyring_nativeDelete<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    service: JString<'caller>,
    account: JString<'caller>,
) {
    unowned_env
        .with_env(|env| -> Result<_, Failure> {
            match entry(env, &service, &account)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(error.into()),
            }
        })
        .resolve::<ThrowSecureStorageException>()
}

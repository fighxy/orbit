# Rust ↔ Kotlin Multiplatform bridge (ABI 6)

Status: implemented. Source of truth is `crates/orbit-ffi/src` and generated
`crates/orbit-ffi/include/orbit.h`. `ORBIT_ABI_VERSION` and
`shared/.../bridge/NativeEngine.kt` `SUPPORTED_ABI_VERSION` are both 6.
This note is the contract, not a UI test report.

## Layers

| Layer | Where | Responsibility |
|---|---|---|
| `orbit-core` | `crates/orbit-core` | Identity, storage, engine, JSON command and event protocol |
| `orbit-ffi` | `crates/orbit-ffi` | C ABI, JNI, handle table, panic catch, error codes |
| KMP SDK | `shared/` | `NativeLibrary` / `NativeEngine`, `OrbitSdk`, `OrbitClient`, secure stores |
| UI | `client/`, `apps/` | Compose screens and state. No key or database access |

One safe operation set (`ops.rs`) is called from the C ABI (iOS via cinterop)
and from JNI (Android and JVM desktop). Kotlin does not duplicate the
protocol, cryptography, or storage.

## Functions

| C ABI | JNI (`com.orbit.sdk.bridge.OrbitJni`) | Role |
|---|---|---|
| `orbit_abi_version` | `nativeAbiVersion` | Contract version. The SDK refuses a mismatch |
| `orbit_identity_generate` | `nativeGenerateIdentity` | New account and device secret for the secure store |
| `orbit_identity_lock` / `_unlock` | `nativeLockIdentity` / `nativeUnlockIdentity` | Seal the secret with a passcode, and open it |
| `orbit_engine_open` | `nativeOpen` | Open an account: `{"data_dir": "/abs/path"}` plus the secret |
| `orbit_engine_submit` | `nativeSubmit` | JSON command → request id |
| `orbit_engine_wait_events` | `nativeWaitEvents` | Blocking wait for an event batch, up to 60 s |
| `orbit_engine_cancel_wait` | `nativeCancelWait` | Wake the wait with an empty batch |
| `orbit_engine_close` | `nativeClose` | Close the engine after the worker stops and the lock drops |
| `orbit_buffer_free` | — | Zero and free a Rust buffer |
| `orbit_last_error_message` | — | Last error text on this thread |

JVM desktop also uses `com.orbit.sdk.platform.DesktopKeyring`
(`desktop_keyring.rs`): Keychain, Credential Manager, Secret Service. That is
a secure-store adapter, not part of the engine contract.

## ABI rules

- **Handle.** Engines are `u64` values that are never reused. A closed handle
  returns `closed` and does not touch freed memory.
- **Errors.** `0` is success. Otherwise `ORBIT_ERR_*` (1–18), the same code in
  C, JNI (`OrbitNativeException(code, message)`), and JSON (`snake_case`).
  A panic does not cross the boundary.
- **Memory.** A buffer has one owner. `orbit_buffer_free` wipes it. An input
  pointer may be `NULL` only when the length is zero.
- **Secrets.** The identity secret moves only between the engine and the
  secure store. Kotlin wipes its copies after use. JSON parse errors do not
  quote the command text.
- **Events.** Each command yields exactly one `command_succeeded` or
  `command_failed`, and that result is never dropped. Notifications
  (`message_added`, `profile_changed`, `contacts_changed`, `network_changed`)
  collapse to one `resync_required` when the queue overflows. The client
  loads a snapshot again.
- **Correlation.** `OrbitClient` sends a command and registers the wait under
  one lock, so the result cannot arrive before the wait is registered.

## Command protocol (JSON, tag `type`)

| Command | Result | Notification |
|---|---|---|
| `get_snapshot` | `snapshot { identity, profile, conversations, network }` | — |
| `list_messages { conversation_id, before_seq?, limit }` | `messages { page }` | — |
| `send_text { conversation_id, text }` | `message_saved { message }` | `message_added` |
| `edit_text { conversation_id, message_id, text }` | `message_saved { message }` | `message_added` |
| `delete_text { conversation_id, message_id }` | `message_saved { message }` | `message_added` |
| `create_group { title, members }` | `room_created { conversation }` | `contacts_changed` |
| `create_channel { title, members }` | `room_created { conversation }` | `contacts_changed` |
| `register_node { node, registration_code? }` | `node_registered { network }` | `network_changed` |
| `create_invite` | `invite_created { text }` | — |
| `inspect_invite { text }` | `invite_inspected { preview }` | — |
| `accept_invite { text }` | `contact_added { contact }` | `contacts_changed` |
| `update_profile { display_name, about }` | `profile_updated { profile }` | `profile_changed` |
| `set_avatar { image }` | `profile_updated { profile }` | `profile_changed` |
| `send_voice { conversation_id, wav_base64 }` | `message_saved { message }` | `message_added` |
| `read_voice { message_id }` | `voice { message_id, wav_base64 }` | — |

`image` and `wav_base64` are standard base64 with padding. An empty `image`
clears the picture. A picture is JPEG or PNG, at most 32 KiB, and travels as
its own envelope (`Avatar`, postcard index 10), not inside the invite. A
voice note is a 16 kHz mono 16-bit PCM WAV, at most 60 seconds, only in a
direct chat or in saved messages. The core splits it into 32 KiB slices
(`MediaStart` index 11, `MediaChunk` index 12) and checks SHA-256 before the
message appears. Groups and channels reject it. Editing a non-text body is
rejected. There is no phrase command: onboarding still creates the random
identity secret.

`members` are conversation ids of ready direct contacts, not account keys.
`create_group` and `create_channel` are pairwise fan-out, not MLS. See
[vertical slice](../vertical-slice.md).

`Conversation.kind` is `saved_messages`, `direct`, `group`, or `channel`.
`title` is optional and omitted when `none`. `can_post` is omitted when true
(the default). A channel subscriber sends `can_post: false`. `revision` is
omitted when zero. `deleted` is omitted when false.

Rust rejects unknown command fields. Kotlin ignores unknown event fields.
Any incompatible change bumps `ORBIT_ABI_VERSION`. Golden JSON tests in
`shared/src/commonTest` encode these commands. They are not a UI pass, and
this note does not claim they were re-run.

## Passcode

Sealed secret layout: `0x10 | m_cost u32 | t_cost u32 | p_cost u8 | salt[16] |
nonce[24] | ciphertext`. The key is Argon2id (64 MiB, 3 passes, 1 lane) of the
passcode. The cipher is XChaCha20-Poly1305. The header is authenticated.
Unlock clamps KDF parameters so a crafted blob cannot demand unbounded memory.

The passcode protects the key from other processes that can read the OS store
(on desktop, often any program of the same user). A short numeric code only
slows guessing of a copied blob. The UI pause after failures is in memory and
does not replace Argon2.

## Build

| Platform | Library | How it is built |
|---|---|---|
| JVM desktop | `liborbit_ffi.so` / `.dylib` / `.dll` | `:shared:cargoBuildHost`; path via `-Dorbit.native.library` |
| Android | `liborbit_ffi.so` for arm64-v8a, x86_64 | `:apps:android:cargoNdkBuild` (cargo-ndk, NDK 29) |
| iOS | `liborbit_ffi.a` | `cargoBuildIos*` on macOS; cinterop links it into the klib |

An installer built before ABI 6 will not load this SDK. This note does not
ask for a new MSI. A debug APK with `liborbit_ffi.so` for arm64-v8a and
x86_64 was assembled locally and was not installed on a device.

## Not checked

- The iOS bridge and Keychain on a device or simulator (CI compiles them).
- The Android app on a device (a debug APK with both ABIs was assembled).
- Keystore or Keychain behavior across device lock and backup restore.
- Clicking the group or channel dialog. The screens are in `client/` and `shared/`.

## Delivery (ABI 6)

Two routes share one outbox.

A mailbox node (`hex@ip:port`, ALPN `orbit/mailbox/1`) keeps a QUIC connection
with relays disabled. Receive uses `Wait`, not periodic `Fetch`. The network
thread hands a batch to the engine worker and waits: `Ack` runs only after a
durable local commit. Bad or unauthorized envelopes are recorded as rejected
before ACK and do not block the rest. A storage failure does not ACK.

A bare endpoint id is a direct session (ALPN `orbit/direct/1`, Iroh
`presets::N0`). The invite carries that id, not an IP. The engine does not
call `Endpoint::online()`. There is no store-and-forward on this path: a
closed app does not accumulate incoming envelopes. Same-process tests use a
process-local socket map; separate processes do not.

`message_added` is an upsert by message id, so an edit or delete updates the
row. `queued` is the local history-plus-outbox transaction. `mailbox` is set
when the route has accepted the stored envelope (a node deposit, or a direct
peer's store ACK). A room message stays `queued` until every fan-out copy has
been accepted. `delivered` is the recipient's checked receipt. For a room that
is one receipt from each other member. `received`
is a stored incoming message. None of these is a read receipt. Contacts,
routes, and the node configuration are encrypted on disk.

Text can be queued as soon as the invite is accepted. It stays in the outbox
until the contact exchange is confirmed. Retry and restart keep the same
ciphertext. Close cancels the network actor and an active long-poll.

Edits and deletes are sealed `EditText` / `DeleteText` (postcard indexes 4 and
5) or, in a room, `RoomEditText` / `RoomDeleteText` bound to the room id.
Only the author. Revision is monotonic. Delete is terminal. An edit that
arrives before the text waits in `pending_message_ops` (cap 64), not in the
quarantine. Room text or edits that arrive before the welcome wait in the
pending room tables (cap 64 each); overflow asks the sender to retry instead
of quarantining. Group and channel sends are one sealed copy per other
member, at most 8 members including the creator. That is not MLS.

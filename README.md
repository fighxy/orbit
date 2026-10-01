# Orbit

Orbit is an early-stage messenger project with a planned **Rust core** and
**Kotlin Multiplatform bridges and clients**. The planned shared UI uses Compose
Multiplatform across Android, iOS, Windows, Linux, and macOS.

The product combines personal and group chats, communities with persistent voice
rooms, broadcast channels, files, voice messages, and video notes. Orbit will use
its own application protocol. Holepunch is an architectural reference; wire and
storage compatibility with Holepunch are not project requirements.

## Current status

Stage 0 of the [roadmap](docs/roadmap.md) is implemented: a working, local-only
messenger client on the Rust core.

| Works today | Verified by |
|---|---|
| Account and device identity (Ed25519), account-signed device certificate | Rust unit tests |
| Identity secret kept only in the OS secure store (desktop keyring, Android Keystore, iOS Keychain); no plaintext fallback | JVM tests; desktop run with and without Secret Service |
| Optional passcode that seals the stored secret with Argon2id + XChaCha20-Poly1305 | Rust, C ABI and JVM integration tests; desktop run |
| SQLite storage owned by Rust: WAL + `synchronous=FULL`, message bodies encrypted with the device key, schema migrations, single-writer lock | Rust tests incl. 1000 messages across restart, tampering, v1→v2 upgrade |
| Engine worker with request IDs, bounded event queue, `resync_required`, cancellable waits | Rust and Kotlin tests incl. close/cancel races |
| C ABI (generated `orbit.h`) and JNI; Kotlin SDK over both | C smoke test, JVM integration tests |
| Compose UI: onboarding (profile, passcode), lock screen, chat list, "Saved messages" chat with history paging, settings | Desktop run under Xvfb; Android debug APK build |

The vertical slice adds registration on a selected `orbit-node`, verified
`orbit://invite/…` contact exchange, and encrypted one-to-one text messaging in
the common Windows/Android/iOS UI. The core persists contacts and routing secrets
under the local device key, saves message history and its ciphertext outbox in
one transaction, retries the same envelope, receives through long-poll and ACKs
only after a durable local commit. Delivery states distinguish queued, stored
on the node and stored by the recipient.

The interim HPKE scheme has **no forward secrecy**: compromise of a device inbox
key exposes previously recorded envelopes addressed to it. MLS/ratcheting,
groups, channels, voice, attachments, backup, push and background delivery remain
outside this slice. `orbit-node` is a separate program; clients initiate outgoing
connections and do not host a mailbox. Device runs on Windows/Android/iOS are
still acceptance steps; successful builds alone do not verify OS lifecycle.

See [vertical-slice setup and acceptance](docs/vertical-slice.md).

## Planned architecture

| Area | Responsibility |
|---|---|
| Rust core | Domain rules, protocol, identity, encryption integration, persistence, synchronization, and delivery |
| KMP SDK | Common client API, DTOs, event streams, lifecycle, and native bridge adapters |
| Compose Multiplatform | Shared screens, navigation, and ViewModels |
| Platform adapters | Secure storage, notifications, file access, capture/playback, and media SDK integration |
| Infrastructure | Relay, encrypted offline storage, push delivery, and SFU for voice rooms |

Initial Rust development starts with `orbit-core` and `orbit-ffi`. Networking,
storage, crypto, and domain areas begin as internal modules. The more detailed
crate split in the architecture is a future extraction map, not a prerequisite
for the first working client.

The KMP bridge is planned as C ABI/cinterop on iOS and JNI on Android/JVM desktop.
The Rust core owns message persistence; platform UI databases must not become
independent sources of truth for the same messages.

### Product boundaries

- **Conversations:** authorized participants exchange messages.
- **Broadcast channels:** publishers create posts; subscribers receive them.
- **Communities:** organize membership, permissions, and text/voice spaces.
- **Voice rooms:** persistent room definitions with temporary real-time sessions.
- **Voice and video notes:** stored attachments, separate from live audio transport.

P2P data exchange can use relay and offline-delivery infrastructure. Group voice
uses an SFU. Availability depends on reachable peers/services and OS lifecycle
constraints; the design does not promise operation without any always-on nodes.

## Technology decisions and experiments

| Status | Decision |
|---|---|
| Selected direction | Rust core; KMP bridges and clients; an independent Orbit application protocol |
| Proposed UI | Compose Multiplatform, validated on actual target devices |
| Transport candidate | Iroh, subject to connection, relay, lifecycle, and platform experiments |
| E2EE candidate | OpenMLS, subject to authorization, persistence, recovery, and group-state experiments |
| Voice baseline | A ready self-hosted SFU for the first experiment; LiveKit is a candidate |
| Later research | A Rust SFU using str0m, evaluated against the working baseline |

LiveKit's server is a third-party Go component, not a Rust implementation of
Orbit. The final media stack is still open. Transport encryption, message E2EE,
and media E2EE are separate integration requirements.

## Repository layout

| Path | Status | Purpose |
|---|---|---|
| `crates/orbit-core/` | Implemented (stage 0) | Identity, storage, engine facade, client protocol |
| `crates/orbit-ffi/` | Implemented (stage 0) | C ABI, JNI, desktop keyring adapter, generated `include/orbit.h` |
| `shared/` | Implemented (stage 0) | KMP SDK: `OrbitSdk`, `OrbitClient`, bridges, secure stores |
| `client/` | Implemented (stage 0) | Shared Compose UI and state holders |
| `apps/desktop/`, `apps/android/` | Implemented (stage 0) | JVM desktop and Android entry points |
| `apps/ios/` | Implemented, not run on a device | XcodeGen host showing `MainViewController()` from the `OrbitClient` framework |
| `crates/orbit-protocol/`, `crates/orbit-transport/` | Implemented (mailbox and envelopes) | Wire protocol types; QUIC client over Iroh |
| `services/orbit-node/` | Implemented (mailbox) | Node: store-and-forward mailbox with TTL, quotas, owner auth, deposit tokens |
| `crates/orbit-cli/` | Implemented | Test tool for a node mailbox (no E2EE; test data only) |
| `deploy/orbit-node/` | Implemented, not run on a VPS | Dockerfile, hardened systemd unit, deployment notes |
| `protocol/` | Started | [`mailbox.md`](protocol/mailbox.md): `orbit/mailbox/1` |
| `tools/` | Existing | C ABI smoke test; Holepunch reference inventory |
| `docs/` | Existing | Architecture, review, ADR, reference map, and implementation plan |

## Build and run

Requirements: Rust 1.97 (pinned in `rust-toolchain.toml`), JDK 21, and for
Android the SDK with API 37, NDK 29.0.14206865 and `cargo install cargo-ndk`.

```sh
cargo test --workspace                 # Rust core and FFI
./gradlew :shared:jvmTest :client:jvmTest
./gradlew :apps:desktop:run            # desktop app; needs an OS keyring
./gradlew :apps:android:assembleDebug  # builds liborbit_ffi.so via cargo-ndk
tools/ffi-smoke/run.sh                 # C header + static library
```

### Windows

CI builds a per-user MSI on `windows-latest` and attaches it to the run as the
`orbit-windows-msi` artifact. Locally on Windows (Rust MSVC toolchain, JDK 21):

```sh
./gradlew :apps:desktop:run                                        # development
./gradlew :apps:desktop:packageMsi -Porbit.cargoProfile=release   # installer
```

- Installs into the user profile without administrator rights; Start menu and
  desktop shortcuts are created. The installer is not code-signed yet, so
  SmartScreen warns on first launch.
- The account secret is stored in Windows Credential Manager (service
  `com.orbit.messenger`); data lives in `%LOCALAPPDATA%\Orbit\<profile>`.
- Open Contacts to register on a reachable node, exchange invitations and send
  encrypted text messages. Local "Saved messages" remains available offline.

iOS (macOS with Xcode): `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`,
`brew install xcodegen`, then `cd apps/ios && xcodegen generate` and open
`Orbit.xcodeproj`; the build phase compiles the Kotlin framework and the Rust
static library.

On Linux the desktop app needs a running Secret Service (GNOME Keyring or
KWallet). `ORBIT_PROFILE=name` starts an isolated profile with its own data
directory and keyring entry. After changing the C ABI, regenerate the header
with the command at the top of `crates/orbit-ffi/cbindgen.toml`; CI rejects a
stale header.

## Implementation order

1. Establish a reproducible build, correct source sets, secure storage, and a
   minimal KMP screen invoking Rust.
2. Validate native lifecycle, private-message persistence, and a three-device
   voice prototype before committing to transport/crypto/media libraries.
3. Deliver a usable personal chat with restart and offline recovery.
4. Add a persistent voice room and a publisher/subscriber channel.
5. Add files/photos, voice messages, and video notes.
6. Extend to private groups, multiple devices, recovery, and wider platform testing.
7. Extract libraries and replace infrastructure where measured requirements justify it.

Every step includes a working client and checks for its own data and lifecycle
risks. Full acceptance criteria are in the [roadmap](docs/roadmap.md).

## Documentation

The detailed planning and review documents are in Russian.

- [Documentation index](docs/README.md)
- [Rust/KMP architecture, revision 2](docs/architecture/rust-kmp.md)
- [Rust ↔ KMP bridge, ABI v3](docs/architecture/kmp-bridge.md)
- [Architecture review and existing scaffold findings](docs/reviews/2026-10-01-architecture-review.md)
- [Implementation roadmap and acceptance gates](docs/roadmap.md)
- [ADR 0001: Rust core, KMP clients, independent protocol](docs/adr/0001-rust-core-kmp-clients.md)
- [Holepunch reference map](docs/holepunch-map.md)

## License

Orbit's project license has not been selected (`UNLICENSED` in the historical
package manifest). Third-party dependencies retain their respective licenses.

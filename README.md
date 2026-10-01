# Orbit

Orbit is an early-stage messenger project with a planned **Rust core** and
**Kotlin Multiplatform bridges and clients**. The planned shared UI uses Compose
Multiplatform across Android, iOS, Windows, Linux, and macOS.

The product combines personal and group chats, communities with persistent voice
rooms, broadcast channels, files, voice messages, and video notes. Orbit will use
its own application protocol. Holepunch is an architectural reference; wire and
storage compatibility with Holepunch are not project requirements.

## Current status

Stage 0 of the [roadmap](docs/roadmap.md) is implemented, and the vertical
slice is in this tree: a messenger client on the Rust core, with direct
invites, optional mailbox delivery, edits, pairwise rooms, avatars, and
voice notes.

| Works today | Verified by |
|---|---|
| Account and device identity (Ed25519), account-signed device certificate | Rust unit tests |
| Identity secret kept only in the OS secure store (desktop keyring, Android Keystore, iOS Keychain); no plaintext fallback | JVM tests; desktop run with and without Secret Service |
| Optional passcode that seals the stored secret with Argon2id + XChaCha20-Poly1305 | Rust, C ABI and JVM integration tests; desktop run |
| SQLite storage owned by Rust: WAL + `synchronous=FULL`, message bodies encrypted with the device key, schema migrations, single-writer lock | Rust tests incl. 1000 messages across restart, tampering, in-place upgrade from schema v1. Current schema version is 7 |
| Engine worker with request IDs, bounded event queue, `resync_required`, cancellable waits | Rust and Kotlin tests incl. close/cancel races |
| C ABI (generated `orbit.h`) and JNI; Kotlin SDK over both | C smoke test, JVM integration tests |
| Compose UI: onboarding (profile, passcode), lock screen, chat list with a local title and preview filter, open chat, "Saved messages", settings, JPEG avatar, microphone for a direct or saved voice note on Android and desktop | `DesktopMediaTest` covers the 32/48 kHz fold and JPEG scaling. The desktop window was not clicked and no new MSI was built. Android debug APK assembled, not installed on a phone |

The vertical slice does not start by pasting a node address. After a display
name is set, the engine binds a direct Iroh endpoint (`presets::N0`, ALPN
`orbit/direct/1`). An `orbit://invite/…` link is a signed contact card. On
that path the node field is a 64-hex endpoint id, not an IP. The transport
key is `data_dir/accounts/<account>/peer.key`. `Endpoint::online()` is not
called at startup. Same-process tests use an in-process socket map; two OS
processes do not. The mailbox path (`hex@ip:port`, ALPN `orbit/mailbox/1`,
relays off) is still there and is optional. **Become a node** /
`orbit-node host` publishes a same-LAN IPv4, which mobile data cannot route.

The core stores contacts and routing secrets under the local device key,
writes history and the ciphertext outbox in one transaction, retries the same
envelope, and ACKs only after a durable local commit. Author edit and delete
are sealed payloads. Pairwise groups and channels fan out a separate sealed
copy to each member (at most 8, including the creator). That is not MLS.
There is no username search, no account transfer, and no multidevice.
Onboarding asks for a display name and an optional passcode and stores a
random identity. A 24-word phrase in the Rust identity type restores the
same account, device, and direct address. It is not in the FFI and not on
the screen. `normalize_username` is a helper, not a directory and not the
registration field. A closed app does not queue incoming direct envelopes.

The interim HPKE scheme has **no forward secrecy**: compromise of a device inbox
key exposes previously recorded envelopes addressed to it. MLS, voice rooms,
video notes, file attachments, backup, push, and background delivery are not
started. A voice note is a chunked 16 kHz PCM WAV, at most 60 seconds, in a
direct chat or in saved messages. Android records and plays it. Desktop
records and plays through the JVM mixer and scales a chosen JPEG; those
helpers were unit-tested, the window was not clicked, and no new MSI was
built. iOS does not record or play. There is no SFU. The native contract is ABI 6.
An installed MSI from ABI 5 or earlier will not load this SDK. The debug APK
`apps/android/build/outputs/apk/debug/android-debug.apk` was built from this
tree and was not installed on a phone. This file does not ask for a new MSI.
Device runs are still acceptance steps; a successful build does not verify OS
lifecycle. The mailbox integration test has not been re-run after the rooms
outbox join.

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
| Transport in the tree | Iroh. Direct sessions use `presets::N0`. Mailbox dials pasted sockets with relays disabled. Device, NAT, and battery trials are not done |
| E2EE in the tree | Interim HPKE, no forward secrecy. OpenMLS is still only a candidate |
| Voice baseline | Not started. A ready self-hosted SFU was the proposed first experiment; LiveKit is a candidate. No SFU is in the tree |
| Later research | A Rust SFU using str0m, evaluated against the working baseline |

LiveKit's server is a third-party Go component, not a Rust implementation of
Orbit. The final media stack is still open. Transport encryption, message E2EE,
and media E2EE are separate integration requirements.

## Repository layout

| Path | Status | Purpose |
|---|---|---|
| `crates/orbit-core/` | Implemented (stage 0) | Identity, storage, engine facade, client protocol |
| `crates/orbit-ffi/` | Implemented (ABI 6) | C ABI, JNI, desktop keyring adapter, generated `include/orbit.h` |
| `shared/` | Implemented (stage 0) | KMP SDK: `OrbitSdk`, `OrbitClient`, bridges, secure stores |
| `client/` | Implemented (stage 0) | Shared Compose UI and state holders |
| `apps/desktop/`, `apps/android/` | Implemented (stage 0) | JVM desktop and Android entry points |
| `apps/ios/` | Implemented, not run on a device | XcodeGen host showing `MainViewController()` from the `OrbitClient` framework |
| `crates/orbit-protocol/`, `crates/orbit-transport/` | Implemented | Envelopes, invites, mailbox protocol; direct ALPN `orbit/direct/1` and mailbox QUIC |
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
- Set a display name, then exchange an `orbit://invite/…` link. A node address
  is only for an optional mailbox, including **Become a node** on the same
  Wi-Fi. Local "Saved messages" stays available offline. The UI can also
  create a pairwise group or channel from ready contacts. That screen has
  not been click-tested for this tree.

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
   minimal KMP screen invoking Rust. _In the tree (stage 0)._
2. Validate native lifecycle, private-message persistence, and a three-device
   voice prototype before treating transport, MLS, or media as chosen.
   _Lifecycle and the voice prototype are not done. Direct Iroh sessions and
   interim HPKE are what the tree uses today._
3. Personal chat with restart. _Direct invite, edit, and delete are in the
   tree. Offline delivery exists only on the optional mailbox path and has
   not been re-proven after rooms. A closed app does not queue direct
   envelopes._
4. A persistent voice room, and a broadcast channel that is not a full mesh.
   _Not started. The pairwise channel in the tree is not that channel._
5. Files and photos, then voice messages and video notes. _Voice notes are chunked PCM WAV in the core. Android record and playback are in the client and were not tapped on a phone. Desktop records and plays through javax.sound and picks a JPEG; the window was not clicked and no new MSI was built. Video notes and file attachments are not started._
6. MLS groups, multiple devices, recovery, and wider platform testing.
   _Not started. The pairwise group is not MLS._
7. Extract libraries and replace infrastructure where measured requirements justify it.

Every step includes a working client and checks for its own data and lifecycle
risks. Full acceptance criteria are in the [roadmap](docs/roadmap.md).

## Documentation

The index, roadmap, bridge, and vertical slice are in English. The architecture
design and the 2026-10-01 review stay in Russian; the design file has an
English status note at the top.

- [Documentation index](docs/README.md)
- [Rust/KMP architecture, revision 2](docs/architecture/rust-kmp.md)
- [Rust ↔ KMP bridge, ABI 6](docs/architecture/kmp-bridge.md)
- [Architecture review and existing scaffold findings](docs/reviews/2026-10-01-architecture-review.md)
- [Implementation roadmap and acceptance gates](docs/roadmap.md)
- [ADR 0001: Rust core, KMP clients, independent protocol](docs/adr/0001-rust-core-kmp-clients.md)
- [Holepunch reference map](docs/holepunch-map.md)

## License

Orbit's project license has not been selected (`UNLICENSED` in the historical
package manifest). Third-party dependencies retain their respective licenses.

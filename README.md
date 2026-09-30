# Orbit

Orbit is an early-stage messenger project with a planned **Rust core** and
**Kotlin Multiplatform bridges and clients**. The planned shared UI uses Compose
Multiplatform across Android, iOS, Windows, Linux, and macOS.

The product combines personal and group chats, communities with persistent voice
rooms, broadcast channels, files, voice messages, and video notes. Orbit will use
its own application protocol. Holepunch is an architectural reference; wire and
storage compatibility with Holepunch are not project requirements.

## Current status

The repository currently contains a Kotlin Multiplatform scaffold, models,
platform storage placeholders, and design documentation. The Rust core, native
bridges, client apps, P2P replication, E2EE integration, and voice infrastructure
are **not implemented yet**.

The scaffold is not ready for real account secrets or private conversations:

- Desktop key storage currently writes a Base64-encoded seed.
- Desktop message storage writes payload lengths rather than message contents.
- Android and iOS key storage contain unfinished operations.
- Desktop source sets mix JVM-only code with Kotlin/Native targets.
- Public identity and message models need to be separated from secret material.

These are tracked as the first implementation tasks in the
[roadmap](docs/roadmap.md). The [review](docs/reviews/2026-10-01-architecture-review.md)
records source evidence and distinguishes static findings from future tests.

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
| `shared/` | Existing scaffold | Evolves into the KMP SDK and bridge/platform adapters |
| `docs/` | Existing | Architecture, review, ADR, reference map, and implementation plan |
| `package.json` | Existing historical inventory | Earlier Holepunch dependency list; no implemented JS bridge |
| `crates/orbit-core/` | Planned | Rust core and internal modules |
| `crates/orbit-ffi/` | Planned | Native API boundary |
| `client/` | Planned | Shared Compose UI |
| `apps/android/`, `apps/ios/`, `apps/desktop/` | Planned | Entry points and platform packaging |
| `services/orbit-node/` | Planned | Headless infrastructure introduced with offline delivery |
| `protocol/` | Planned | Schemas, versions, limits, and test vectors |
| `deploy/` | Planned | Infrastructure configuration |

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
- [Architecture review and existing scaffold findings](docs/reviews/2026-10-01-architecture-review.md)
- [Implementation roadmap and acceptance gates](docs/roadmap.md)
- [ADR 0001: Rust core, KMP clients, independent protocol](docs/adr/0001-rust-core-kmp-clients.md)
- [Holepunch reference map](docs/holepunch-map.md)

## License

Orbit's project license has not been selected (`UNLICENSED` in the historical
package manifest). Third-party dependencies retain their respective licenses.

# Holepunch Component Map

This document is a reference map of the [Holepunch ecosystem](https://github.com/holepunchto), not Orbit's implemented dependency graph. Orbit is planned around its own Rust protocol, with Kotlin Multiplatform bridges and clients.

See the [architecture decision](adr/0001-rust-core-kmp-clients.md), [reviewed architecture](architecture/rust-kmp.md), and [implementation roadmap](roadmap.md). The architecture document maps these functions to proposed Orbit modules.

## Layer overview

| Layer | Reference components |
|---|---|
| Applications | Keet, PearPass, Autopass, Hypershell, Hyperbeam |
| Deployment | pear, pear-runtime, pear-install |
| JavaScript runtime | bare, bare-kit, workers, libjs and alternative engines |
| Protocol | compact-encoding, protomux, secret-stream, hrpc |
| Offline delivery | blind-peering, blind-peer, blind-peer-muxer |
| Invitations and identity | blind-pairing, keet-identity-key, keypear |
| Files | hyperdrive, localdrive, mirror-drive, hyperblobs |
| Collaboration and data | autobase, hyperdb, hypercore, hyperbee, corestore |
| Networking | hyperswarm, hyperdht, libudx |
| Native foundation | libsodium, libuv, CMake |

## Networking

| Module | Role |
|---|---|
| `hyperswarm` | High-level P2P API: join topics, manage connections, automatic reconnection |
| `hyperdht` | Kademlia DHT: peer discovery, UDP hole-punching, node announcements |
| `libudx` (C) | Reliable, multiplexed, congestion-controlled streams over UDP |
| `@hyperswarm/secret-stream` | Noise-encrypted streams between peers |

## Data

| Module | Role |
|---|---|
| `hypercore` | Secure append-only log, one writer per core, signed blocks |
| `corestore` | Factory managing collections of Hypercores on disk |
| `hyperbee` | Sorted key-value store (B-tree) on top of a Hypercore |
| `hyperdb` | Schema-based P2P database with replication |

## Collaboration

| Module | Role |
|---|---|
| `autobase` | Multi-writer log: merges every peer's Hypercore into one shared view |

## Files

| Module | Role |
|---|---|
| `hyperdrive` | Distributed file system (Hyperbee metadata + Hyperblobs content) |
| `localdrive` | Local filesystem interoperable with Hyperdrive |
| `mirror-drive` | Mirror between Hyperdrive and Localdrive |
| `hyperblobs` | Large binary blob storage for Hyperdrive |

## Identity

| Module | Role |
|---|---|
| `keet-identity-key` | HD Ed25519 keypairs from a 24-word seed phrase |
| `keypear` | Keychain with sub-keys and attestations |

## Invites

| Module | Role |
|---|---|
| `blind-pairing` | Join a room from a short invite code without exposing keys |
| `blind-pairing-core` | Core invite/request/confirm protocol |

## Delivery

| Module | Role |
|---|---|
| `blind-peering` | Client that asks blind peers to keep cores available offline |
| `blind-peer` | Server-side blind peer (optional, self-hosted) |
| `blind-peer-muxer` | Protomux channel muxer for blind peers |

## Protocol

| Module | Role |
|---|---|
| `protomux` | Multiplex multiple message-oriented protocols over one stream |
| `compact-encoding` | Compact binary parsers/serialisers |
| `hyperbeam` | 1-to-1 end-to-end encrypted pipe over Hyperswarm |
| `hrpc` | RPC framework over Protomux |

## Runtime

| Module | Role |
|---|---|
| `bare` | Small modular JS runtime for desktop and mobile (C) |
| `bare-kit` | Embed Bare into native apps (iOS/Android) |
| `bare-worker` / `bare-thread` | Worker threads and thread support |
| `libjs` | ABI-stable C bindings to V8 (default engine) |
| `libqjs` / `libjsc` / `libjerry` / `libmqjs` | Swappable engines: QuickJS, JavaScriptCore, JerryScript, Micro QuickJS |
| `bare-fs` / `bare-pack` / `bare-bundle` | Filesystem, packing, bundling for Bare |

## Deploy

| Module | Role |
|---|---|
| `pear` | CLI: build, stage, seed, and update P2P apps |
| `pear-runtime` | Embeddable runtime with P2P over-the-air updates |
| `pear-install` | Install Pear and Pear applications |

## Native foundation (C)

| Library | Role |
|---|---|
| `libsodium` | Ed25519, X25519, AEAD (crypto primitives) |
| `libuv` | Async I/O, event loop |
| CMake | Cross-platform build system |

## Design patterns worth stealing

1. **Append-only signed log** (Hypercore) as the unit of replication —
   every room is a set of cores, not a database row.
2. **Multi-writer merge** (Autobase) instead of a single server-side
   ordering — conflicts resolved by linearisation, not locks.
3. **Blind peers** for offline delivery — encrypted replicas held by
   untrusted nodes. Content encryption does not by itself conceal IP addresses,
   timing, traffic volume, or all participant metadata.
4. **Invitation protocols** (blind-pairing) as a reference for authenticated
   joining. Orbit needs its own specified capability and verification rules.
5. **Swappable JS engine** (libjs) — the same app can run on V8,
   QuickJS, or JavaScriptCore without code changes.

## What orbit takes from this

- Reuse the relevant ideas in a Rust core with a versioned Orbit application protocol.
- Keep bridges, platform adapters, and clients in Kotlin Multiplatform.
- Start with `orbit-core` and `orbit-ffi`; extract other crates when their boundaries are demonstrated.
- Use existing transport, cryptographic, and media libraries after platform experiments.
- JavaScript engines, Bare, and Pear are reference material, not planned Orbit runtime requirements.
- Root `package.json` currently remains a historical dependency inventory. Its cleanup is tracked in the roadmap; installed packages do not demonstrate an implemented integration.
- Protocol and storage compatibility with Holepunch are not requirements of the selected design.


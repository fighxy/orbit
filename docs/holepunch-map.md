# Holepunch Component Map

Reference architecture of the Holepunch stack (728 repos in the
[holepunchto](https://github.com/holepunchto) org). This is a map for
studying their design — orbit's own protocol will be written in Kotlin,
using these modules only as a reference.

## Layer overview

```
┌─────────────────────────────────────────────────────────┐
│  Apps: Keet, PearPass, Autopass, Hypershell, Hyperbeam   │
├─────────────────────────────────────────────────────────┤
│  Deploy: pear (CLI), pear-runtime, pear-install         │
├─────────────────────────────────────────────────────────┤
│  Runtime: bare, bare-kit, bare-worker, bare-thread      │
│           libjs / libqjs / libjsc / libjerry / libmqjs  │
├─────────────────────────────────────────────────────────┤
│  Protocol: protomux, compact-encoding,                  │
│            @hyperswarm/secret-stream, hyperbeam, hrpc   │
├─────────────────────────────────────────────────────────┤
│  Delivery: blind-peering, blind-peer, blind-peer-muxer  │
├─────────────────────────────────────────────────────────┤
│  Invites: blind-pairing, blind-pairing-core             │
├─────────────────────────────────────────────────────────┤
│  Identity: keet-identity-key, keypear                   │
├─────────────────────────────────────────────────────────┤
│  Files: hyperdrive, localdrive, mirror-drive, hyperblobs│
├─────────────────────────────────────────────────────────┤
│  Collaboration: autobase, hyperdb                       │
├─────────────────────────────────────────────────────────┤
│  Data: hypercore, hyperbee, corestore                   │
├─────────────────────────────────────────────────────────┤
│  Networking: hyperswarm, hyperdht, libudx               │
├─────────────────────────────────────────────────────────┤
│  Native: libsodium (crypto), libuv (I/O), CMake         │
└─────────────────────────────────────────────────────────┘
```

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
3. **Blind peers** for offline delivery — encrypted mirrors held by
   untrusted nodes, no server knows content or participants.
4. **Invite codes** (blind-pairing) that never expose long-term keys.
5. **Swappable JS engine** (libjs) — the same app can run on V8,
   QuickJS, or JavaScriptCore without code changes.

## What orbit takes from this

- Architecture patterns above → reimplemented in Kotlin.
- Nothing is vendored or linked at runtime; the map is a study aid.

# Orbit

P2P messenger client built on the Holepunch stack. No servers, no accounts — messages, calls, and files travel directly between devices over an end-to-end encrypted peer-to-peer network.

## Stack

| Layer | Module | npm | Role |
|---|---|---|---|
| Networking | Hyperswarm | `hyperswarm` | Topic-based peer discovery, NAT traversal, connection management |
| Networking | HyperDHT | `hyperdht` | Kademlia DHT, UDP hole-punching, Noise-encrypted streams |
| Data | Hypercore | `hypercore` | Secure append-only log (one writer per core) |
| Data | Corestore | `corestore` | Factory managing collections of Hypercores on disk |
| Data | Hyperbee | `hyperbee` | Sorted key-value store on top of Hypercore |
| Data | HyperDB | `hyperdb` | Schema-based database with P2P replication |
| Collaboration | Autobase | `autobase` | Multi-writer log: merges every peer's core into one shared view |
| Files | Hyperdrive | `hyperdrive` | Distributed file system for media and attachments |
| Files | Localdrive | `localdrive` | Local filesystem interoperable with Hyperdrive |
| Files | Mirrordrive | `mirror-drive` | Mirror between Hyperdrive and Localdrive |
| Identity | keet-identity-key | `keet-identity-key` | Hierarchical deterministic Ed25519 keypairs from a 24-word seed phrase |
| Identity | keypear | `keypear` | Keychain with sub-keys and attestations |
| Invites | blind-pairing | `blind-pairing` | Join a room from a short invite code without exposing keys |
| Invites | blind-pairing-core | `blind-pairing-core` | Core invite/request/confirm protocol |
| Delivery | blind-peering | `blind-peering` | Client that asks blind peers to keep cores available offline |
| Delivery | blind-peer | `blind-peer` | Server-side blind peer (optional, self-hosted) |
| Protocol | compact-encoding | `compact-encoding` | Compact binary parsers/serialisers for messages |
| Protocol | protomux | `protomux` | Multiplexed channels over a single connection |
| Protocol | @hyperswarm/secret-stream | `@hyperswarm/secret-stream` | Noise-encrypted streams between peers |
| Pipe | hyperbeam | `hyperbeam` | 1-to-1 end-to-end encrypted pipe over Hyperswarm |
| Runtime | bare | `bare` | Zero-core JavaScript runtime for desktop and mobile |
| Runtime | pear-runtime | `pear-runtime` | Embeddable runtime with P2P over-the-air updates |
| Deploy | pear | `pear` (CLI) | Build, stage, seed, and update P2P apps |

## Architecture (planned)

```
UI (SwiftUI / Android Views / Compose)
  │
ViewModel
  │
Repository  │─►  local DB (SwiftData / Room / SQLDelight)
  │─►  Autobase room (messages, members, events)
  │─►  Hyperdrive (media cache)
  │─►  Hyperswarm (network)
```

- **Room** = one Autobase. Every member is a writer; Autobase linearises writes into a shared, eventually consistent view.
- **Identity** = seed phrase → Ed25519 keypairs via `keet-identity-key`. No phone number, no email.
- **Invites** = `blind-pairing` short codes. Joining never exposes long-term keys.
- **Offline delivery** = `blind-peering` asks blind peers to mirror encrypted cores; messages sync whenever any peer comes online.
- **Media** = Hyperdrive for files, streamed with previews and on-demand originals.

## Status

Early stage. Repository initialised with stack documentation; client code not started yet.

## License

Client code: TBD. Holepunch dependencies: Apache-2.0 / MIT — preserve `LICENSE` and `NOTICE` files in any distribution.

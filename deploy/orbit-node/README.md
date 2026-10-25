# Deploying orbit-node

`orbit-node` is the optional mailbox. It stores sealed envelopes for
recipients who are offline. It is not required for two people to meet.

The usual path is a direct session: an `orbit://invite/…` link carries a
signed contact card whose node field is an endpoint id, not an IP. Sockets
and relay fallback come from Iroh `presets::N0` (ALPN `orbit/direct/1`).
That path has no mailbox. A closed app does not accumulate incoming
envelopes. See [vertical slice](../../docs/vertical-slice.md).

Use this service when you want store-and-forward. Clients reach it at
`hex@ip:port` over ALPN `orbit/mailbox/1`, with relays disabled. One VPS is
enough for a small network. This note was not re-checked against the rooms
outbox join, and it is not a load test.

`orbit-node host` (and the in-app **Стать узлом** action, which runs that
command) is a same-LAN helper. It writes a concrete IPv4 of this computer,
not `0.0.0.0`, and keeps the node key and registration code. A `192.168.x.x`
address is not routed from a phone on mobile data. Devices on that Wi-Fi can
paste it. From the public Internet, UDP (default 7443) has to be forwarded
to the host. The Windows helper tries to allow the port in the firewall.
This note does not claim that a given machine already has the rule.

## Requirements for a public mailbox

- A public IP and an open **UDP** port (default 7443).
- A rough size for tens to hundreds of text users: 1 vCPU, 1–2 GB RAM, 20 GB
  disk. That is an estimate, not a benchmark.

## Setup

1. `orbit-node init > orbit-node.toml` writes an example.
2. Set `data_dir`, `public_addrs = ["<VPS IP>:7443"]`, and a long random
   `registration_code`. Without the code, anyone who can reach the node can
   create a mailbox.
3. Run it with systemd (`orbit-node.service`) or Docker (`Dockerfile`).
4. On start the process prints `node address: <id>@<ip:port>`. That string is
   what a client pastes. The node key is `data_dir/node.key`. Losing it
   changes the address, so back up `data_dir`.

There is still no username search. The display name is a label, not a lookup
key. One account is one device.

## Check with the console tool

```sh
orbit-cli keygen alice.key
orbit-cli register --node <address> --key alice.key --code <registration_code>
orbit-cli token-create --node <address> --key alice.key     # token for a sender
orbit-cli send --node <address> --to <mailbox alice> --token <token> --text "test"
orbit-cli fetch --node <address> --key alice.key --ack
```

`orbit-cli` sends **without end-to-end encryption**. Test data only.

## What the operator can see

Applications seal personal messages before send (HPKE). The console tool still
sends cleartext. Either way the node sees client IPs, request times and
sizes, mailbox ids, and message counts. It does not see direct-session
plaintext, because those envelopes are not deposited here.

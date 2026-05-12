# Mycelium Architecture (MVP-Oriented)

## Workspace Layout

- `crates/mycelium-core`: shared data model + traits (no transport runtime dependencies).
- `crates/mycelium-node`: libp2p-backed node engine (discovery, direct messaging, persistence).
- `crates/mycelium-cli`: operator-facing test CLI for local mesh experiments.
- `docs/research-notes.md`: design inputs from Briar, Serval, BATMAN, Freifunk, IPFS, DTN.

## Module Boundaries

- `connectivity/` (inside node behavior): interface transport adapters, currently TCP/LAN + mDNS discovery.
- `transport/` (core + node): direct request/response + gossip dissemination.
- `data/` (core): content-addressed envelope IDs, TTL, priority.
- `identity/` (core): peer identity model (ed25519 peer IDs from libp2p keypairs).
- `sync/` (core placeholder): Bloom-filter digest model for delta exchange.
- `energy/` (core): node energy state (`Active`, `Intermittent`, `Passive`) and scheduling hints.

## Core Traits and Data Types

- `MessageStore`: persistence boundary for local-first durability (currently sled implementation in node crate).
- `Envelope`: transport-neutral metadata (`id`, source/destination, payload, TTL, priority).
- `DirectMessage`: typed user payload bound to an envelope.

## MVP Execution Flow

1. Node boots with ed25519 identity and starts listening on a multiaddr.
2. mDNS discovers peers on LAN and updates explicit gossip peers.
3. CLI can send:
   - direct one-hop message: `/send <peer_id> <text>`
   - gossip broadcast: `<text>`
4. inbound/outbound messages are persisted locally (sled) before/after exchange.
5. direct requests return lightweight acknowledgements for delivery feedback.

## Tradeoffs (Current MVP)

- Chosen: `libp2p` for composable discovery + transport protocols.
  - Tradeoff: larger dependency surface than bespoke UDP/TCP stack.
- Chosen: sled for embeddable local durability.
  - Tradeoff: eventually may need SQLite for richer query/index migration paths.
- Chosen: mDNS discovery for LAN-first environments.
  - Tradeoff: not useful across routed/network-partition boundaries (future: BLE + rendezvous relays + DTN sync digests).

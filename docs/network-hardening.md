# Network Hardening Notes (Prompt 3)

This document summarizes protocol and API changes introduced in Prompt 3:

- scoped dissemination
- bloom-based anti-entropy
- envelope signing + verification
- TTL garbage collection
- hop-limit enforcement
- peer reputation

## 1) Wire Protocol Changes

`WireMessage` now includes:

- `SyncBloom { bloom: Vec<u8>, count: u64 }`
- `ScopeAnnounce { scopes: Vec<String> }`

Legacy variants are still supported:

- `SyncIds { ids: Vec<String> }`
- `SyncRequest { ids: Vec<String> }`
- `SyncData { messages: Vec<DirectMessage> }`

This allows mixed-version meshes during rollout.

## 2) Scoped Dissemination

`Scope` matching supports:

- exact: `berlin/mitte`
- wildcard prefix: `berlin/*`

Behavior:

- nodes advertise interests via `ScopeAnnounce`
- incoming scoped messages are processed only if local subscription matches
- scoped broadcast prefers peers whose advertised scopes match

## 3) Envelope Compatibility

`Envelope` now has additional fields:

- `signature: Option<Vec<u8>>`
- `hop_count: u8` (`#[serde(default)]`)
- `max_hops: u8` (`#[serde(default = "default_max_hops")]`)

Compatibility details:

- old stored messages deserialize cleanly due to `serde(default)`
- signature verification is enforced only when a signature exists
- unsigned legacy messages remain accepted for backward compatibility

## 4) Anti-Entropy (Bloom)

Bloom exchange flow:

1. sender sends fixed-size bloom (`SyncBloom`)
2. receiver computes differential sets
3. receiver requests missing IDs (`SyncRequest`) and/or sends extra IDs (`SyncIds`)

Effect:

- first sync payload is bounded (~1 KB bloom) rather than linear in ID count

## 5) Store Integrity and GC

Message store now supports:

- `gc_expired()` for TTL-based cleanup
- `stats()` returning `{ count, oldest_ms }`

Runner behavior:

- periodic GC tick every 6 hours
- manual triggers via commands (`GcNow`, `StoreStats`)

## 6) Hop-Limit

Forwarding now enforces:

- drop when `hop_count >= max_hops`
- increment `hop_count` on forward

Metric added:

- `messages_dropped_hops`

## 7) Peer Reputation

Simple strike model:

- strikes for invalid/duplicate/non-productive traffic
- temporary throttle after threshold
- strike decay and partial recovery on valid traffic

API endpoint:

- `GET /api/v1/peers/{peer_id}/reputation`

## 8) REST API Additions

Added endpoints:

- `GET  /api/v1/store/stats`
- `POST /api/v1/store/gc`
- `GET  /api/v1/peers/{peer_id}/reputation`
- `POST /api/v1/peers/add`
- `GET  /api/v1/scopes`
- `POST /api/v1/scopes/subscribe`
- `DELETE /api/v1/scopes/{scope}`

## 9) Android / Client Migration Notes

- No UDL breaking changes were introduced in Prompt 3.
- Existing Android calls remain valid.
- Recommended client updates:
  - call `/api/v1/scopes` for current subscriptions
  - use `/api/v1/peers/add` for QR/bootstrap pairing flows
  - surface reputation and store stats in diagnostics screens

## 10) Verification Commands

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

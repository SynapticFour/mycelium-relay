# Mesh/DTN Pattern Extraction

## Briar
- Works well: local transports first (Bluetooth, Wi-Fi, Tor), per-contact replication, metadata minimization.
- Breaks at scale: pairwise sync fan-out grows quickly, group history convergence can be slow with many intermittently connected peers.
- Improvement: combine Briar's trust model with probabilistic anti-entropy (Bloom-filter summaries) to avoid full pairwise history checks.

## Serval + Rhizome
- Works well: store-carry-forward bundles, priority-aware opportunistic transfer, resilient disconnected operation.
- Breaks at scale: large bundle catalogs increase sync overhead and storage churn; routing quality drops with high mobility.
- Improvement: bounded bundle windows per neighborhood + adaptive TTL by energy state and observed delivery success.

## BATMAN
- Works well: decentralized link-quality scoring, local decision making without global topology maps.
- Breaks at scale: control traffic overhead rises in dense meshes, quality metrics can lag behind rapid link shifts.
- Improvement: hybrid mode: BATMAN-like live path hints for low-latency traffic + DTN fallback for unstable links.

## Freifunk Deployments
- Works well: community-owned infrastructure, commodity hardware viability, practical multi-hop operation.
- Breaks at scale: heterogenous node quality and radio contention create uneven reliability; operator complexity increases.
- Improvement: explicit energy/connectivity classes and policy-driven forwarding to prevent weak nodes from overload.

## IPFS
- Works well: content addressing, deduplication, immutable object references, resilient retrieval paths.
- Breaks at scale: DHT-heavy global discovery can be expensive in constrained/offline environments.
- Improvement: local-first content index with neighborhood gossip; only escalate to wider search opportunistically.

## DTN / Bundle Protocol
- Works well: custody transfer semantics, delayed delivery tolerance, path-independence.
- Breaks at scale: custody/accounting metadata and retransmit state can become heavy for small devices.
- Improvement: compact custody-lite acknowledgements and strict expiration/priority queues tuned for embedded storage budgets.

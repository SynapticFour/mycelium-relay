# SynapticFour Mycelium family

**You are here:** [mycelium-relay](https://github.com/SynapticFour/mycelium-relay) — public libp2p bootstrap relay (Fly.io).

See [Mycelium docs/ECOSYSTEM.md](https://github.com/SynapticFour/Mycelium/blob/main/docs/ECOSYSTEM.md) for the full family map.

## Local run

```bash
cargo run -p mycelium-relay
curl -s http://localhost:8080/health
```

## Production deploy

```bash
./scripts/deploy-relay-fly.sh
```

No `make up`/`down` — relay is a single binary service; local = `cargo run`, production = Fly.io.

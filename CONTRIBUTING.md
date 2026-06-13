# Contributing to Mycelium Relay

Early public beta — coordinate with the main [Mycelium](https://github.com/SynapticFour/Mycelium) repository when changes affect shared crates.

## Reporting Issues

| Type | Channel |
|------|---------|
| **Bug** | GitHub Issue with repro steps |
| **Feature** | GitHub Issue with use case |
| **Security** | **security@synapticfour.com** only — see [SECURITY.md](SECURITY.md) |

## Development

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p mycelium-relay
```

## Sync from Mycelium

Most crates are mirrored from the main repo:

```bash
# From Mycelium repo root:
./scripts/sync-relay-repo.sh /path/to/mycelium-relay
```

Prefer implementing shared logic in **Mycelium** first, then sync here.

## Commit Conventions

```
feat(relay): add rendezvous rate limit
fix(deploy): correct fly.toml health check path
docs: clarify AGPL obligations for relay operators
```

## Pull Requests

- Keep relay-specific changes focused (deploy, relay binary, ops docs)
- Ensure CI passes
- Note if a matching Mycelium PR is required
- Contributions are licensed under AGPL-3.0-or-later

## Deploy changes

Document Fly.io impact in the PR. Do not commit secrets — use Fly secrets / GitHub Actions secrets.

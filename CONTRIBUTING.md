# Contributing

## Rust workspace

- Format: `cargo fmt --all`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Tests: `cargo test --workspace`

## Platform-specific work

- **Android**: see `android/README.md`
- **Desktop (Tauri)**: see `crates/mycelium-desktop/` and release workflow under `.github/workflows/`
- **Relay deployment**: see `deploy/relay/README.md` and `.github/workflows/deploy.yml`

Pull requests should stay focused, include a clear description of intent, and keep CI green.

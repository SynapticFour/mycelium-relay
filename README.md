# Mycelium

Decentralized, censorship-resistant, delay-tolerant mesh networking prototype in Rust.

## Run

```bash
cargo run -p mycelium-cli
```

Start two terminals on the same LAN and exchange peer IDs:
- list known peers: `/peers`
- direct message: `/chat <peer_id> hello`
- scoped broadcast: `/broadcast mycelium/chat hello`

## Smoke Test

For a full MVP validation flow (chat, bulletin, mail, REST API), see:

- `docs/mvp-smoke-test.md`

## Android

For Android build, UniFFI binding generation, emulator/device install, and E2E checks, see:

- `android/README.md`

## Network Hardening

For scoped dissemination, bloom anti-entropy, signing, GC, hop limits, and reputation details, see:

- `docs/network-hardening.md`

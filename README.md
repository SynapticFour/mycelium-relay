# Mycelium

Research-oriented mesh networking workspace in Rust (libp2p, delay-tolerant messaging, relay, desktop, and Android targets).

[![Mycelium CI](https://github.com/SynapticFour/mycelium-relay/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/SynapticFour/mycelium-relay/actions/workflows/ci.yml)

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

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Security

See [SECURITY.md](SECURITY.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

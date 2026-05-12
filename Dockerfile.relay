# ── Stage 1: Build ──────────────────────────────────────────────────────────
FROM rust:1.82-slim-bookworm AS builder

# System-Dependencies für libp2p/OpenSSL
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Abhängigkeiten zuerst cachen (Schicht wird wiederverwendet wenn Cargo.* unverändert)
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/

# Nur das Relay-Binary bauen (nicht den ganzen Workspace)
RUN cargo build --release -p mycelium-relay

# ── Stage 2: Runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Non-root user für Sicherheit
RUN useradd -r -u 1001 -s /bin/false mycelium

# Persistentes Datenverzeichnis für Keypair (wird in Fly.io als Volume gemountet)
RUN mkdir -p /data && chown mycelium:mycelium /data

COPY --from=builder /app/target/release/mycelium-relay /usr/local/bin/mycelium-relay
RUN chmod +x /usr/local/bin/mycelium-relay

USER mycelium

# Ports: 4001 TCP (libp2p), 4001 UDP (QUIC), 8080 TCP (HTTP Status-Dashboard)
EXPOSE 4001/tcp
EXPOSE 4001/udp
EXPOSE 8080/tcp

ENV RUST_LOG=info,mycelium_relay=debug

CMD ["/usr/local/bin/mycelium-relay", \
     "--listen", "/ip4/0.0.0.0/tcp/4001", \
     "--listen-quic", "/ip4/0.0.0.0/udp/4001/quic-v1", \
     "--keypair-path", "/data/identity", \
     "--status-port", "8080"]

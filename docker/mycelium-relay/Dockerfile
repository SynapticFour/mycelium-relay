# Build: docker build -f docker/mycelium-relay/Dockerfile -t mycelium-relay:local .
# Run:  docker run --rm -p 4001:4001/tcp -p 4001:4001/udp mycelium-relay:local --listen /ip4/0.0.0.0/tcp/4001
FROM rust:1.85-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p mycelium-relay

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/target/release/mycelium-relay /usr/local/bin/mycelium-relay
ENTRYPOINT ["/usr/local/bin/mycelium-relay"]

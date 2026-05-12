#!/usr/bin/env bash
# Deploy the Mycelium libp2p relay to Fly.io (remote Docker build).
# Prereq: `fly auth login`, repo secret FLY_API_TOKEN in CI, optional local FLY_API_TOKEN.
#
# Usage (from anywhere):
#   bash scripts/deploy-relay-fly.sh
#   bash scripts/deploy-relay-fly.sh --strategy immediate
#
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v flyctl >/dev/null 2>&1 && ! command -v fly >/dev/null 2>&1; then
  echo "flyctl not found. Install: https://fly.io/docs/hands-on/install-flyctl/" >&2
  exit 1
fi

FLY_BIN="$(command -v flyctl 2>/dev/null || command -v fly)"

# Repo root = Docker build context; image is Dockerfile.relay (matches CI).
exec "$FLY_BIN" deploy --remote-only \
  --config deploy/relay/fly.toml \
  --dockerfile Dockerfile.relay \
  "$@"

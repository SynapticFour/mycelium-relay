#!/usr/bin/env bash
# Deploy the Mycelium libp2p relay to Fly.io (remote Docker build).
#
# Prereq: fly auth login (local) or FLY_API_TOKEN (CI). Optional: FLY_ORG for
# `fly apps create` when you have multiple orgs (e.g. FLY_ORG=personal).
#
# Usage (from repo root or anywhere):
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
FLY_TOML="$ROOT/deploy/relay/fly.toml"

if [[ ! -f "$FLY_TOML" ]]; then
  echo "Missing $FLY_TOML" >&2
  exit 1
fi

# Must match deploy/relay/fly.toml (single source for app + region + volume name).
FLY_APP="$(grep -m1 '^app = ' "$FLY_TOML" | sed 's/^app = "\(.*\)".*/\1/')"
FLY_REGION="$(grep -m1 '^primary_region' "$FLY_TOML" | sed 's/^primary_region = "\(.*\)".*/\1/')"
FLY_VOL="$(awk '/^\[mounts\]/{f=1;next} f && /^source = / { print; exit }' "$FLY_TOML" | sed 's/^source = "\(.*\)"/\1/')"
if [[ -z "$FLY_APP" || -z "$FLY_REGION" || -z "$FLY_VOL" ]]; then
  echo "Could not parse app, primary_region, or [mounts] source from $FLY_TOML" >&2
  exit 1
fi

app_exists() {
  local out
  out="$("$FLY_BIN" apps list --json 2>/dev/null || true)"
  # flyctl pretty-prints JSON with spaces after ":" (e.g. "Name": "app").
  echo "$out" | grep -qE "\"Name\"[[:space:]]*:[[:space:]]*\"$FLY_APP\""
}

volume_exists() {
  local out
  out="$("$FLY_BIN" volumes list -a "$FLY_APP" --json 2>/dev/null || true)"
  echo "$out" | grep -qE "\"name\"[[:space:]]*:[[:space:]]*\"$FLY_VOL\""
}

if ! app_exists; then
  echo "fly: creating app \"$FLY_APP\" (region will follow deploy)..."
  create_args=(apps create "$FLY_APP" -y)
  if [[ -n "${FLY_ORG:-}" ]]; then
    create_args+=(-o "$FLY_ORG")
  fi
  "$FLY_BIN" "${create_args[@]}"
fi

if ! volume_exists; then
  echo "fly: creating volume \"$FLY_VOL\" in $FLY_REGION for app \"$FLY_APP\"..."
  "$FLY_BIN" volumes create "$FLY_VOL" -a "$FLY_APP" -r "$FLY_REGION" -s 1 -y
fi

# Repo root = Docker build context; image is Dockerfile.relay (matches CI).
exec "$FLY_BIN" deploy --remote-only \
  --config deploy/relay/fly.toml \
  --dockerfile Dockerfile.relay \
  "$@"

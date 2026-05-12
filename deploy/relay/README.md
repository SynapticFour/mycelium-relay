# Mycelium Relay — Fly.io deployment

Single image definition: **`Dockerfile.relay`** at the **repository root** (workspace build context).  
Fly app config: **`deploy/relay/fly.toml`** (no `[build]` block — deploy script and CI pass `--dockerfile Dockerfile.relay`).

## One-time setup

### 1. Fly.io CLI

```bash
curl -L https://fly.io/install.sh | sh
fly auth login
```

Create the app and volume once (names must match `fly.toml` `app` and `[mounts] source`):

```bash
fly apps create myc-relay-synfour --org personal   # or your org; match fly.toml `app`
fly volumes create mycelium_relay_data \
  --app myc-relay-synfour \
  --region fra \
  --size 1
```

### 2. GitHub Actions

Create a deploy token and add **`FLY_API_TOKEN`** under **Settings → Secrets and variables → Actions**.

The workflow **`.github/workflows/deploy.yml`** runs `scripts/deploy-relay-fly.sh` on pushes that touch relay code, `Dockerfile.relay`, or this folder.

### 3. Manual deploy (same as CI)

From the **repository root**:

```bash
bash scripts/deploy-relay-fly.sh
```

With extra `flyctl deploy` flags:

```bash
bash scripts/deploy-relay-fly.sh --strategy immediate
```

### 4. Local Docker build (smoke)

```bash
docker build -f Dockerfile.relay -t mycelium-relay:local .
docker run --rm -p 4001:4001/tcp -p 4001:4001/udp -p 8080:8080 mycelium-relay:local
curl -s http://localhost:8080/health
```

### 5. Logs and peer id

```bash
fly logs --app myc-relay-synfour | grep -i peer
```

## Notes

- **GitHub** only runs workflows under **`.github/workflows/`** at the repo root; there is no duplicate workflow under `deploy/relay/.github`.
- Fly free tier: shared CPU / 256 MB is enough for a small relay; scale when needed.

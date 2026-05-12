# Mycelium Relay Server – Deployment

## Erstmalig einrichten (einmalig manuell)

### 1. Fly.io Account & CLI

```bash
# Fly CLI installieren
curl -L https://fly.io/install.sh | sh

# Einloggen
fly auth login

# App erstellen (einmalig)
fly apps create mycelium-relay --org personal

# Volume für persistenten Keypair anlegen (einmalig, 1 GB reicht)
fly volumes create mycelium_relay_data \
  --app mycelium-relay \
  --region fra \
  --size 1
```

### 2. GitHub Secret setzen

```bash
# Fly.io API Token erstellen
fly tokens create deploy -x 999999h

# → Diesen Token als GitHub Secret "FLY_API_TOKEN" in deinem Repo speichern
# GitHub Repo → Settings → Secrets and variables → Actions → New secret
```

### 3. Erst-Deployment

```bash
fly deploy --config deploy/relay/fly.toml --dockerfile Dockerfile.relay
```

### 4. Peer-ID auslesen (WICHTIG – sofort nach erstem Start)

```bash
fly logs --app mycelium-relay | grep "Peer ID"
# → Relay Peer ID: 12D3KooWXxxxx...

# Status-Endpunkt prüfen:
curl https://mycelium-relay.fly.dev/
# → {"status":"ok","peer_id":"12D3KooW...","connections":0,...}
```

Diese Peer-ID kommt in die App-Konfiguration (siehe Cursor Prompt 7b).

### 5. Öffentliche Multiaddr

Nach dem Deployment ist dein Relay unter folgender Adresse erreichbar:

```
/dns4/mycelium-relay.fly.dev/tcp/4001/p2p/12D3KooW<DEINE-PEER-ID>
```

## GitHub Actions

GitHub führt Workflows nur aus **`.github/workflows/` im Repository-Root** aus. Dort liegt die aktive Datei `.github/workflows/deploy.yml`. Eine Kopie mit gleichem Inhalt liegt unter `deploy/relay/.github/workflows/deploy.yml` (Projektstruktur); bei Änderungen beide Dateien abgleichen oder nur die Root-Datei pflegen und die Kopie aktualisieren.

## Kosten

Fly.io Free Tier deckt 3 shared-cpu-1x VMs mit 256 MB RAM ab.
Der Relay-Server nutzt eine davon. Kosten: $0/Monat bis du wächst.

## Mehrere Relay-Server (für Redundanz)

```bash
# Zweite Region (z.B. Amsterdam)
fly regions add ams --app mycelium-relay
fly scale count 2 --app mycelium-relay
```

Fly.io verteilt die Instanzen automatisch.

## Tests vor dem Deployment

```bash
# Lokal testen:
docker build -f Dockerfile.relay -t mycelium-relay-test .
docker run -p 4001:4001 -p 8080:8080 mycelium-relay-test

# Status prüfen:
curl http://localhost:8080/
# → {"status":"ok","peer_id":"12D3KooW...","connections":0,...}

# Relay-Node compiliert noch:
cargo build --release -p mycelium-relay
```

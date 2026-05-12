# Mycelium MVP Smoke Test

This document gives a copy/paste validation flow for:
- two local nodes
- chat
- bulletin board
- mesh mail
- REST API checks

## Prerequisites

- Build and tests are green:

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

- Optional (for prettier JSON):

```bash
brew install jq
```

## 1) Start Two Nodes

Open two terminals.

### Terminal A

```bash
cargo run -p mycelium-cli -- --db /tmp/node-a --name Alice --api-port 7760
```

### Terminal B

```bash
cargo run -p mycelium-cli -- --db /tmp/node-b --name Bob --api-port 7761
```

Both terminals print a line like:

```text
local peer id: <PEER_ID>
```

Copy both peer IDs.

## 2) Verify Peer Discovery

In both terminals:

```text
/peers
```

You should see the other peer eventually.

## 3) Mesh-Mail Test

Send a mail from A to B:

```text
/mail <PEER_ID_OF_B> Hallo | Das ist eine Test-Mail ueber Mycelium.
```

Check B inbox via REST:

```bash
curl -s http://127.0.0.1:7761/api/v1/mail/inbox | jq
```

Expected: at least one mail entry with `subject = "Hallo"`.

Mark the message as read (replace `<MAIL_ID>`):

```bash
curl -s -X PUT http://127.0.0.1:7761/api/v1/mail/<MAIL_ID>/read | jq
```

## 4) Chat Test

From A to B:

```text
/chat <PEER_ID_OF_B> Hi Bob!
```

In B terminal:

```text
/chat:history <PEER_ID_OF_A>
```

REST check:

```bash
curl -s http://127.0.0.1:7761/api/v1/chat/<PEER_ID_OF_A> | jq
```

Expected: latest chat contains `"Hi Bob!"`.

## 5) Bulletin Test

In A terminal:

```text
/bulletin berlin/mitte Markt | Frisches Brot am Platz
```

In B terminal:

```text
/bulletin:list berlin/mitte
```

REST check:

```bash
curl -s http://127.0.0.1:7761/api/v1/bulletin/berlin%2Fmitte | jq
```

Expected: one bulletin with title `Markt`.

## 6) API Status and Settings

Check status:

```bash
curl -s http://127.0.0.1:7760/api/v1/status | jq
curl -s http://127.0.0.1:7761/api/v1/status | jq
```

Check settings:

```bash
curl -s http://127.0.0.1:7760/api/v1/settings | jq
```

Update display name:

```bash
curl -s -X PUT http://127.0.0.1:7760/api/v1/settings/name \
  -H 'content-type: application/json' \
  -d '{"display_name":"AliceMesh"}' | jq
```

Update energy state:

```bash
curl -s -X PUT http://127.0.0.1:7760/api/v1/settings/energy \
  -H 'content-type: application/json' \
  -d '{"state":"intermittent"}' | jq
```

## 7) Expected MVP Outcome

- Node A and Node B discover each other.
- A can send chat/mail to B.
- Bulletin posts are visible from B.
- REST endpoints return valid JSON.
- Status endpoint returns peer ID, display name, and node metrics.

## 8) Troubleshooting

- If no peers appear:
  - wait 10-30s for mDNS discovery
  - ensure both terminals run on same host/LAN
- If REST fails:
  - verify API port in startup command
  - check `http://127.0.0.1:7760/api/v1/status`
- If message not delivered:
  - verify peer ID copied correctly
  - run `/peers` before `/chat` or `/mail`

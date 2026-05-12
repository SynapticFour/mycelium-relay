#!/usr/bin/env bash
set -euo pipefail

echo "=== Mycelium Node - Raspberry Pi Setup ==="

sudo useradd -r -s /bin/false mycelium 2>/dev/null || true
sudo mkdir -p /var/lib/mycelium
sudo chown mycelium:mycelium /var/lib/mycelium

if [ ! -f /usr/local/bin/mycelium ]; then
  echo "ERROR: /usr/local/bin/mycelium not found. Copy the binary first:"
  echo "  scp target/aarch64-unknown-linux-gnu/release/mycelium pi@<IP>:/usr/local/bin/"
  exit 1
fi

sudo chmod +x /usr/local/bin/mycelium
sudo mkdir -p /etc/mycelium
cat << "EOF" | sudo tee /etc/mycelium/node.env >/dev/null
RUST_LOG=info
MYCELIUM_LISTEN=/ip4/0.0.0.0/tcp/7761
MYCELIUM_DB=/var/lib/mycelium
MYCELIUM_NAME=pi-node
MYCELIUM_API_PORT=7760
EOF

cat << "EOF" | sudo tee /etc/systemd/system/mycelium.service >/dev/null
[Unit]
Description=Mycelium Mesh Node
After=network.target
Wants=network-online.target

[Service]
Type=simple
User=mycelium
EnvironmentFile=/etc/mycelium/node.env
ExecStart=/usr/local/bin/mycelium \
  --listen ${MYCELIUM_LISTEN} \
  --db ${MYCELIUM_DB} \
  --name ${MYCELIUM_NAME} \
  --api-port ${MYCELIUM_API_PORT}
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable mycelium
sudo systemctl start mycelium

echo "=== Setup complete ==="
echo "Status: sudo systemctl status mycelium"
echo "Logs:   sudo journalctl -u mycelium -f"
echo "API:    http://localhost:7760/api/v1/status"

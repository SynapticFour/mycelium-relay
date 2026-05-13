//! Periodic reachability probe for hybrid Internet / mesh operation.

use mycelium_core::transport::ConnectivityMode;
use tokio::sync::watch;
use tracing::info;

const PROBE_HOST: &str = "mycelium-relay.fly.dev";
const PROBE_PORT: u16 = 4001;

pub struct ConnectivityMonitor {
    pub mode_tx: watch::Sender<ConnectivityMode>,
    pub mode_rx: watch::Receiver<ConnectivityMode>,
}

impl ConnectivityMonitor {
    pub fn new() -> Self {
        let (mode_tx, mode_rx) = watch::channel(ConnectivityMode::Internet);
        Self { mode_tx, mode_rx }
    }

    /// Background task: probe TCP reachability every 15s and update [`ConnectivityMode`].
    pub fn spawn_monitor(tx: watch::Sender<ConnectivityMode>) {
        tokio::spawn(async move {
            let mut current = *tx.borrow();
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                let reachable = check_internet_connectivity().await;
                let new_mode = if reachable {
                    ConnectivityMode::Internet
                } else {
                    ConnectivityMode::MeshOnly
                };
                if new_mode != current {
                    info!("connectivity changed: {:?} → {:?}", current, new_mode);
                    current = new_mode;
                    let _ = tx.send(new_mode);
                }
            }
        });
    }
}

impl Default for ConnectivityMonitor {
    fn default() -> Self {
        Self::new()
    }
}

async fn check_internet_connectivity() -> bool {
    let addr = format!("{PROBE_HOST}:{PROBE_PORT}");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::net::TcpStream::connect(addr),
    )
    .await;
    matches!(result, Ok(Ok(_)))
}

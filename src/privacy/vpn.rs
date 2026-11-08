//! In-process control surface for NanomiteVPN.
//!
//! [`start`] owns one [`VpnEngine`] at a time: it binds the UDP socket,
//! attempts to create the TUN device, adds any peers from the config,
//! initiates the handshakes and keeps the run loop alive on a spawned task.
//! [`stop`] aborts that task. The manager never touches the UI thread.

use crate::nanomite_vpn::{
    config::Config,
    engine::{PeerConfig as EnginePeerConfig, TunnelEvent, VpnEngine},
};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

/// Snapshot of the VPN module's current state.
#[derive(Debug, Clone)]
pub struct Status {
    pub running: bool,
    pub state: String,
    pub fingerprint: Option<String>,
    pub listen_port: Option<u16>,
    pub tun: String,
    pub peers: Vec<PeerStatus>,
    pub note: Option<String>,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            running: false,
            state: "stopped".to_string(),
            fingerprint: None,
            listen_port: None,
            tun: String::new(),
            peers: Vec::new(),
            note: None,
        }
    }
}

/// One configured peer and its tunnel state.
#[derive(Debug, Clone, Default)]
pub struct PeerStatus {
    pub public_key: String,
    pub endpoint: String,
    pub state: String,
}

struct Runtime {
    engine: Arc<VpnEngine>,
    task: tokio::task::JoinHandle<()>,
    listen_port: u16,
}

static RUNTIME: OnceLock<Mutex<Option<Runtime>>> = OnceLock::new();

fn state_guard() -> std::sync::MutexGuard<'static, Option<Runtime>> {
    RUNTIME
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("vpn lock poisoned")
}

/// Start (or report an already-running) VPN engine.
///
/// `config_path` overrides the config file; otherwise `~/.config/nanomite/vpn.toml`
/// is used when present, otherwise defaults. A missing/privileged TUN device is
/// reported as a note, not a failure: handshakes and packets still work,
/// kernel routing just isn't installed.
pub async fn start(config_path: Option<&Path>) -> Result<String, String> {
    if state_guard().is_some() {
        return Ok("vpn is already running".to_string());
    }

    let config = match config_path {
        Some(path) => {
            Config::from_file(path).map_err(|e| format!("failed to load vpn config: {e}"))?
        }
        None => Config::default_config_path()
            .ok()
            .and_then(|path| path.is_file().then_some(path))
            .map(|path| Config::from_file(&path))
            .transpose()
            .map_err(|e| format!("failed to load default vpn config: {e}"))?
            .unwrap_or_default(),
    };

    let listen_port = config.listen_port;
    let vpn_config = crate::nanomite_vpn::engine::VpnConfig {
        listen_port,
        tun_config: crate::nanomite_vpn::tun::TunConfig::default(),
        ..Default::default()
    };

    let mut engine = VpnEngine::new(vpn_config)
        .await
        .map_err(|e| format!("vpn engine creation failed: {e}"))?;

    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<TunnelEvent>();
    engine.set_event_channel(event_tx);
    let _drain = tokio::spawn(async move { while event_rx.recv().await.is_some() {} });

    let bind_addr = SocketAddr::from(([0, 0, 0, 0], listen_port));
    engine
        .bind(bind_addr)
        .await
        .map_err(|e| format!("vpn bind failed: {e}"))?;

    let tun_note = match engine.create_tun().await {
        Ok(()) => {
            let tun = &config.tun;
            format!("tun {} ({}/{})", tun.name, tun.address, tun.prefix_len)
        }
        Err(e) => {
            format!("tun unavailable ({e}); handshakes and packets still work")
        }
    };

    for peer in &config.peers {
        let public_key = Config::parse_public_key(&peer.public_key)
            .map_err(|e| format!("invalid peer public key {}: {e}", peer.public_key))?;
        let endpoint: SocketAddr = peer
            .endpoint
            .parse()
            .map_err(|e| format!("invalid peer endpoint {}: {e}", peer.endpoint))?;
        let peer_config = EnginePeerConfig {
            public_key,
            endpoint,
            allowed_ips: peer.allowed_ips.clone(),
            persistent_keepalive: peer.persistent_keepalive,
            preshared_key: None,
        };
        engine
            .add_peer(peer_config)
            .await
            .map_err(|e| format!("failed to add peer: {e}"))?;
        engine
            .connect(&public_key)
            .await
            .map_err(|e| format!("failed to connect to peer: {e}"))?;
    }

    let engine = Arc::new(engine);
    let engine_for_task = engine.clone();
    let task = tokio::spawn(async move {
        let _ = engine_for_task.run().await;
    });

    state_guard().replace(Runtime {
        engine,
        task,
        listen_port,
    });

    let status = status().await;
    Ok(format!(
        "vpn started: udp/0.0.0.0:{}, fp {}, peers: {}, {}",
        status.listen_port.unwrap_or(listen_port),
        status.fingerprint.as_deref().unwrap_or("-"),
        status.peers.len(),
        tun_note
    ))
}

/// Stop the VPN engine and abort its run loop.
pub async fn stop() -> Result<String, String> {
    let runtime = state_guard().take();
    let Some(runtime) = runtime else {
        return Ok("vpn is not running".to_string());
    };
    for (pk, _) in runtime.engine.peers().await {
        let _ = runtime.engine.disconnect(&pk).await;
    }
    runtime.task.abort();
    Ok("vpn stopped".to_string())
}

/// Current status snapshot.
pub async fn status() -> Status {
    let guard = state_guard();
    let Some(runtime) = guard.as_ref() else {
        return Status::default();
    };
    let engine = &runtime.engine;
    let mut status = Status {
        running: true,
        state: format!("{:?}", engine.state().await),
        listen_port: Some(runtime.listen_port),
        ..Default::default()
    };
    status.fingerprint = Some(engine.fingerprint());
    for (pk, state) in engine.peers().await {
        status.peers.push(PeerStatus {
            public_key: hex::encode(pk),
            endpoint: String::new(),
            state: format!("{state:?}"),
        });
    }
    status
}

/// Convenience: block the calling thread on [`start`] with a runtime handle.
pub fn start_blocking(config_path: Option<&Path>) -> Result<String, String> {
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|e| format!("no tokio runtime available: {e}"))?;
    handle.block_on(start(config_path))
}

/// Convenience: block the calling thread on [`stop`] with a runtime handle.
pub fn stop_blocking() -> Result<String, String> {
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|e| format!("no tokio runtime available: {e}"))?;
    handle.block_on(stop())
}

/// Blocking render of [`status`] for the tool dispatcher.
pub fn status_blocking() -> Result<String, String> {
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|e| format!("no tokio runtime available: {e}"))?;
    let status = handle.block_on(status());
    Ok(format!(
        "vpn: running={} state={} listen={:?} peers={} {}",
        status.running,
        status.state,
        status.listen_port,
        status.peers.len(),
        status.note.as_deref().unwrap_or("")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_defaults_to_stopped() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let status = rt.block_on(status());
        assert!(!status.running);
        assert_eq!(status.state, "stopped");
        assert!(status.peers.is_empty());
    }

    #[test]
    fn runtime_handle_is_available_in_spawn_blocking() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ok = rt.block_on(async {
            tokio::task::spawn_blocking(|| tokio::runtime::Handle::try_current().is_ok())
                .await
                .unwrap()
        });
        assert!(
            ok,
            "tool calls run in spawn_blocking and need the runtime handle"
        );
    }

    #[test]
    fn stop_when_not_running_is_a_noop() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(stop());
        assert!(result.is_ok());
        assert!(result.unwrap().contains("not running"));
    }

    #[test]
    fn start_blocking_reports_no_runtime_outside_a_runtime() {
        let result = start_blocking(None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no tokio runtime available"));
    }
}

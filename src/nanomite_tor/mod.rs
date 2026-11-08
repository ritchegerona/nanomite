//! NanomiteTOR — a library-only Tor integration.
//!
//! The module is a typed, testable client for a local Tor daemon:
//!
//! - [`control`] speaks `control-spec.txt` over a pluggable transport.
//! - [`socks`] is an RFC 1928/1929 SOCKS5 client with stream isolation.
//! - [`process`] supervises a managed `tor` binary (or attaches to one).
//! - [`bootstrap`], [`circuit`], [`onion`], [`leak`] and [`config`] cover
//!   progress, introspection, v3 onion services, leak policy and torrc
//!   generation.
//!
//! Everything above the transport layer is pure and unit-tested offline. Tor is
//! never bundled: a system binary is required at runtime. The module performs no
//! privileged action and exposes no agent-callable apply path.

pub mod bootstrap;
pub mod circuit;
pub mod config;
pub mod control;
pub mod events;
pub mod leak;
pub mod onion;
pub mod process;
pub mod socks;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

pub use config::{AuthMode, BridgeLine, OnionServiceConfig, TorConfig};
pub use control::{
    ControlAuth, ControlClient, ControlEvent, ControlReply, ControlTransport, ProtocolInfo,
    TcpControlTransport, TorSignal,
};
pub use events::{TorEvent, TorEventEnvelope};
pub use leak::{RouteDecision, TorLeakGuard};
pub use onion::{OnionFlag, OnionKey, OnionKeyInput, OnionPort, OnionRequest, OnionResponse};
pub use process::{ProcessSpawner, SystemSpawner, TorChild, TorProcess};
pub use socks::{Socks5Client, SocksAddr, SocksTarget};

/// Top-level result type.
pub type TorResult<T> = Result<T, TorError>;

/// Errors produced by the Tor integration.
#[derive(Debug)]
pub enum TorError {
    Config(String),
    Protocol(String),
    Control(String),
    Bootstrap(String),
    Socks(String),
    Onion(String),
    Process(String),
    Leak(String),
    Network(String),
    Io(std::io::Error),
    Serde(serde_json::Error),
    NotRunning,
    Timeout(String),
    RateLimited(String),
}

impl std::fmt::Display for TorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TorError::Config(s) => write!(f, "config error: {s}"),
            TorError::Protocol(s) => write!(f, "protocol error: {s}"),
            TorError::Control(s) => write!(f, "control error: {s}"),
            TorError::Bootstrap(s) => write!(f, "bootstrap error: {s}"),
            TorError::Socks(s) => write!(f, "socks error: {s}"),
            TorError::Onion(s) => write!(f, "onion service error: {s}"),
            TorError::Process(s) => write!(f, "process error: {s}"),
            TorError::Leak(s) => write!(f, "leak protection error: {s}"),
            TorError::Network(s) => write!(f, "network error: {s}"),
            TorError::Io(e) => write!(f, "io error: {e}"),
            TorError::Serde(e) => write!(f, "serde error: {e}"),
            TorError::NotRunning => write!(f, "tor is not running"),
            TorError::Timeout(s) => write!(f, "timeout: {s}"),
            TorError::RateLimited(s) => write!(f, "rate limited: {s}"),
        }
    }
}

impl std::error::Error for TorError {}

impl From<std::io::Error> for TorError {
    fn from(e: std::io::Error) -> Self {
        TorError::Io(e)
    }
}

impl From<serde_json::Error> for TorError {
    fn from(e: serde_json::Error) -> Self {
        TorError::Serde(e)
    }
}

/// High-level facade tying the pieces together.
///
/// `TorController` owns an optional managed process. Callers that already have
/// a daemon can skip [`TorController::launch`] and use [`connect_control`]
/// directly.
///
/// [`connect_control`]: TorController::connect_control
pub struct TorController {
    config: TorConfig,
    process: TorProcess,
    newnym: circuit::NewnymLimiter,
}

impl TorController {
    /// Validate `config` and build a controller (does not start anything).
    pub fn new(config: TorConfig) -> TorResult<Self> {
        config.validate()?;
        Ok(Self {
            process: TorProcess::new(config.clone()),
            config,
            newnym: circuit::NewnymLimiter::new(),
        })
    }

    pub fn config(&self) -> &TorConfig {
        &self.config
    }

    /// The address other applications should use as their SOCKS5 proxy.
    pub fn socks_addr(&self) -> TorResult<SocketAddr> {
        format!("127.0.0.1:{}", self.config.socks_port)
            .parse()
            .map_err(|e| TorError::Config(format!("invalid socks address: {e}")))
    }

    pub fn is_running(&self) -> bool {
        self.process.is_running()
    }

    pub fn status(&self) -> &bootstrap::BootstrapStatus {
        self.process.status()
    }

    /// Launch a managed `tor` and wait for bootstrap.
    pub async fn launch(&mut self, timeout: Duration) -> TorResult<bootstrap::BootstrapStatus> {
        self.process.start(&SystemSpawner::default(), timeout).await
    }

    /// Same as [`launch`](Self::launch) with an injected spawner (tests).
    pub async fn launch_with(
        &mut self,
        spawner: &dyn ProcessSpawner,
        timeout: Duration,
    ) -> TorResult<bootstrap::BootstrapStatus> {
        self.process.start(spawner, timeout).await
    }

    /// Stop the managed daemon, if any.
    pub async fn shutdown(&mut self) -> TorResult<()> {
        self.process.shutdown().await
    }

    /// Connect and authenticate to the control port.
    pub async fn connect_control(
        &self,
        auth: ControlAuth,
    ) -> TorResult<ControlClient<TcpControlTransport>> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.config.control_port));
        let transport = TcpControlTransport::connect(addr).await?;
        let mut client = ControlClient::new(transport);
        client.authenticate(&auth).await?;
        Ok(client)
    }

    /// Authenticate with the cookie the managed daemon wrote.
    pub fn cookie_auth(&self) -> TorResult<ControlAuth> {
        ControlAuth::cookie_from_file(&self.config.cookie_path())
    }

    /// Request new circuits, honouring tor's ten-second rate limit.
    ///
    /// Returns `false` (without sending) when called too soon.
    pub async fn newnym(
        &mut self,
        client: &mut ControlClient<TcpControlTransport>,
    ) -> TorResult<bool> {
        if !self.newnym.try_acquire(Instant::now()) {
            return Ok(false);
        }
        client.signal(TorSignal::Newnym).await?;
        Ok(true)
    }

    /// Build a leak guard reflecting the current running state.
    pub fn guard(&self) -> TorResult<TorLeakGuard> {
        let mut guard = TorLeakGuard::new(self.socks_addr()?);
        guard.set_active(self.process.is_running());
        Ok(guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn controller() -> TorController {
        let config = TorConfig {
            data_dir: PathBuf::from("/tmp/nanomite-tor-controller"),
            ..TorConfig::default()
        };
        TorController::new(config).expect("controller")
    }

    #[test]
    fn tor_result_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TorResult<()>>();
    }

    #[test]
    fn socks_addr_is_loopback() {
        let controller = controller();
        assert_eq!(
            controller.socks_addr().unwrap(),
            "127.0.0.1:9050".parse().unwrap()
        );
    }

    #[test]
    fn guard_starts_inactive_until_launched() {
        let controller = controller();
        let guard = controller.guard().expect("guard");
        assert!(!guard.is_active());
        assert_eq!(
            guard.route("93.184.216.34:443".parse().unwrap()),
            RouteDecision::Direct
        );
    }

    #[test]
    fn invalid_config_is_rejected() {
        let config = TorConfig {
            socks_port: 0,
            ..TorConfig::default()
        };
        assert!(TorController::new(config).is_err());
    }
}

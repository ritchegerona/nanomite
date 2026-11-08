//! Tor configuration and torrc generation.
//!
//! `TorConfig` is the single source of truth for a managed `tor` process: it is
//! rendered to a torrc file, validated before use, and every path it touches is
//! created with owner-only permissions (`0700` directories, `0600` files).
//! Nothing here performs network I/O, so the whole module is unit-testable.

use crate::nanomite_tor::{TorError, TorResult};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How the control port authenticates clients.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// No authentication (`CookieAuthentication 0`).
    None,
    /// Cookie file authentication (`CookieAuthentication 1`) — the default.
    #[default]
    Cookie,
}

/// One `Bridge` line, verbatim as tor expects it after the keyword.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeLine {
    /// e.g. `obfs4 192.0.2.1:443 FINGERPRINT cert=... iat-mode=0`.
    pub line: String,
}

impl BridgeLine {
    pub fn new(line: impl Into<String>) -> Self {
        Self { line: line.into() }
    }
}

/// A classic `HiddenServiceDir` / `HiddenServicePort` service.
///
/// Modern v3 services are created at runtime with `ADD_ONION` (see
/// [`crate::nanomite_tor::onion`]); this type covers the torrc form, which is
/// still useful for services that must exist before the control connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnionServiceConfig {
    /// Directory tor keeps the service key in.
    pub dir: PathBuf,
    /// `(virtual_port, target)` pairs. An empty target means tor forwards to
    /// the same port on localhost.
    pub ports: Vec<(u16, String)>,
}

impl OnionServiceConfig {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            ports: Vec::new(),
        }
    }

    pub fn with_port(mut self, virtual_port: u16, target: impl Into<String>) -> Self {
        self.ports.push((virtual_port, target.into()));
        self
    }
}

/// Complete configuration for a local Tor daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorConfig {
    /// Local SOCKS5 listener port (bound to loopback).
    #[serde(default = "_default_socks_port")]
    pub socks_port: u16,
    /// Local control listener port (bound to loopback).
    #[serde(default = "_default_control_port")]
    pub control_port: u16,
    /// Tor data directory (must be owner-only).
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    /// Control-port authentication mode.
    #[serde(default)]
    pub auth: AuthMode,
    /// Exit relays to prefer, as node fingerprints/names or two-letter country
    /// codes (e.g. `us`).
    #[serde(default)]
    pub exit_nodes: Vec<String>,
    /// Relays/countries never to use.
    #[serde(default)]
    pub exclude_nodes: Vec<String>,
    /// Treat `exit_nodes` as mandatory (`StrictNodes 1`).
    #[serde(default)]
    pub strict_nodes: bool,
    /// Bridge lines; only used when `use_bridges` is set.
    #[serde(default)]
    pub bridges: Vec<BridgeLine>,
    /// Enable the bridge/pluggable-transport path (`UseBridges 1`).
    #[serde(default)]
    pub use_bridges: bool,
    /// Enable Tor's connection padding (`ConnectionPadding 1`).
    #[serde(default = "_default_true")]
    pub connection_padding: bool,
    /// `CircuitBuildTimeout` in seconds.
    #[serde(default = "_default_circuit_timeout")]
    pub circuit_build_timeout: u32,
    /// Scrub sensitive details from logs (`SafeLogging 1`).
    #[serde(default = "_default_true")]
    pub safe_logging: bool,
    /// Services declared in torrc.
    #[serde(default)]
    pub hidden_services: Vec<OnionServiceConfig>,
    /// Extra raw torrc lines appended last (escape hatch).
    #[serde(default)]
    pub extra_lines: Vec<String>,
}

fn _default_socks_port() -> u16 {
    9050
}

fn _default_control_port() -> u16 {
    9051
}

fn _default_circuit_timeout() -> u32 {
    60
}

fn _default_true() -> bool {
    true
}

impl Default for TorConfig {
    fn default() -> Self {
        Self {
            socks_port: _default_socks_port(),
            control_port: _default_control_port(),
            data_dir: default_data_dir(),
            auth: AuthMode::default(),
            exit_nodes: Vec::new(),
            exclude_nodes: Vec::new(),
            strict_nodes: false,
            bridges: Vec::new(),
            use_bridges: false,
            connection_padding: true,
            circuit_build_timeout: _default_circuit_timeout(),
            safe_logging: true,
            hidden_services: Vec::new(),
            extra_lines: Vec::new(),
        }
    }
}

/// `NANOMITE_TOR_DIR` when set, else the platform data dir plus `nanomite/tor`.
pub fn default_data_dir() -> PathBuf {
    let override_dir = std::env::var_os("NANOMITE_TOR_DIR").map(PathBuf::from);
    default_data_dir_with(override_dir)
}

/// Pure form of [`default_data_dir`] so tests never mutate process env.
pub fn default_data_dir_with(override_dir: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = override_dir {
        return dir;
    }
    let base = dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("nanomite").join("tor")
}

impl TorConfig {
    /// Path of the control-auth cookie written by tor.
    pub fn cookie_path(&self) -> PathBuf {
        self.data_dir.join("control_auth_cookie")
    }

    /// Path of the generated torrc.
    pub fn torrc_path(&self) -> PathBuf {
        self.data_dir.join("torrc")
    }

    /// Validate the configuration without touching the filesystem.
    pub fn validate(&self) -> TorResult<()> {
        if self.socks_port == 0 {
            return Err(TorError::Config("socks_port must be non-zero".into()));
        }
        if self.control_port == 0 {
            return Err(TorError::Config("control_port must be non-zero".into()));
        }
        if self.socks_port == self.control_port {
            return Err(TorError::Config(
                "socks_port and control_port must differ".into(),
            ));
        }
        if self.data_dir.as_os_str().is_empty() {
            return Err(TorError::Config("data_dir must not be empty".into()));
        }
        if self.circuit_build_timeout == 0 {
            return Err(TorError::Config(
                "circuit_build_timeout must be non-zero".into(),
            ));
        }
        if self.use_bridges && self.bridges.is_empty() {
            return Err(TorError::Config(
                "use_bridges requires at least one bridge line".into(),
            ));
        }
        if self.bridges.iter().any(|b| b.line.trim().is_empty()) {
            return Err(TorError::Config("bridge lines must not be empty".into()));
        }
        for service in &self.hidden_services {
            if service.dir.as_os_str().is_empty() {
                return Err(TorError::Config(
                    "hidden service dir must not be empty".into(),
                ));
            }
            if service.ports.is_empty() {
                return Err(TorError::Config(
                    "hidden service needs at least one port".into(),
                ));
            }
        }
        Ok(())
    }

    /// Render the configuration as torrc lines (without a trailing newline).
    pub fn torrc_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("SocksPort 127.0.0.1:{}", self.socks_port),
            format!("ControlPort 127.0.0.1:{}", self.control_port),
            format!("DataDirectory {}", quote(&self.data_dir)),
            format!(
                "CookieAuthentication {}",
                if self.auth == AuthMode::Cookie { 1 } else { 0 }
            ),
            format!("SafeLogging {}", u8::from(self.safe_logging)),
            format!("ConnectionPadding {}", u8::from(self.connection_padding)),
            format!("CircuitBuildTimeout {}", self.circuit_build_timeout),
        ];

        if !self.exit_nodes.is_empty() {
            lines.push(format!("ExitNodes {}", node_set(&self.exit_nodes)));
        }
        if self.strict_nodes {
            lines.push("StrictNodes 1".to_string());
        }
        if !self.exclude_nodes.is_empty() {
            lines.push(format!("ExcludeNodes {}", node_set(&self.exclude_nodes)));
        }
        if self.use_bridges {
            lines.push("UseBridges 1".to_string());
            for bridge in &self.bridges {
                lines.push(format!("Bridge {}", bridge.line.trim()));
            }
        }
        for service in &self.hidden_services {
            lines.push(format!("HiddenServiceDir {}", quote(&service.dir)));
            for (virtual_port, target) in &service.ports {
                if target.trim().is_empty() {
                    lines.push(format!("HiddenServicePort {virtual_port}"));
                } else {
                    lines.push(format!(
                        "HiddenServicePort {virtual_port} {}",
                        target.trim()
                    ));
                }
            }
        }
        for extra in &self.extra_lines {
            let trimmed = extra.trim();
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
        }
        lines
    }

    /// Render the full torrc file body (trailing newline included).
    pub fn to_torrc(&self) -> String {
        let mut body = self.torrc_lines().join("\n");
        body.push('\n');
        body
    }
}

fn node_set(nodes: &[String]) -> String {
    let inner = nodes
        .iter()
        .map(|n| n.trim())
        .filter(|n| !n.is_empty())
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{inner}}}")
}

fn quote(path: &Path) -> String {
    let text = path.to_string_lossy();
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Create `path` (and parents) and restrict it to the owner.
pub fn ensure_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write a file with owner-only permissions, replacing any existing file.
pub fn write_private_file(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    std::fs::write(path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> TorConfig {
        TorConfig {
            data_dir: PathBuf::from("/tmp/nanomite-tor-test"),
            ..TorConfig::default()
        }
    }

    #[test]
    fn defaults_are_loopback_and_cookie_authed() {
        let c = TorConfig::default();
        assert_eq!(c.socks_port, 9050);
        assert_eq!(c.control_port, 9051);
        assert_eq!(c.auth, AuthMode::Cookie);
        assert!(c.connection_padding);
        assert!(c.safe_logging);
        c.validate().expect("defaults valid");
    }

    #[test]
    fn torrc_contains_core_lines() {
        let body = cfg().to_torrc();
        assert!(body.contains("SocksPort 127.0.0.1:9050"));
        assert!(body.contains("ControlPort 127.0.0.1:9051"));
        assert!(body.contains("CookieAuthentication 1"));
        assert!(body.contains("SafeLogging 1"));
        assert!(body.contains("CircuitBuildTimeout 60"));
        assert!(body.ends_with('\n'));
    }

    #[test]
    fn torrc_renders_exit_nodes_and_bridges() {
        let mut c = cfg();
        c.exit_nodes = vec!["us".into(), "de".into()];
        c.strict_nodes = true;
        c.exclude_nodes = vec!["ru".into()];
        c.use_bridges = true;
        c.bridges = vec![BridgeLine::new("obfs4 192.0.2.1:443 FP cert=x")];
        let body = c.to_torrc();
        assert!(body.contains("ExitNodes {us,de}"));
        assert!(body.contains("StrictNodes 1"));
        assert!(body.contains("ExcludeNodes {ru}"));
        assert!(body.contains("UseBridges 1"));
        assert!(body.contains("Bridge obfs4 192.0.2.1:443 FP cert=x"));
        c.validate().expect("valid");
    }

    #[test]
    fn torrc_renders_hidden_services() {
        let mut c = cfg();
        c.hidden_services = vec![OnionServiceConfig::new("/tmp/hs")
            .with_port(80, "127.0.0.1:8080")
            .with_port(22, "")];
        let body = c.to_torrc();
        assert!(body.contains("HiddenServiceDir \"/tmp/hs\""));
        assert!(body.contains("HiddenServicePort 80 127.0.0.1:8080"));
        assert!(body.contains("HiddenServicePort 22"));
        c.validate().expect("valid");
    }

    #[test]
    fn port_collision_is_rejected() {
        let mut c = cfg();
        c.control_port = c.socks_port;
        assert!(matches!(c.validate(), Err(TorError::Config(_))));
    }

    #[test]
    fn bridges_without_lines_are_rejected() {
        let mut c = cfg();
        c.use_bridges = true;
        assert!(c.validate().is_err());
        c.bridges = vec![BridgeLine::new("   ")];
        assert!(c.validate().is_err());
    }

    #[test]
    fn hidden_service_without_ports_is_rejected() {
        let mut c = cfg();
        c.hidden_services = vec![OnionServiceConfig::new("/tmp/hs")];
        assert!(c.validate().is_err());
    }

    #[test]
    fn data_dir_override_is_pure() {
        assert_eq!(
            default_data_dir_with(Some(PathBuf::from("/custom/tor"))),
            PathBuf::from("/custom/tor")
        );
        let fallback = default_data_dir_with(None);
        assert!(fallback.ends_with("nanomite/tor"));
    }

    #[test]
    fn quote_escapes_quotes() {
        let c = TorConfig {
            data_dir: PathBuf::from("/tmp/a \"b\" c"),
            ..TorConfig::default()
        };
        assert!(c
            .to_torrc()
            .contains("DataDirectory \"/tmp/a \\\"b\\\" c\""));
    }
}

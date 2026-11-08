//! `/vpn` and `/tor` slash commands plus the in-app privacy event surface.
//!
//! The VPN/TOR modules are feature-gated (`vpn`, `tor`). When a build does
//! not include a module, its command reports how to rebuild instead of
//! failing.

#[cfg(any(feature = "vpn", feature = "tor"))]
use std::path::Path;

use super::{App, Entry, EntryRole};
#[derive(Debug, Clone)]
pub enum PrivacyEvent {
    /// Informational line printed to the transcript.
    Notice(String),
    /// The result of a start/stop/status action.
    Done {
        module: String,
        action: String,
        ok: bool,
        text: String,
    },
}

pub(crate) fn run_vpn(this: &mut App, args: &str) {
    let args_owned = args.to_string();
    let trimmed = args_owned.trim();
    if trimmed.is_empty() || trimmed == "help" {
        this.toast("/vpn start|stop|status - start/stop/inspect the built-in VPN tunnel");
        this.mark_dirty();
        return;
    }
    let mut parts = trimmed.split_whitespace();
    let _action: String = parts.next().unwrap_or("").to_string();
    let _config_path = parts.next().map(|s| s.to_string());

    #[cfg(feature = "vpn")]
    {
        let tx = this.privacy_tx.clone();
        let module = "vpn".to_string();
        let action_for_toast = _action.clone();
        this.handle.spawn_blocking(move || {
            let result = match _action.as_str() {
                "start" => {
                    crate::privacy::vpn::start_blocking(_config_path.as_deref().map(Path::new))
                }
                "stop" => crate::privacy::vpn::stop_blocking(),
                "status" => crate::privacy::vpn::status_blocking(),
                other => Err(format!(
                    "unknown /vpn subcommand `{other}` - try start|stop|status|help"
                )),
            };
            let _ = tx.send(PrivacyEvent::Done {
                module,
                action: _action.to_string(),
                ok: result.is_ok(),
                text: result.unwrap_or_else(|e| e),
            });
        });
        this.toast(format!("vpn: {action_for_toast}..."));
    }
    #[cfg(not(feature = "vpn"))]
    {
        this.toast(
            "vpn module not compiled in - rebuild with `cargo build --release --features vpn`",
        );
    }
    this.mark_dirty();
}

pub(crate) fn run_tor(this: &mut App, args: &str) {
    let args_owned = args.to_string();
    let trimmed = args_owned.trim();
    if trimmed.is_empty() || trimmed == "help" {
        this.toast("/tor start|stop|status - start/stop/inspect the built-in Tor daemon");
        this.mark_dirty();
        return;
    }
    let mut parts = trimmed.split_whitespace();
    let _action: String = parts.next().unwrap_or("").to_string();
    let _data_dir = parts.next().map(|s| s.to_string());

    #[cfg(feature = "tor")]
    {
        let tx = this.privacy_tx.clone();
        let module = "tor".to_string();
        let action_for_toast = _action.clone();
        this.handle.spawn_blocking(move || {
            let result = match _action.as_str() {
                "start" => crate::privacy::tor::start_blocking(_data_dir.as_deref().map(Path::new)),
                "stop" => crate::privacy::tor::stop_blocking(),
                "status" => crate::privacy::tor::status_blocking(),
                other => Err(format!(
                    "unknown /tor subcommand `{other}` - try start|stop|status|help"
                )),
            };
            let _ = tx.send(PrivacyEvent::Done {
                module,
                action: _action.to_string(),
                ok: result.is_ok(),
                text: result.unwrap_or_else(|e| e),
            });
        });
        this.toast(format!("tor: {action_for_toast}..."));
    }
    #[cfg(not(feature = "tor"))]
    {
        this.toast(
            "tor module not compiled in - rebuild with `cargo build --release --features tor`",
        );
    }
    this.mark_dirty();
}

impl App {
    /// Drain one privacy event, same shape as every other async result
    /// channel handled in `pump_engine`.
    pub(crate) fn apply_privacy_event(&mut self, event: PrivacyEvent) {
        match event {
            PrivacyEvent::Notice(text) => self.toast(text),
            PrivacyEvent::Done {
                module,
                action,
                ok,
                text,
            } => {
                let prefix = if ok { "ok" } else { "error" };
                self.entries.push(Entry::new(
                    EntryRole::System,
                    format!("[{module}] {action}: {prefix} - {text}"),
                ));
                self.toast(format!("[{module}] {action} - {text}"));
            }
        }
        self.mark_dirty();
    }
}

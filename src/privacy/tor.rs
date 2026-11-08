//! In-process control surface for NanomiteTOR.
//!
//! [`start`] launches a local `tor` daemon (discovered on `PATH` as `tor`,
//! or via `NANOMITE_TOR_BIN`) through [`crate::nanomite_tor::process`],
//! waits for bootstrap, and keeps the [`TorController`] alive. [`stop`]
//! shuts the daemon down.

use crate::nanomite_tor::{config::TorConfig, TorController};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// Snapshot of the TOR module's current state.
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub running: bool,
    pub percent: u8,
    pub tag: String,
    pub summary: String,
    pub socks: Option<String>,
    pub control: Option<String>,
    pub note: Option<String>,
}

struct Runtime {
    controller: TorController,
    task: tokio::task::JoinHandle<()>,
}

static RUNTIME: OnceLock<Mutex<Option<Runtime>>> = OnceLock::new();

fn state_guard() -> std::sync::MutexGuard<'static, Option<Runtime>> {
    RUNTIME
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("tor lock poisoned")
}

/// Start (or report an already-running) `tor` daemon.
///
/// `data_dir` overrides the data directory; otherwise the default data dir is
/// used. Requires a `tor` binary on `PATH` (or `NANOMITE_TOR_BIN`).
pub async fn start(data_dir: Option<&Path>) -> Result<String, String> {
    if state_guard().is_some() {
        return Ok("tor is already running".to_string());
    }

    let mut config = TorConfig::default();
    if let Some(dir) = data_dir.filter(|d| !d.as_os_str().is_empty()) {
        config.data_dir = dir.to_path_buf();
    }
    let controller =
        TorController::new(config).map_err(|e| format!("tor controller failed: {e}"))?;
    let socks = controller
        .socks_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let control_port = controller.config().control_port;

    let mut controller = controller;
    let status = controller
        .launch(std::time::Duration::from_secs(60))
        .await
        .map_err(|e| format!("tor launch failed: {e}"))?;

    let task = tokio::spawn(async move {
        // Keep the controller alive until stop() is called. The controller's
        // drop impl shuts the daemon down; we park this task forever.
        tokio::time::sleep(std::time::Duration::from_secs(u64::MAX)).await;
    });

    state_guard().replace(Runtime { controller, task });

    Ok(format!(
        "tor started: bootstrap {}% ({}), socks {socks}, control 127.0.0.1:{control_port}",
        status.percent, status.tag
    ))
}

/// Stop the `tor` daemon.
pub async fn stop() -> Result<String, String> {
    let mut guard = state_guard();
    let Some(mut runtime) = guard.take() else {
        return Ok("tor is not running".to_string());
    };
    drop(guard);
    runtime.task.abort();
    runtime
        .controller
        .shutdown()
        .await
        .map_err(|e| e.to_string())?;
    Ok("tor stopped".to_string())
}

/// Current status snapshot.
pub fn status() -> Status {
    let guard = state_guard();
    let Some(runtime) = guard.as_ref() else {
        return Status::default();
    };
    let status = runtime.controller.status();
    Status {
        running: true,
        percent: status.percent,
        tag: status.tag.clone(),
        summary: status.summary.clone(),
        socks: runtime.controller.socks_addr().ok().map(|a| a.to_string()),
        control: Some(format!(
            "127.0.0.1:{}",
            runtime.controller.config().control_port
        )),
        note: None,
    }
}

/// Convenience: block the calling thread on [`start`] with a runtime handle.
pub fn start_blocking(data_dir: Option<&Path>) -> Result<String, String> {
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|e| format!("no tokio runtime available: {e}"))?;
    handle.block_on(start(data_dir))
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
    let status = status();
    Ok(format!(
        "tor: running={} bootstrap={}% ({}) socks={:?} control={:?}",
        status.running, status.percent, status.tag, status.socks, status.control
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_defaults_to_stopped() {
        let status = status();
        assert!(!status.running);
        assert_eq!(status.percent, 0);
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

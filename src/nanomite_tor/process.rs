//! Managed `tor` process supervisor.
//!
//! Renders a torrc, launches the system `tor` binary, follows its log until
//! bootstrap completes and stops it on request. The process boundary is behind
//! [`ProcessSpawner`] / [`TorChild`] so the supervisor is tested with a fake
//! child and never needs a real daemon.
//!
//! Tor is never bundled: the binary is resolved from `PATH` (or an explicit
//! path) and its absence is a clean [`TorError::Process`].

use crate::nanomite_tor::bootstrap::{self, BootstrapStatus};
use crate::nanomite_tor::config::{ensure_private_dir, write_private_file, TorConfig};
use crate::nanomite_tor::{TorError, TorResult};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdout, Command};

/// Cap on retained log lines (enough for diagnostics, bounded memory).
const MAX_LOG_LINES: usize = 2_000;

/// A spawned tor process.
#[async_trait]
pub trait TorChild: Send {
    /// OS process id.
    fn id(&self) -> u32;
    /// Next stdout line, or `None` at EOF.
    async fn next_stdout_line(&mut self) -> TorResult<Option<String>>;
    /// Terminate the process (idempotent).
    async fn kill(&mut self) -> TorResult<()>;
}

/// Creates tor children. Tests inject a fake; production uses
/// [`SystemSpawner`].
#[async_trait]
pub trait ProcessSpawner: Send + Sync {
    async fn spawn(&self, torrc: &Path) -> TorResult<Box<dyn TorChild>>;
}

/// Supervises one `tor` daemon.
pub struct TorProcess {
    config: TorConfig,
    child: Option<Box<dyn TorChild>>,
    status: BootstrapStatus,
    log: Vec<String>,
}

impl TorProcess {
    pub fn new(config: TorConfig) -> Self {
        Self {
            config,
            child: None,
            status: BootstrapStatus::default(),
            log: Vec::new(),
        }
    }

    pub fn config(&self) -> &TorConfig {
        &self.config
    }

    pub fn status(&self) -> &BootstrapStatus {
        &self.status
    }

    pub fn log(&self) -> &[String] {
        &self.log
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    /// Validate, create the data directory and write the torrc.
    pub fn prepare(&self) -> TorResult<()> {
        self.config.validate()?;
        ensure_private_dir(&self.config.data_dir)
            .map_err(|e| TorError::Process(format!("cannot create data directory: {e}")))?;
        write_private_file(
            self.config.torrc_path().as_path(),
            self.config.to_torrc().as_bytes(),
        )
        .map_err(|e| TorError::Process(format!("cannot write torrc: {e}")))?;
        Ok(())
    }

    /// Prepare, spawn and drive to bootstrap completion.
    pub async fn start(
        &mut self,
        spawner: &dyn ProcessSpawner,
        timeout: Duration,
    ) -> TorResult<BootstrapStatus> {
        self.prepare()?;
        let child = spawner.spawn(&self.config.torrc_path()).await?;
        self.child = Some(child);
        match self.wait_for_bootstrap(timeout).await {
            Ok(status) => Ok(status),
            Err(error) => {
                let _ = self.shutdown().await;
                Err(error)
            }
        }
    }

    /// Follow the log until `Bootstrapped 100%` or `timeout`.
    pub async fn wait_for_bootstrap(&mut self, timeout: Duration) -> TorResult<BootstrapStatus> {
        let Some(mut child) = self.child.take() else {
            return Err(TorError::NotRunning);
        };
        let result = tokio::time::timeout(
            timeout,
            read_until_bootstrapped(&mut *child, &mut self.log, &mut self.status),
        )
        .await;
        self.child = Some(child);
        match result {
            Ok(inner) => inner,
            Err(_) => Err(TorError::Timeout(format!(
                "tor did not bootstrap within {}s",
                timeout.as_secs()
            ))),
        }
    }

    /// Terminate the daemon, if running.
    pub async fn shutdown(&mut self) -> TorResult<()> {
        if let Some(mut child) = self.child.take() {
            child.kill().await?;
        }
        Ok(())
    }
}

async fn read_until_bootstrapped(
    child: &mut dyn TorChild,
    log: &mut Vec<String>,
    status: &mut BootstrapStatus,
) -> TorResult<BootstrapStatus> {
    loop {
        let Some(line) = child.next_stdout_line().await? else {
            return Err(TorError::Process(
                "tor exited before finishing bootstrap".into(),
            ));
        };
        log.push(line.clone());
        if log.len() > MAX_LOG_LINES {
            log.drain(..MAX_LOG_LINES / 2);
        }
        if let Some(update) = bootstrap::parse_log_line(&line) {
            *status = update;
            if status.done() {
                return Ok(status.clone());
            }
        }
    }
}

/// Spawns the real `tor` binary.
#[derive(Debug, Clone)]
pub struct SystemSpawner {
    pub tor_bin: PathBuf,
}

impl Default for SystemSpawner {
    fn default() -> Self {
        Self {
            tor_bin: PathBuf::from("tor"),
        }
    }
}

impl SystemSpawner {
    pub fn new(tor_bin: impl Into<PathBuf>) -> Self {
        Self {
            tor_bin: tor_bin.into(),
        }
    }
}

#[async_trait]
impl ProcessSpawner for SystemSpawner {
    async fn spawn(&self, torrc: &Path) -> TorResult<Box<dyn TorChild>> {
        let mut command = Command::new(&self.tor_bin);
        command
            .arg("-f")
            .arg(torrc)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|e| {
            TorError::Process(format!(
                "cannot launch {}: {e} (is tor installed?)",
                self.tor_bin.display()
            ))
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TorError::Process("tor stdout was not piped".into()))?;
        Ok(Box::new(SystemChild {
            child,
            stdout: BufReader::new(stdout),
        }))
    }
}

struct SystemChild {
    child: Child,
    stdout: BufReader<ChildStdout>,
}

#[async_trait]
impl TorChild for SystemChild {
    fn id(&self) -> u32 {
        self.child.id().unwrap_or(0)
    }

    async fn next_stdout_line(&mut self) -> TorResult<Option<String>> {
        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .await
            .map_err(|e| TorError::Process(format!("read tor log: {e}")))?;
        if read == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()))
    }

    async fn kill(&mut self) -> TorResult<()> {
        let _ = self.child.start_kill();
        self.child
            .wait()
            .await
            .map_err(|e| TorError::Process(format!("wait for tor to exit: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct FakeChild {
        id: u32,
        lines: VecDeque<String>,
        killed: Arc<AtomicBool>,
    }

    #[async_trait]
    impl TorChild for FakeChild {
        fn id(&self) -> u32 {
            self.id
        }

        async fn next_stdout_line(&mut self) -> TorResult<Option<String>> {
            Ok(self.lines.pop_front())
        }

        async fn kill(&mut self) -> TorResult<()> {
            self.killed.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    struct FakeSpawner {
        lines: Vec<String>,
        killed: Arc<AtomicBool>,
        spawned: Arc<AtomicBool>,
    }

    impl FakeSpawner {
        fn new(lines: &[&str]) -> Self {
            Self {
                lines: lines.iter().map(|l| (*l).to_string()).collect(),
                killed: Arc::new(AtomicBool::new(false)),
                spawned: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    #[async_trait]
    impl ProcessSpawner for FakeSpawner {
        async fn spawn(&self, _torrc: &Path) -> TorResult<Box<dyn TorChild>> {
            self.spawned.store(true, Ordering::SeqCst);
            Ok(Box::new(FakeChild {
                id: 4242,
                lines: self.lines.iter().cloned().collect(),
                killed: self.killed.clone(),
            }))
        }
    }

    fn config(dir: &Path) -> TorConfig {
        TorConfig {
            data_dir: dir.to_path_buf(),
            ..TorConfig::default()
        }
    }

    #[test]
    fn prepare_writes_torrc() {
        let dir = tempfile::tempdir().expect("tempdir");
        let process = TorProcess::new(config(dir.path()));
        process.prepare().expect("prepare");
        let torrc = std::fs::read_to_string(dir.path().join("torrc")).expect("read torrc");
        assert!(torrc.contains("SocksPort 127.0.0.1:9050"));
        assert!(torrc.contains("ControlPort 127.0.0.1:9051"));
    }

    #[test]
    fn prepare_rejects_invalid_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut cfg = config(dir.path());
        cfg.socks_port = 0;
        let process = TorProcess::new(cfg);
        assert!(process.prepare().is_err());
    }

    #[tokio::test]
    async fn start_reaches_bootstrap_done() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spawner = FakeSpawner::new(&[
            "Nov 08 [notice] Bootstrapped 10% (conn_done): Connected to a relay",
            "Nov 08 [notice] Opening Socks listener on 127.0.0.1:9050",
            "Nov 08 [notice] Bootstrapped 100% (done): Done",
        ]);
        let mut process = TorProcess::new(config(dir.path()));
        let status = process
            .start(&spawner, Duration::from_secs(2))
            .await
            .expect("start");
        assert!(status.done());
        assert!(process.is_running());
        assert!(spawner.spawned.load(Ordering::SeqCst));
        assert!(process.log().iter().any(|l| l.contains("Socks listener")));
    }

    #[tokio::test]
    async fn premature_exit_is_an_error_and_shuts_down() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spawner = FakeSpawner::new(&["[notice] Bootstrapped 40% (conn): Connecting"]);
        let mut process = TorProcess::new(config(dir.path()));
        let error = process
            .start(&spawner, Duration::from_secs(2))
            .await
            .unwrap_err();
        assert!(matches!(error, TorError::Process(_)));
        assert!(!process.is_running());
        assert!(spawner.killed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn wait_without_child_is_not_running() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut process = TorProcess::new(config(dir.path()));
        let error = process
            .wait_for_bootstrap(Duration::from_millis(10))
            .await
            .unwrap_err();
        assert!(matches!(error, TorError::NotRunning));
    }

    #[tokio::test]
    async fn shutdown_kills_the_child() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spawner = FakeSpawner::new(&["[notice] Bootstrapped 100% (done): Done"]);
        let mut process = TorProcess::new(config(dir.path()));
        process
            .start(&spawner, Duration::from_secs(2))
            .await
            .expect("start");
        process.shutdown().await.expect("shutdown");
        assert!(!process.is_running());
        assert!(spawner.killed.load(Ordering::SeqCst));
        process.shutdown().await.expect("idempotent");
    }
}

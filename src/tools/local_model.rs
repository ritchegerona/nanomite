//! # Local Model Support - Offline GGUF Inference via llama.cpp
//!
//! Nanomite can run reasoning models entirely offline by bundling llama.cpp
//! (specifically the llama-server binary) and serving GGUF models through an
//! OpenAI-compatible /chat/completions endpoint on localhost.
//!
//! This module manages the full lifecycle:
//! - Detect/install llama-server (via Homebrew llama.cpp or manual build)
//! - Download GGUF models from a configurable URL or local path
//! - Spawn llama-server on a free localhost port
//! - Health-check the server before handing it to the model layer
//! - Track loaded models in a persistent registry
//!
//! The LocalModelProvider is an OpenAiProvider that talks to the local
//! llama-server, so the existing streaming, tool-calling, and reasoning-delta
//! pipeline works unchanged for local models.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::io::AsyncWriteExt;

use crate::agent::openai::OpenAiProvider;
use crate::tools::auto_install::{AutoInstaller, PackageManager, SkillTracker};

/// Default port for the local llama-server endpoint.
pub const DEFAULT_LLAMA_PORT: u16 = 8080;

/// A locally loaded GGUF model served by a llama-server instance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocalModelEntry {
    /// Stable id used to refer to this model (e.g. llama-3.1-8b-q4).
    pub id: String,
    /// Human-readable label.
    pub label: String,
    /// Path to the GGUF file on disk.
    pub model_path: PathBuf,
    /// HTTP port the llama-server is listening on.
    pub port: u16,
    /// Base URL of the OpenAI-compatible endpoint (e.g. http://127.0.0.1:8080).
    pub base_url: String,
    /// When the model was loaded (unix seconds).
    pub loaded_at: u64,
    /// Whether the server is currently healthy.
    pub healthy: bool,
}

/// Persistent registry of loaded local models.
#[derive(Debug, Clone, Default)]
pub struct LocalModelRegistry {
    models: HashMap<String, LocalModelEntry>,
    path: Option<PathBuf>,
}

impl LocalModelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self { path: Some(path), ..Default::default() }
    }

    pub fn load(path: &PathBuf) -> Self {
        let models = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => HashMap::new(),
        };
        Self { models, path: Some(path.clone()) }
    }

    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(&self.models).unwrap_or_default();
        if std::fs::write(&tmp, &body).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    pub fn insert(&mut self, entry: LocalModelEntry) {
        self.models.insert(entry.id.clone(), entry);
        self.save();
    }

    pub fn get(&self, id: &str) -> Option<&LocalModelEntry> {
        self.models.get(id)
    }

    pub fn list(&self) -> Vec<&LocalModelEntry> {
        self.models.values().collect()
    }

    pub fn remove(&mut self, id: &str) -> Option<LocalModelEntry> {
        let removed = self.models.remove(id);
        self.save();
        removed
    }

    pub fn mark_healthy(&mut self, id: &str, healthy: bool) {
        if let Some(e) = self.models.get_mut(id) {
            e.healthy = healthy;
            self.save();
        }
    }
}

/// Lifecycle manager for local llama.cpp GGUF models.
#[derive(Debug, Clone)]
pub struct LocalModelManager {
    /// Where GGUF models are stored.
    models_dir: PathBuf,
    /// Persistent registry of loaded models.
    registry: Arc<Mutex<LocalModelRegistry>>,
    /// Auto-installer for llama.cpp / llama-server.
    installer: Arc<Mutex<AutoInstaller>>,
    /// Skill tracker shared with auto-install.
    skills: Arc<Mutex<SkillTracker>>,
}

impl LocalModelManager {
    pub fn new(
        models_dir: PathBuf,
        registry_path: PathBuf,
        skills: Arc<Mutex<SkillTracker>>,
    ) -> Self {
        let installer = Arc::new(Mutex::new(AutoInstaller::new(
            Arc::new(Mutex::new(SkillTracker::new())),
        )));
        let registry = Arc::new(Mutex::new(LocalModelRegistry::load(&registry_path)));

        // Ensure models dir exists.
        let _ = std::fs::create_dir_all(&models_dir);

        Self {
            models_dir,
            registry,
            installer,
            skills,
        }
    }

    /// Path where llama-server is expected to live (installed via brew or built).
    pub fn llama_server_path(&self) -> PathBuf {
        // Prefer a brew-installed llama-server; fall back to PATH lookup.
        if let Ok(p) = std::env::var("NANOMITE_LLAMA_SERVER") {
            return PathBuf::from(p);
        }
        PathBuf::from("llama-server")
    }

    /// Ensure llama-server is available on PATH, installing via brew if needed.
    pub async fn ensure_llama_server(&self) -> Result<bool, String> {
        let installer = self.installer.lock().await;
        installer.ensure_tool("llama-server").await
    }

    /// Ensure a GGUF model file is present locally, downloading it if a URL is given.
    ///
    /// model_url - optional http(s) URL to download the GGUF from.
    /// model_name - filename to store under models_dir.
    pub async fn ensure_model(
        &self,
        model_url: Option<&str>,
        model_name: &str,
    ) -> Result<PathBuf, String> {
        let dest = self.models_dir.join(model_name);

        // Already present.
        if dest.exists() {
            return Ok(dest);
        }

        // Download if a URL was provided.
        if let Some(url) = model_url {
            self.download_model(url, &dest).await?;
            return Ok(dest);
        }

        Err(format!(
            "model {model_name} not found at {} and no download URL provided",
            dest.display()
        ))
    }

    /// Download a GGUF model file to dest with a hard timeout.
    pub async fn download_model(
        &self,
        url: &str,
        dest: &Path,
    ) -> Result<(), String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|e| format!("reqwest build failed: {e}"))?;

        let mut resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("model download request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!(
                "model download returned {} for {url}",
                resp.status()
            ));
        }

        let mut file = tokio::fs::File::create(dest)
            .await
            .map_err(|e| format!("could not create model file: {e}"))?;

        while let Some(result) = resp.chunk().await.transpose() {
            let chunk = result.map_err(|e| format!("stream read error: {e}"))?;
            file.write_all(&chunk).await.map_err(|e| format!("model write error: {e}"))?;
        }

        file.flush().await.map_err(|e| format!("model flush error: {e}"))?;
        Ok(())
    }

    /// Find a free localhost port to bind llama-server to.
    fn find_free_port(&self) -> Result<u16, String> {
        // Try the default port first, then scan upward.
        for port in DEFAULT_LLAMA_PORT..=DEFAULT_LLAMA_PORT + 100 {
            if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
                return Ok(port);
            }
        }
        Err("no free localhost port for llama-server".to_string())
    }

    /// Spawn llama-server for the given model on a free port.
    ///
    /// model_id - stable id for the registry entry.
    /// model_path - path to the GGUF file.
    /// ctx_size - context window size.
    /// n_gpu_layers - GPU offload layers (0 = CPU only).
    pub async fn load_model(
        &self,
        model_id: &str,
        label: &str,
        model_path: &Path,
        ctx_size: Option<u32>,
        n_gpu_layers: Option<i32>,
    ) -> Result<LocalModelEntry, String> {
        // Ensure llama-server binary exists.
        self.ensure_llama_server().await?;

        let port = self.find_free_port()?;
        let base_url = format!("http://127.0.0.1:{port}");

        let mut cmd = Command::new(self.llama_server_path());
        cmd.arg("--model").arg(model_path);
        cmd.arg("--host").arg("127.0.0.1");
        cmd.arg("--port").arg(port.to_string());
        cmd.arg("--ctx-size").arg(ctx_size.unwrap_or(4096).to_string());

        if let Some(layers) = n_gpu_layers {
            cmd.arg("--n-gpu-layers").arg(layers.to_string());
        }

        // Enable OpenAI-compatible chat/completions endpoint.
        cmd.arg("--api").arg("openai");

        // Redirect server logs to a file for debugging.
        let log_path = self.models_dir.join(format!("llama-server-{model_id}.log"));
        let log_file = std::fs::File::create(&log_path)
            .map_err(|e| format!("could not create llama-server log: {e}"))?;
        cmd.stdout(Stdio::from(log_file.try_clone().unwrap()));
        cmd.stderr(Stdio::from(log_file));

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("llama-server spawn failed: {e}"))?;

        // Health-check: poll /health until the server responds.
        let deadline = Instant::now() + Duration::from_secs(120);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| format!("health check client build failed: {e}"))?;

        let mut healthy = false;
        while Instant::now() < deadline {
            if let Some(status) = child.try_wait().map_err(|e| format!("child wait error: {e}"))? {
                return Err(format!(
                    "llama-server exited early with {status}; check log at {}",
                    log_path.display()
                ));
            }
            match client.get(format!("{base_url}/health")).send().await {
                Ok(resp) if resp.status().is_success() => {
                    healthy = true;
                    break;
                }
                _ => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }

        if !healthy {
            let _ = child.kill();
            return Err(format!(
                "llama-server did not become healthy within 120s; check log at {}",
                log_path.display()
            ));
        }

        let entry = LocalModelEntry {
            id: model_id.to_string(),
            label: label.to_string(),
            model_path: model_path.to_path_buf(),
            port,
            base_url,
            loaded_at: crate::neural::now_secs(),
            healthy: true,
        };

        self.registry.lock().await.insert(entry.clone());
        Ok(entry)
    }

    /// Unload (stop) a locally served model by id.
    pub async fn unload_model(&self, model_id: &str) -> Result<(), String> {
        let mut reg = self.registry.lock().await;
        if let Some(entry) = reg.get(model_id) {
            // Attempt to kill the process listening on the port.
            let _ = std::process::Command::new("pkill")
                .arg("-f")
                .arg(format!("llama-server.*--port {}", entry.port))
                .status();
            reg.mark_healthy(model_id, false);
            reg.remove(model_id);
        }
        Ok(())
    }

    /// List all registered local models.
    pub async fn list_models(&self) -> Vec<LocalModelEntry> {
        self.registry.lock().await.list().into_iter().cloned().collect()
    }

    /// Registry accessor.
    pub async fn registry(&self) -> tokio::sync::MutexGuard<'_, LocalModelRegistry> {
        self.registry.lock().await
    }

    /// Models directory accessor.
    pub fn models_dir(&self) -> &PathBuf {
        &self.models_dir
    }

    /// Get an OpenAiProvider configured to talk to the local llama-server
    /// for the given model id.
    pub async fn provider_for(
        &self,
        model_id: &str,
    ) -> Result<OpenAiProvider, String> {
        let reg = self.registry.lock().await;
        let entry = reg
            .get(model_id)
            .ok_or_else(|| format!("local model {model_id} not loaded"))?;

        if !entry.healthy {
            return Err(format!("local model {model_id} is not healthy"));
        }

        OpenAiProvider::with_endpoint(&entry.base_url, None)
            .map_err(|e| format!("local model provider creation failed: {e}"))
    }
}

impl Default for LocalModelManager {
    fn default() -> Self {
        let dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("nanomite")
            .join("local-models");
        let registry = dir.join("registry.json");
        let skills = Arc::new(Mutex::new(SkillTracker::new()));
        Self::new(dir, registry, skills)
    }
}

/// Build an OpenAiProvider configured to talk to a local llama-server
/// endpoint. This lets the agent runtime reuse the existing OpenAI code path
/// with zero changes to streaming, tool calling, or reasoning-delta handling.
pub fn local_provider(base_url: &str) -> Result<OpenAiProvider, String> {
    OpenAiProvider::with_endpoint(base_url, None)
        .map_err(|e| format!("local model provider creation failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_models_dir() -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "nanomite_local_model_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn registry_round_trip() {
        let dir = temp_models_dir();
        let path = dir.join("registry.json");
        let mut reg = LocalModelRegistry::with_path(path.clone());

        reg.insert(LocalModelEntry {
            id: "test-model".to_string(),
            label: "Test Model".to_string(),
            model_path: PathBuf::from("/tmp/test.gguf"),
            port: 8080,
            base_url: "http://127.0.0.1:8080".to_string(),
            loaded_at: 1234567890,
            healthy: true,
        });

        let loaded = LocalModelRegistry::load(&path);
        assert!(loaded.get("test-model").is_some());
        assert_eq!(loaded.list().len(), 1);

        reg.remove("test-model");
        let empty = LocalModelRegistry::load(&path);
        assert!(empty.get("test-model").is_none());
        assert_eq!(empty.list().len(), 0);
    }

    #[test]
    fn find_free_port_is_bounded() {
        let mgr = LocalModelManager::default();
        // Default port range should return a port or an error (not hang).
        let _ = mgr.find_free_port();
    }

    #[test]
    fn default_models_dir_is_under_data_dir() {
        let mgr = LocalModelManager::default();
        assert!(mgr.models_dir.ends_with("nanomite/local-models"));
    }
}

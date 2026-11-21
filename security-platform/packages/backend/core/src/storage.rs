use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio::fs;
use uuid::Uuid;

pub struct LocalStorage {
    base_path: PathBuf,
    base_url: String,
}

impl LocalStorage {
    pub fn new(base_path: impl AsRef<Path>, base_url: impl Into<String>) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
            base_url: base_url.into(),
        }
    }

    fn full_path(&self, key: &str) -> PathBuf {
        self.base_path.join(key)
    }

    fn ensure_parent(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    pub async fn put(&self, key: &str, data: &[u8]) -> Result<String> {
        let path = self.full_path(key);
        self.ensure_parent(&path)?;
        fs::write(&path, data).await?;
        Ok(format!("{}/{}", self.base_url, key))
    }

    pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let path = self.full_path(key);
        if path.exists() {
            let data = fs::read(&path).await?;
            Ok(Some(data))
        } else {
            Ok(None)
        }
    }

    pub async fn delete(&self, key: &str) -> Result<()> {
        let path = self.full_path(key);
        if path.exists() {
            fs::remove_file(&path).await?;
        }
        Ok(())
    }

    pub async fn exists(&self, key: &str) -> Result<bool> {
        Ok(self.full_path(key).exists())
    }

    pub async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let mut results = Vec::new();
        let prefix_path = self.full_path(prefix);
        
        if prefix_path.exists() {
            let mut entries = fs::read_dir(&prefix_path).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(relative) = path.strip_prefix(&self.base_path) {
                        results.push(relative.to_string_lossy().to_string());
                    }
                }
            }
        }
        Ok(results)
    }

    pub async fn presigned_url(&self, key: &str, _expiry_secs: u64) -> Result<String> {
        // For local storage, just return the direct URL
        Ok(format!("{}/{}", self.base_url, key))
    }
}

pub enum StorageBackend {
    Local(LocalStorage),
}

impl StorageBackend {
    pub fn create(storage_type: &str, config: &crate::config::AppConfig) -> Result<Self> {
        match storage_type {
            "local" => {
                let base_path = config.storage.local_path.clone().unwrap_or_else(|| "./data/storage".into());
                let base_url = config.storage.base_url.clone().unwrap_or_else(|| {
                    format!("http://{}:{}/storage", config.server.host, config.server.port)
                });
                Ok(Self::Local(LocalStorage::new(base_path, base_url)))
            }
            "s3" => {
                Err(anyhow::anyhow!("S3 storage not implemented in lightweight build"))
            }
            _ => Err(anyhow::anyhow!("Unknown storage type: {}", storage_type)),
        }
    }

    pub async fn put(&self, key: &str, data: &[u8]) -> Result<String> {
        match self {
            Self::Local(s) => s.put(key, data).await,
        }
    }

    pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Local(s) => s.get(key).await,
        }
    }

    pub async fn delete(&self, key: &str) -> Result<()> {
        match self {
            Self::Local(s) => s.delete(key).await,
        }
    }

    pub async fn exists(&self, key: &str) -> Result<bool> {
        match self {
            Self::Local(s) => s.exists(key).await,
        }
    }

    pub async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        match self {
            Self::Local(s) => s.list(prefix).await,
        }
    }

    pub async fn presigned_url(&self, key: &str, expiry_secs: u64) -> Result<String> {
        match self {
            Self::Local(s) => s.presigned_url(key, expiry_secs).await,
        }
    }
}

// Helper for generating storage keys
pub fn generate_storage_key(org_id: Uuid, category: &str, filename: &str) -> String {
    format!("{}/{}/{}", org_id, category, filename)
}

pub fn generate_artifact_key(assessment_id: Uuid, tool_name: &str, filename: &str) -> String {
    format!("artifacts/{}/{}/{}", assessment_id, tool_name, filename)
}

pub fn generate_evidence_key(finding_id: Uuid, filename: &str) -> String {
    format!("evidence/{}/{}", finding_id, filename)
}

pub fn generate_report_key(assessment_id: Uuid, format: &str) -> String {
    format!("reports/{}/report.{}", assessment_id, format)
}
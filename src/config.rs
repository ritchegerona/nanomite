use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Where completions come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// Any `/v1/chat/completions` endpoint (OpenAI, NVIDIA, Groq, Together, Ollama, LM Studio, vLLM).
    OpenAi,
    /// Scripted provider for the offline test-suite only. Never compiled into the shipped binary.
    #[cfg(test)]
    Scripted,
}

/// A selectable endpoint offered by `/provider`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPreset {
    /// Stable id used in the picker and by `NANOMITE_PROVIDER`.
    pub id: &'static str,
    pub label: &'static str,
    pub base_url: &'static str,
    pub default_model: &'static str,
    /// Env var consulted for a key, if this endpoint needs one.
    pub key_env: Option<&'static str>,
    pub offline: bool,
}

/// Built-in endpoints. `custom` is appended at runtime when `NANOMITE_BASE_URL` differs.
/// Cloud providers first, then local runtimes. (The test-suite appends a scripted
/// provider via `#[cfg(test)]`; it never ships.)
pub const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "openai",
        label: "OpenAI",
        base_url: "https://api.openai.com/v1",
        default_model: "gpt-4o-mini",
        key_env: Some("OPENAI_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "anthropic",
        label: "Anthropic",
        base_url: "https://api.anthropic.com/v1",
        default_model: "claude-sonnet-4-5",
        key_env: Some("ANTHROPIC_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "gemini",
        label: "Google Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        default_model: "gemini-2.5-flash",
        key_env: Some("GEMINI_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "xai",
        label: "xAI (Grok)",
        base_url: "https://api.x.ai/v1",
        default_model: "grok-3-mini",
        key_env: Some("XAI_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "groq",
        label: "Groq",
        base_url: "https://api.groq.com/openai/v1",
        default_model: "llama-3.3-70b-versatile",
        key_env: Some("GROQ_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "mistral",
        label: "Mistral AI",
        base_url: "https://api.mistral.ai/v1",
        default_model: "mistral-small-latest",
        key_env: Some("MISTRAL_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "deepseek",
        label: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        default_model: "deepseek-chat",
        key_env: Some("DEEPSEEK_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "openrouter",
        label: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        default_model: "openrouter/auto",
        key_env: Some("OPENROUTER_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "together",
        label: "Together AI",
        base_url: "https://api.together.xyz/v1",
        default_model: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        key_env: Some("TOGETHER_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "fireworks",
        label: "Fireworks AI",
        base_url: "https://api.fireworks.ai/inference/v1",
        default_model: "accounts/fireworks/models/llama-v3p3-70b-instruct",
        key_env: Some("FIREWORKS_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "cerebras",
        label: "Cerebras",
        base_url: "https://api.cerebras.ai/v1",
        default_model: "llama-3.3-70b",
        key_env: Some("CEREBRAS_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "perplexity",
        label: "Perplexity",
        base_url: "https://api.perplexity.ai",
        default_model: "sonar",
        key_env: Some("PERPLEXITY_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "github",
        label: "GitHub Models",
        base_url: "https://models.github.ai/inference",
        default_model: "openai/gpt-4o-mini",
        key_env: Some("GITHUB_TOKEN"),
        offline: false,
    },
    ProviderPreset {
        id: "nvidia",
        label: "NVIDIA NIM",
        base_url: "https://integrate.api.nvidia.com/v1",
        default_model: "meta/llama-3.3-70b-instruct",
        key_env: Some("NVIDIA_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "dashscope",
        label: "Qwen (DashScope)",
        base_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        default_model: "qwen-plus",
        key_env: Some("DASHSCOPE_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "moonshot",
        label: "Moonshot (Kimi)",
        base_url: "https://api.moonshot.cn/v1",
        default_model: "kimi-k2-0905-preview",
        key_env: Some("MOONSHOT_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "zhipu",
        label: "Zhipu (GLM)",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        default_model: "glm-4.6",
        key_env: Some("ZHIPUAI_API_KEY"),
        offline: false,
    },
    ProviderPreset {
        id: "ollama",
        label: "Ollama",
        base_url: "http://localhost:11434/v1",
        default_model: "llama3.2",
        key_env: None,
        offline: true,
    },
    ProviderPreset {
        id: "lmstudio",
        label: "LM Studio",
        base_url: "http://localhost:1234/v1",
        default_model: "",
        key_env: None,
        offline: true,
    },
    ProviderPreset {
        id: "vllm",
        label: "vLLM",
        base_url: "http://localhost:8000/v1",
        default_model: "",
        key_env: None,
        offline: true,
    },
    ProviderPreset {
        id: "llamacpp",
        label: "llama.cpp server",
        base_url: "http://localhost:8080/v1",
        default_model: "",
        key_env: None,
        offline: true,
    },
    /// Scripted provider: test-suite only, never in the shipped roster.
    #[cfg(test)]
    ProviderPreset {
        id: "scripted",
        label: "Scripted (tests only)",
        base_url: "",
        default_model: "scripted-model",
        key_env: None,
        offline: true,
    },
];

pub fn find_preset(id: &str) -> Option<&'static ProviderPreset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// Maps a preset id to its backend. Production is always the real OpenAI-compatible
/// client; the scripted provider only exists in test builds.
fn kind_for_preset(id: &str) -> ProviderKind {
    #[cfg(test)]
    let kind = if id == "scripted" {
        ProviderKind::Scripted
    } else {
        ProviderKind::OpenAi
    };
    #[cfg(not(test))]
    let kind = ProviderKind::OpenAi;
    let _ = id;
    kind
}

fn preset_for_url(url: &str) -> Option<&'static ProviderPreset> {
    let url = url.trim_end_matches('/');
    PRESETS
        .iter()
        .find(|p| !p.base_url.is_empty() && p.base_url == url)
}

/// Reasoning effort forwarded to models that expose the `reasoning_effort` knob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reasoning {
    #[default]
    Medium,
    Minimal,
    Low,
    High,
}

impl Reasoning {
    pub const CYCLE: [Reasoning; 4] = [
        Reasoning::Minimal,
        Reasoning::Low,
        Reasoning::Medium,
        Reasoning::High,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Reasoning::Minimal => "minimal",
            Reasoning::Low => "low",
            Reasoning::Medium => "medium",
            Reasoning::High => "high",
        }
    }

    pub fn label(self) -> &'static str {
        self.as_str()
    }

    pub fn next(self) -> Self {
        let idx = Self::CYCLE.iter().position(|r| *r == self).unwrap_or(0);
        Self::CYCLE[(idx + 1) % Self::CYCLE.len()]
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub provider: ProviderKind,
    /// Active preset id, or `custom` when the URL is not one of the built-ins.
    pub preset: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    /// Model used when switching back to a preset that has one.
    pub default_model: String,
    pub reasoning: Reasoning,
    /// Token budget used for the context gauge in the status bar.
    pub context_limit: usize,
    pub max_steps: usize,
    pub max_file_bytes: usize,
    pub step_timeout_ms: u64,
    pub workspace: PathBuf,
    /// Shown once in the transcript when something needs the user's attention.
    pub notice: Option<String>,
    /// Where the setup is persisted; `None` disables saving (tests, one-off runs).
    pub store_path: Option<PathBuf>,
}

/// The subset of settings persisted to disk; everything else stays env-driven.
#[derive(Debug, Default, Clone)]
struct Stored {
    provider: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    reasoning: Option<String>,
}

/// Flat `key = "value"` subset of TOML: hand-editable, no extra dependency.
fn parse_stored(text: &str) -> Stored {
    let mut stored = Stored::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = unquote(value.trim());
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "provider" => stored.provider = Some(value),
            "base_url" => stored.base_url = Some(value),
            "api_key" => stored.api_key = Some(value),
            "model" => stored.model = Some(value),
            "reasoning" => stored.reasoning = Some(value),
            _ => {}
        }
    }
    stored
}

/// A missing or unreadable file simply means "nothing saved yet".
fn read_stored(path: &Path) -> Stored {
    fs::read_to_string(path)
        .map(|text| parse_stored(&text))
        .unwrap_or_default()
}

/// Minimal escaping so quotes and backslashes in values survive a round trip.
fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn unquote(value: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn stored_text(cfg: &Config) -> String {
    let mut lines = vec![
        "# nanomite setup - written automatically; edit by hand if you like.".to_string(),
        "# Precedence: built-in defaults < this file < environment variables.".to_string(),
        String::new(),
        format!("provider = {}", quote(&cfg.preset)),
        format!("base_url = {}", quote(&cfg.base_url)),
    ];
    if let Some(key) = &cfg.api_key {
        lines.push(format!("api_key = {}", quote(key)));
    }
    lines.push(format!("model = {}", quote(&cfg.model)));
    lines.push(format!("reasoning = {}", quote(cfg.reasoning.as_str())));
    lines.push(String::new());
    lines.join("\n")
}

/// Where the setup lives: `NANOMITE_CONFIG` wins, else `~/.config/nanomite/config.toml`.
fn default_store_path() -> PathBuf {
    if let Some(path) = first_env_one("NANOMITE_CONFIG") {
        return PathBuf::from(path);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nanomite")
        .join("config.toml")
}

/// Pre-rename location; read once so an existing setup (API key!) survives the rename.
fn legacy_store_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("qvr") // legacy pre-rename location
        .join("config.toml")
}

impl Config {
    /// Environment-only configuration: nothing is read from or written to disk.
    pub fn from_env() -> Self {
        Self::build(&Stored::default(), None, true)
    }

    /// Defaults <- `~/.config/nanomite/config.toml` <- environment.
    ///
    /// This is what the binary uses, so a one-time `/connect` setup sticks across
    /// restarts while environment variables still override a single invocation.
    pub fn load() -> Self {
        let path = default_store_path();
        if path.exists() {
            return Self::build(&read_stored(&path), Some(path), true);
        }
        let legacy = legacy_store_path();
        if legacy.exists() {
            return Self::build(&read_stored(&legacy), Some(path), true);
        }
        Self::build(&Stored::default(), Some(path), true)
    }

    /// File-only configuration: deterministic, no environment overlay (tests).
    pub fn load_from(path: &Path) -> Self {
        Self::build(&read_stored(path), Some(path.to_path_buf()), false)
    }

    fn build(stored: &Stored, store_path: Option<PathBuf>, use_env: bool) -> Self {
        let env_str = |keys: &[&str]| if use_env { first_env(keys) } else { None };
        let env_one = |key: &str| if use_env { first_env_one(key) } else { None };
        let workspace = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let base_env = env_str(&["NANOMITE_BASE_URL", "OPENAI_BASE_URL", "OPENAI_API_BASE"]);
        let mut base_url = base_env
            .clone()
            .or_else(|| stored.base_url.clone())
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        let model_env = env_str(&["NANOMITE_MODEL", "OPENAI_MODEL"]);
        let default_model = model_env
            .clone()
            .or_else(|| stored.model.clone())
            .unwrap_or_else(|| "gpt-4o-mini".to_string());

        // A provider saved in the file only counts if it is still a known preset.
        let file_provider = stored
            .provider
            .as_deref()
            .map(str::to_ascii_lowercase)
            .filter(|id| find_preset(id).is_some());
        let explicit = env_one("NANOMITE_PROVIDER")
            .map(|v| v.to_ascii_lowercase())
            .or(file_provider);
        let matched = preset_for_url(&base_url)
            .map(|p| p.id.to_string())
            .unwrap_or_else(|| "custom".to_string());
        let generic_key = env_str(&["NANOMITE_API_KEY", "OPENAI_API_KEY", "ANTHROPIC_API_KEY"])
            .or_else(|| stored.api_key.clone());

        let (provider, preset, model, api_key, mut notice): (
            ProviderKind,
            String,
            String,
            Option<String>,
            Option<String>,
        ) = match explicit.as_deref() {
            // Any built-in id is accepted: it pins the endpoint and default model
            // unless NANOMITE_BASE_URL / NANOMITE_MODEL say otherwise.
            Some(id) if find_preset(id).is_some() => {
                let p = find_preset(id).unwrap();
                if base_env.is_none() && stored.base_url.is_none() && !p.base_url.is_empty() {
                    base_url = p.base_url.to_string();
                }
                let model = model_env
                    .clone()
                    .or_else(|| stored.model.clone())
                    .or_else(|| (!p.default_model.is_empty()).then(|| p.default_model.to_string()))
                    .unwrap_or_else(|| default_model.clone());
                let key = env_str(&[p.key_env.unwrap_or("NANOMITE_API_KEY"), "NANOMITE_API_KEY"])
                    .or_else(|| stored.api_key.clone());
                let note = if key.is_none() && !p.offline {
                    Some(format!("{}: no API key set", p.label))
                } else {
                    None
                };
                (kind_for_preset(p.id), p.id.to_string(), model, key, note)
            }
            Some(other) => (
                ProviderKind::OpenAi,
                matched.clone(),
                default_model.clone(),
                generic_key,
                Some(format!("unknown NANOMITE_PROVIDER={other:?} ignored")),
            ),
            None => {
                let note = if generic_key.is_none() && !is_keyless(&base_url) {
                    Some("no API key set - keyless endpoints (Ollama, LM Studio) still work".into())
                } else {
                    None
                };
                (
                    ProviderKind::OpenAi,
                    matched.clone(),
                    default_model.clone(),
                    generic_key,
                    note,
                )
            }
        };

        if let Some(p) = find_preset(&preset) {
            if p.offline {
                notice = None;
            }
        }

        let reasoning = match env_str(&["NANOMITE_REASONING"])
            .as_deref()
            .or(stored.reasoning.as_deref())
        {
            Some("minimal") => Reasoning::Minimal,
            Some("low") => Reasoning::Low,
            Some("high") => Reasoning::High,
            _ => Reasoning::Medium,
        };

        Self {
            provider,
            preset,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            default_model,
            reasoning,
            context_limit: {
                let raw = if use_env {
                    num_env("NANOMITE_CONTEXT_LIMIT", 200_000)
                } else {
                    200_000
                };
                raw.max(1_000)
            },
            max_steps: if use_env {
                num_env("NANOMITE_MAX_STEPS", 12)
            } else {
                12
            },
            max_file_bytes: if use_env {
                num_env("NANOMITE_MAX_FILE_BYTES", 200_000)
            } else {
                200_000
            },
            step_timeout_ms: if use_env {
                num_env_u64("NANOMITE_STEP_TIMEOUT_MS", 120_000)
            } else {
                120_000
            },
            workspace,
            notice,
            store_path,
        }
    }

    /// Writes the setup to disk. No-op when persistence is disabled (tests).
    pub fn save(&self) {
        let Some(path) = &self.store_path else {
            return;
        };
        self.save_to(path);
    }

    /// Atomic write (tmp + rename) with owner-only permissions on the key.
    pub fn save_to(&self, path: &Path) {
        let text = stored_text(self);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("toml.tmp");
        if fs::write(&tmp, text).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
            }
            let _ = fs::rename(&tmp, path);
        }
    }

    /// Built-in presets plus a `custom` entry when the active URL is not one of them.
    pub fn presets(&self) -> Vec<ProviderPreset> {
        let mut list: Vec<ProviderPreset> = PRESETS.to_vec();
        if self.preset == "custom" {
            list.insert(
                0,
                ProviderPreset {
                    id: "custom",
                    label: "Custom endpoint",
                    base_url: "",
                    default_model: "",
                    key_env: Some("NANOMITE_API_KEY"),
                    offline: false,
                },
            );
        }
        list
    }

    pub fn current_preset(&self) -> Option<ProviderPreset> {
        self.presets().into_iter().find(|p| p.id == self.preset)
    }

    pub fn provider_label(&self) -> String {
        match self.current_preset() {
            Some(p) => p.label.to_string(),
            None => host_of(&self.base_url),
        }
    }

    /// Switches endpoint, key source and default model in one move.
    pub fn apply_preset(&mut self, id: &str) -> bool {
        let Some(preset) = find_preset(id) else {
            return false;
        };
        self.provider = kind_for_preset(preset.id);
        self.preset = preset.id.to_string();
        if !preset.base_url.is_empty() {
            self.base_url = preset.base_url.trim_end_matches('/').to_string();
        }
        if !preset.default_model.is_empty() {
            self.default_model = preset.default_model.to_string();
            self.model = preset.default_model.to_string();
        }
        self.api_key = preset
            .key_env
            .and_then(first_env_one)
            .or_else(|| first_env_one("NANOMITE_API_KEY"));
        self.notice = if self.api_key.is_none() && !preset.offline {
            Some(format!("{}: no API key set", preset.label))
        } else {
            None
        };
        // Provider switches are part of the setup: persist them immediately.
        self.save();
        true
    }

    /// Reasoning effort is only sent to models that are known to accept it.
    pub fn supports_reasoning_effort(&self) -> bool {
        let m = self.model.to_ascii_lowercase();
        m.starts_with("o1")
            || m.starts_with("o3")
            || m.starts_with("o4")
            || m.starts_with("gpt-5")
            || m.contains("reasoner")
    }

    pub fn endpoint(&self) -> String {
        if self.base_url.ends_with("/chat/completions") {
            self.base_url.clone()
        } else {
            format!("{}/chat/completions", self.base_url)
        }
    }
}

fn is_keyless(url: &str) -> bool {
    url.starts_with("http://localhost") || url.starts_with("http://127.0.0.1")
}

/// Authority of a base URL (`api.openai.com`, `localhost:11434`).
pub fn host_of(url: &str) -> String {
    let authority = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or_default();
    if authority.is_empty() {
        "localhost".to_string()
    } else {
        authority.to_string()
    }
}

fn first_env(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| first_env_one(k))
}

fn first_env_one(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn num_env(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

fn num_env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_unique() {
        let mut ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
        ids.sort();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate preset ids");
    }

    #[test]
    fn roster_is_well_formed() {
        for p in PRESETS {
            assert!(!p.label.is_empty(), "{} has no label", p.id);
            if p.id != "scripted" {
                assert!(p.base_url.starts_with("http"), "{} has no endpoint", p.id);
            }
        }
        let mut keys: Vec<_> = PRESETS.iter().filter_map(|p| p.key_env).collect();
        keys.sort();
        let count = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), count, "duplicate key env vars");
    }

    #[test]
    fn setup_roundtrips_through_disk() {
        let mut cfg = Config::from_env();
        assert!(cfg.apply_preset("deepseek"));
        cfg.api_key = Some("sk-test-123".into());
        cfg.reasoning = Reasoning::High;

        let path = std::env::temp_dir().join(format!("nanomite-config-{}.toml", std::process::id()));
        cfg.save_to(&path);

        let loaded = Config::load_from(&path);
        assert_eq!(loaded.preset, "deepseek");
        assert_eq!(loaded.base_url, "https://api.deepseek.com/v1");
        assert_eq!(loaded.model, "deepseek-chat");
        assert_eq!(loaded.api_key.as_deref(), Some("sk-test-123"));
        assert_eq!(loaded.reasoning, Reasoning::High);
        assert_eq!(loaded.provider_label(), "DeepSeek");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn apply_preset_switches_endpoint_and_model() {
        let mut cfg = Config::from_env();
        assert!(cfg.apply_preset("ollama"));
        assert_eq!(cfg.base_url, "http://localhost:11434/v1");
        assert_eq!(cfg.model, "llama3.2");
        assert!(cfg.api_key.is_none(), "ollama is keyless");

        assert!(cfg.apply_preset("groq"));
        assert_eq!(cfg.provider_label(), "Groq");
        assert_eq!(
            cfg.endpoint(),
            "https://api.groq.com/openai/v1/chat/completions"
        );

        // Newer roster entries resolve the same way.
        assert!(cfg.apply_preset("deepseek"));
        assert_eq!(
            cfg.endpoint(),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(cfg.model, "deepseek-chat");
        assert_eq!(cfg.provider_label(), "DeepSeek");

        assert!(cfg.apply_preset("vllm"));
        assert_eq!(cfg.base_url, "http://localhost:8000/v1");
        assert_eq!(cfg.provider_label(), "vLLM");

        assert!(cfg.apply_preset("scripted"));
        assert_eq!(cfg.provider, ProviderKind::Scripted);
        assert_eq!(cfg.model, "scripted-model");
        assert!(!cfg.apply_preset("nope"), "unknown ids must be rejected");
    }
}

use std::collections::BTreeMap;
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
    // Scripted provider: test-suite only, never in the shipped roster.
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

/// Known model context window sizes (in tokens).
/// Returns the context limit for a given model name, or a default if unknown.
pub fn model_context_limit(model: &str) -> usize {
    let m = model.to_ascii_lowercase();
    // OpenAI
    if m.contains("gpt-5.4")
        || m.contains("gpt-5.5")
        || m.contains("gpt-5.6")
        || m.contains("gpt-6")
    {
        return 1_050_000; // ~1.05M
    }
    if m.contains("gpt-5") {
        return 400_000; // GPT-5, 5.1, 5.2, 5.3
    }
    if m.contains("gpt-4.1") {
        return 1_047_576; // ~1M
    }
    if m.contains("gpt-4o") || m.contains("gpt-4.5") {
        return 128_000;
    }
    if m.contains("gpt-4") {
        return 128_000;
    }
    if m.contains("o1") || m.contains("o3") || m.contains("o4") {
        return 200_000;
    }
    if m.contains("gpt-3.5") {
        return 16_384;
    }
    // Anthropic
    if m.contains("claude-5")
        || m.contains("claude-4")
        || m.contains("claude-opus-4")
        || m.contains("claude-sonnet-4")
        || m.contains("claude-haiku-4")
        || m.contains("claude-fable")
        || m.contains("claude-mythos")
    {
        return 1_000_000;
    }
    if m.contains("claude-3.5-sonnet") || m.contains("claude-3.7-sonnet") {
        return 200_000;
    }
    if m.contains("claude-3.5-haiku") || m.contains("claude-3.5-opus") {
        return 200_000;
    }
    if m.contains("claude-3") {
        return 200_000;
    }
    // Google
    if m.contains("gemini-4") {
        return 1_048_576; // 1M unified
    }
    if m.contains("gemini-3") {
        return 1_048_576; // 1M
    }
    if m.contains("gemini-2.5") {
        return 1_000_000;
    }
    if m.contains("gemini-2.0") {
        return 2_000_000;
    }
    if m.contains("gemini-1.5") {
        return 2_000_000;
    }
    if m.contains("gemini") {
        return 1_000_000;
    }
    // xAI
    if m.contains("grok-4.7")
        || m.contains("grok-4.6")
        || m.contains("grok-4.5")
        || m.contains("grok-4.3")
        || m.contains("grok-4.2")
        || m.contains("grok-4.1")
    {
        return 1_000_000;
    }
    if m.contains("grok-4") {
        return 500_000; // grok-4, grok-4-fast
    }
    if m.contains("grok-3") {
        return 128_000;
    }
    if m.contains("grok") {
        return 128_000;
    }
    // NVIDIA Nemotron
    if m.contains("nemotron") {
        return 1_000_000;
    }
    // DeepSeek
    if m.contains("deepseek-v4") {
        return 1_000_000;
    }
    if m.contains("deepseek-r1") || m.contains("deepseek-v3") {
        return 128_000;
    }
    if m.contains("deepseek") {
        return 128_000;
    }
    // Meta / Llama
    if m.contains("llama-4-scout") {
        return 10_000_000; // 10M
    }
    if m.contains("llama-4") {
        return 1_048_576; // 1M
    }
    if m.contains("llama-3.3") {
        return 128_000;
    }
    if m.contains("llama-3.2") {
        return 128_000;
    }
    if m.contains("llama-3.1") {
        return 128_000;
    }
    if m.contains("llama-3") {
        return 8_192;
    }
    if m.contains("llama") {
        return 4_096;
    }
    // Mistral
    if m.contains("mistral-large-4") || m.contains("mistral-large-24") {
        return 1_000_000;
    }
    if m.contains("mistral-large") || m.contains("mistral-medium") || m.contains("mistral-small") {
        return 256_000; // Mistral Large 3, Medium 3.5, Small 4
    }
    if m.contains("mistral") {
        return 32_768;
    }
    // Qwen
    if m.contains("qwen3.8-max")
        || m.contains("qwen3.7-max")
        || m.contains("qwen3.8-plus")
        || m.contains("qwen3.7-plus")
    {
        return 1_000_000;
    }
    if m.contains("qwen3-max") || m.contains("qwen3.5-max") || m.contains("qwen3.6-max") {
        return 262_144; // 256K
    }
    if m.contains("qwen3-coder")
        || m.contains("qwen3-next")
        || m.contains("qwen3.6")
        || m.contains("qwen3.8")
        || m.contains("qwen3.5")
        || m.contains("qwen3")
    {
        return 262_144; // 256K native (some extensible to 1M via YaRN)
    }
    if m.contains("qwen-2.5") || m.contains("qwen-2") {
        return 128_000;
    }
    if m.contains("qwen") {
        return 128_000;
    }
    // Yi
    if m.contains("yi-1.5") || m.contains("yi-") {
        return 200_000;
    }
    // Phi
    if m.contains("phi-4") {
        return 128_000;
    }
    if m.contains("phi-3.5") || m.contains("phi-3") {
        return 128_000;
    }
    // GLM / Zhipu
    if m.contains("glm-4") {
        return 128_000;
    }
    // Test/scripted models
    if m.contains("scripted-model") {
        return 200_000;
    }
    // Generic fallbacks for common patterns
    if m.contains("32k") || m.contains("32000") {
        return 32_768;
    }
    if m.contains("128k") || m.contains("128000") {
        return 128_000;
    }
    if m.contains("200k") || m.contains("200000") {
        return 200_000;
    }
    if m.contains("256k") || m.contains("256000") || m.contains("262144") {
        return 262_144;
    }
    if m.contains("1m") || m.contains("1000000") || m.contains("1048576") {
        return 1_000_000;
    }
    if m.contains("2m") || m.contains("2000000") || m.contains("2097152") {
        return 2_000_000;
    }
    if m.contains("10m") || m.contains("10000000") || m.contains("10485760") {
        return 10_000_000;
    }
    // Default conservative estimate
    8_192
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

/// HACKER MODE settings. Disabled by default: entering the cybersecurity
/// environment is always an explicit opt-in. No credential fields live here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HackerConfig {
    /// Whether the `/hacker` environment may be entered at all.
    pub enabled: bool,
    /// Authorized scope: CIDRs, IP literals and/or domain names. This is the
    /// user's authorization input and is never derived from model output.
    pub scope: Vec<String>,
    /// Assessment id shown in the workspace and the audit trail.
    pub assessment_id: String,
    /// How long an activated assessment stays valid.
    pub session_minutes: u64,
    /// Owner decision: skip the interactive approval prompt while an
    /// authorized assessment is active. Still gated on a valid, unexpired,
    /// in-scope assessment and always audited.
    pub auto_approve: bool,
    /// Write the append-only security audit log.
    pub audit: bool,
    /// Persist findings and evidence to the SQLite store. Off means operations
    /// still run and audit, but nothing structured is retained.
    pub store: bool,
    /// Capability token that binds an `OpCapability` to this session. When set
    /// (`NANOMITE_HACKER_CAPABILITY_TOKEN`), every `/hacker run` is gated
    /// through the typestate capability layer before execution. When absent,
    /// operations fall back to the existing `HackerSession` authorization.
    pub capability_token: Option<String>,
    /// Modules permitted by the capability ceiling. When the token is set but
    /// this is empty, only the four read-only modules (recon, network, web,
    /// vuln) are allowed.
    pub allowed_modules: Option<Vec<String>>,
}

impl Default for HackerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            scope: Vec::new(),
            assessment_id: "local".into(),
            session_minutes: 60,
            auto_approve: true,
            audit: true,
            store: true,
            capability_token: None,
            allowed_modules: None,
        }
    }
}

/// Errors produced when validating a `HackerConfig`.
#[derive(Debug)]
pub enum HackerConfigError {
    /// `enabled = true` but no scope was configured.
    ScopeRequired,
    /// `session_minutes` is zero (window would never open).
    SessionMinutesZero,
    /// A scope entry is neither a valid CIDR nor a valid domain name.
    InvalidScopeEntry(String),
}

impl std::fmt::Display for HackerConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HackerConfigError::ScopeRequired => {
                write!(f, "hacker.enabled = true but no authorized scope configured")
            }
            HackerConfigError::SessionMinutesZero => {
                write!(f, "hacker.session_minutes must be greater than zero")
            }
            HackerConfigError::InvalidScopeEntry(entry) => {
                write!(f, "invalid hacker scope entry: {entry}")
            }
        }
    }
}

impl std::error::Error for HackerConfigError {}

impl HackerConfig {
    /// Validate the configuration. Call this after loading from file/env
    /// and before activating HACKER MODE.
    pub fn validate(&self) -> Result<(), HackerConfigError> {
        if self.enabled && self.scope.is_empty() {
            return Err(HackerConfigError::ScopeRequired);
        }
        if self.session_minutes == 0 {
            return Err(HackerConfigError::SessionMinutesZero);
        }

        for entry in &self.scope {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !trimmed.contains('/')
                && !is_ip_literal(trimmed)
                && !is_domain(trimmed)
            {
                return Err(HackerConfigError::InvalidScopeEntry(
                    trimmed.to_string(),
                ));
            }
        }

        Ok(())
    }
}

fn is_ip_literal(s: &str) -> bool {
    s.parse::<std::net::IpAddr>().is_ok()
}

fn is_domain(s: &str) -> bool {
    s.contains('.') && s.chars().all(|c| c.is_ascii() && !c.is_whitespace())
}

#[derive(Debug, Clone)]
pub struct Config {
    pub provider: ProviderKind,
    /// Active preset id, or `custom` when the URL is not one of the built-ins.
    pub preset: String,
    pub base_url: String,
    pub api_key: Option<String>,
    /// Per-provider API keys, keyed by preset id. The active preset's key is
    /// mirrored in `api_key`, so several providers can stay configured at once
    /// and switching presets restores that provider's key. Persisted as the
    /// `[providers]` section.
    pub provider_keys: BTreeMap<String, String>,
    pub model: String,
    /// Model used when switching back to a preset that has one.
    pub default_model: String,
    pub reasoning: Reasoning,
    /// UI skin persisted across sessions (`mono`, `umbrella`, …).
    pub theme: String,
    /// Tools hidden from the agent ("tool library" toggle): names are absent
    /// from the provider's schema list and refused at dispatch. Persisted as
    /// a comma-separated list; `NANOMITE_DISABLED_TOOLS` overrides the file.
    pub disabled_tools: Vec<String>,
    /// Central tool authorization policy: auto (default) / plan / readonly.
    /// Persisted as a single keyword; `NANOMITE_TOOL_POLICY` overrides the file.
    pub tool_policy: crate::agent::policy::ToolPolicy,
    /// Ask before External-risk tool calls (shell, SSH, destructive deletes)
    /// run, even when the mode policy allows them: y allow, a always allow,
    /// n deny, d always deny. Persisted; `NANOMITE_CONFIRM_EXTERNAL`
    /// overrides the file.
    pub confirm_external: bool,
    /// Token budget used for the context gauge in the status bar.
    pub context_limit: usize,
    pub max_steps: usize,
    pub max_tool_calls: usize,
    pub max_file_bytes: usize,
    pub step_timeout_ms: u64,
    /// Disable animations (spinners, fades) for reduced-motion preference.
    pub reduced_motion: bool,
    /// opencode-style follow-up suggestions: after each finished turn a tiny
    /// side-call asks the model for 3 short next-step prompts, shown as a
    /// strip above the composer. Persisted; `NANOMITE_SUGGESTIONS` overrides.
    pub followup_suggestions: bool,
    /// Compact transcript density: single-row prompts, no message footers,
    /// tighter gaps. Persisted; `/density` or `ctrl+g` toggles it live.
    pub compact: bool,
    pub workspace: PathBuf,
    /// Shown once in the transcript when something needs the user's attention.
    pub notice: Option<String>,
    /// Where the setup is persisted; `None` disables saving (tests, one-off runs).
    pub store_path: Option<PathBuf>,
    /// Extra instructions appended to the system prompt on every turn.
    ///
    /// This is the same file the self-improvement engine edits
    /// (`Target::Prompt`): hand-write it, or let the agent propose a change to
    /// it in `/improve`. Defaults to `~/.config/nanomite/prompt.md`.
    pub prompt_file: Option<PathBuf>,
    /// HACKER MODE settings (`[hacker]` section).
    pub hacker: HackerConfig,
    /// Run a learning session automatically once per calendar day. Off by
    /// default: a daily session spends model tokens, so it is an explicit
    /// opt-in. Persisted; `NANOMITE_LEARN_DAILY` overrides.
    pub learning_daily: bool,
    /// Pinned learning topic. `None` rotates through [`crate::learn::DEFAULT_TOPICS`].
    pub learning_topic: Option<String>,
}

/// The subset of settings persisted to disk; everything else stays env-driven.
#[derive(Debug, Default, Clone)]
struct Stored {
    provider: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    /// `[providers]` section: preset id -> API key.
    provider_keys: BTreeMap<String, String>,
    model: Option<String>,
    reasoning: Option<String>,
    theme: Option<String>,
    disabled_tools: Option<String>,
    tool_policy: Option<String>,
    prompt_file: Option<String>,
    reduced_motion: Option<bool>,
    suggestions: Option<bool>,
    compact: Option<bool>,
    confirm_external: Option<bool>,
    // `[hacker]` section.
    hacker_enabled: Option<bool>,
    hacker_scope: Option<String>,
    hacker_assessment_id: Option<String>,
    hacker_session_minutes: Option<u64>,
    hacker_auto_approve: Option<bool>,
    hacker_audit: Option<bool>,
    hacker_store: Option<bool>,
    hacker_allowed_modules: Option<Vec<String>>,
    // `[learn]` section.
    learning_daily: Option<bool>,
    learning_topic: Option<String>,
}

/// Flat `key = "value"` subset of TOML: hand-editable, no extra dependency.
fn parse_stored(text: &str) -> Stored {
    let mut stored = Stored::default();
    // `[section]` headers switch the table keys are matched against. Only the
    // top level and `[hacker]` are recognised; other sections are ignored so
    // hand-added comment blocks never break loading.
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = inner.trim().to_ascii_lowercase();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = unquote(value.trim());
        if value.is_empty() {
            continue;
        }
        let key = key.trim();
        match section.as_str() {
            "providers" => {
                let id = key.trim().to_ascii_lowercase();
                if !id.is_empty() {
                    stored.provider_keys.insert(id, value);
                }
            }
            "hacker" => match key {
                "enabled" => stored.hacker_enabled = Some(parse_bool(&value)),
                "scope" => stored.hacker_scope = Some(value),
                "assessment_id" => stored.hacker_assessment_id = Some(value),
                "session_minutes" => stored.hacker_session_minutes = value.parse().ok(),
                "auto_approve" => stored.hacker_auto_approve = Some(parse_bool(&value)),
                "audit" => stored.hacker_audit = Some(parse_bool(&value)),
                "store" => stored.hacker_store = Some(parse_bool(&value)),
                "allowed_modules" => {
                    stored.hacker_allowed_modules = Some(
                        value.split([',', ' ', '\t', '\n'])
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .collect(),
                    );
                }
                _ => {}
            },
            "learn" => match key {
                "daily" => stored.learning_daily = parse_bool_opt(&value),
                "topic" => stored.learning_topic = Some(value),
                _ => {}
            },
            "" => match key {
                "provider" => stored.provider = Some(value),
                "base_url" => stored.base_url = Some(value),
                "api_key" => stored.api_key = Some(value),
                "model" => stored.model = Some(value),
                "reasoning" => stored.reasoning = Some(value),
                "theme" => stored.theme = Some(value),
                "disabled_tools" => stored.disabled_tools = Some(value),
                "tool_policy" => stored.tool_policy = Some(value),
                "prompt_file" => stored.prompt_file = Some(value),
                "reduced_motion" => stored.reduced_motion = parse_bool_opt(&value),
                "suggestions" => stored.suggestions = parse_bool_opt(&value),
                "compact" => stored.compact = parse_bool_opt(&value),
                "confirm_external" => stored.confirm_external = parse_bool_opt(&value),
                _ => {}
            },
            _ => {}
        }
    }
    stored
}

/// Lenient boolean: "1/true/yes/on" (case-insensitive) is true; anything else
/// is false. Matches the env conventions elsewhere in this file.
fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Like [`parse_bool`] but `None` for values that are neither true nor false,
/// so a typo does not silently become `false`.
fn parse_bool_opt(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
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

/// Comma-separated tool-name list → trimmed, lowercased, de-duplicated
/// names. Unknown names are tolerated: a disabled list may name a tool this
/// build does not have, which simply disables nothing.
pub fn parse_disabled_tools(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in raw.split(',') {
        let name = part.trim().to_ascii_lowercase();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
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
    lines.push(format!("theme = {}", quote(&cfg.theme)));
    if !cfg.disabled_tools.is_empty() {
        lines.push(format!(
            "disabled_tools = {}",
            quote(&cfg.disabled_tools.join(","))
        ));
    }
    if cfg.tool_policy != crate::agent::policy::ToolPolicy::default() {
        lines.push(format!("tool_policy = {}", quote(cfg.tool_policy.as_str())));
    }
    if let Some(path) = &cfg.prompt_file {
        lines.push(format!(
            "prompt_file = {}",
            quote(&path.display().to_string())
        ));
    }
    lines.push(format!("reduced_motion = {}", cfg.reduced_motion));
    lines.push(format!("suggestions = {}", cfg.followup_suggestions));
    lines.push(format!("compact = {}", cfg.compact));
    if !cfg.confirm_external {
        lines.push("confirm_external = false".to_string());
    }
    lines.push(String::new());
    // Per-provider keys. Kept as a section so several providers stay configured
    // at once; the active provider's key is also mirrored in the top-level
    // `api_key` above for older readers.
    if !cfg.provider_keys.is_empty() {
        lines.push("[providers]".to_string());
        for (id, key) in &cfg.provider_keys {
            lines.push(format!("{id} = {}", quote(key)));
        }
        lines.push(String::new());
    }
    // HACKER MODE: written as a section so the knobs are discoverable. Disabled
    // by default; enabling and set the authorized scope explicitly.
    lines.push("[hacker]".to_string());
    lines.push(format!("enabled = {}", cfg.hacker.enabled));
    if !cfg.hacker.scope.is_empty() {
        lines.push(format!("scope = {}", quote(&cfg.hacker.scope.join(","))));
    }
    lines.push(format!(
        "assessment_id = {}",
        quote(&cfg.hacker.assessment_id)
    ));
    lines.push(format!("session_minutes = {}", cfg.hacker.session_minutes));
    lines.push(format!("auto_approve = {}", cfg.hacker.auto_approve));
    lines.push(format!("audit = {}", cfg.hacker.audit));
    lines.push(format!("store = {}", cfg.hacker.store));
    if let Some(modules) = &cfg.hacker.allowed_modules {
        if !modules.is_empty() {
            lines.push(format!("allowed_modules = {}", quote(&modules.join(","))));
        }
    }
    lines.push(String::new());
    // Daily learning loop. Off by default (each session spends model tokens);
    // `/learn daily on` flips it, `topic` pins a subject instead of rotating.
    lines.push("[learn]".to_string());
    lines.push(format!("daily = {}", cfg.learning_daily));
    if let Some(topic) = &cfg.learning_topic {
        lines.push(format!("topic = {}", quote(topic)));
    }
    lines.push(String::new());
    lines.join("\n")
}

/// Where the setup lives: `NANOMITE_CONFIG` wins, else `~/.config/nanomite/config.toml`.
pub fn default_store_path() -> PathBuf {
    if let Some(path) = first_env_one("NANOMITE_CONFIG") {
        return PathBuf::from(path);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nanomite")
        .join("config.toml")
}

/// Default location of the custom prompt file: `~/.config/nanomite/prompt.md`,
/// overridable with `NANOMITE_PROMPT_FILE` or the persisted `prompt_file` key.
/// The file is optional: a missing file is silent, not an error.
pub fn default_prompt_path() -> PathBuf {
    if let Some(path) = first_env_one("NANOMITE_PROMPT_FILE") {
        return PathBuf::from(path);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nanomite")
        .join("prompt.md")
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
        // Per-provider keys saved so far; the active preset's entry is kept in
        // sync with `api_key` below, which also migrates the legacy single key.
        let mut provider_keys = stored.provider_keys.clone();

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
                    .or_else(|| provider_keys.get(p.id).cloned())
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

        // Keep the active provider in the map so its key survives a save, and
        // so a legacy top-level `api_key` migrates into per-provider storage.
        if let Some(key) = &api_key {
            if !preset.is_empty() {
                provider_keys.insert(preset.clone(), key.clone());
            }
        }

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

        let theme = env_str(&["NANOMITE_THEME"])
            .or_else(|| stored.theme.clone())
            .and_then(|label| {
                crate::ui::theme::ThemeId::from_label(&label).map(|id| id.label().to_string())
            })
            .unwrap_or_else(|| crate::ui::theme::ThemeId::Umbrella.label().to_string());

        // Disabled tools: environment wins over the file, same as every other
        // persisted setting.
        let disabled_tools = {
            let raw = if use_env {
                first_env_one("NANOMITE_DISABLED_TOOLS")
            } else {
                None
            }
            .or_else(|| stored.disabled_tools.clone())
            .unwrap_or_default();
            parse_disabled_tools(&raw)
        };

        let tool_policy = {
            let raw = if use_env {
                first_env_one("NANOMITE_TOOL_POLICY")
            } else {
                None
            }
            .or_else(|| stored.tool_policy.clone())
            .unwrap_or_default();
            crate::agent::policy::ToolPolicy::parse(&raw).unwrap_or_default()
        };

        let confirm_external = {
            let raw = if use_env {
                first_env_one("NANOMITE_CONFIRM_EXTERNAL")
            } else {
                None
            };
            match raw.as_deref() {
                Some("0") | Some("false") | Some("off") => false,
                Some(_) => true,
                None => stored.confirm_external.unwrap_or(true),
            }
        };

        // HACKER MODE: disabled unless explicitly enabled, and the scope is
        // the user's authorization input. Env overrides the file, as usual.
        let hacker = {
            let enabled = if use_env {
                first_env_one("NANOMITE_HACKER")
                    .map(|v| parse_bool(&v))
                    .unwrap_or_else(|| stored.hacker_enabled.unwrap_or(false))
            } else {
                stored.hacker_enabled.unwrap_or(false)
            };
            let scope_raw = if use_env {
                first_env_one("NANOMITE_HACKER_SCOPE")
            } else {
                None
            }
            .or_else(|| stored.hacker_scope.clone())
            .unwrap_or_default();
            let scope: Vec<String> = scope_raw
                .split([',', ' ', '\t', '\n'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            let assessment_id = if use_env {
                first_env_one("NANOMITE_HACKER_ASSESSMENT")
            } else {
                None
            }
            .or_else(|| stored.hacker_assessment_id.clone())
            .unwrap_or_else(|| "local".to_string());
            let session_minutes = if use_env {
                num_env_u64("NANOMITE_HACKER_SESSION_MINUTES", 60)
            } else {
                stored.hacker_session_minutes.unwrap_or(60)
            };
            let auto_approve = if use_env {
                match first_env_one("NANOMITE_HACKER_AUTO_APPROVE") {
                    Some(v) => parse_bool(&v),
                    None => stored.hacker_auto_approve.unwrap_or(true),
                }
            } else {
                stored.hacker_auto_approve.unwrap_or(true)
            };
            let audit = if use_env {
                match first_env_one("NANOMITE_HACKER_AUDIT") {
                    Some(v) => parse_bool(&v),
                    None => stored.hacker_audit.unwrap_or(true),
                }
            } else {
                stored.hacker_audit.unwrap_or(true)
            };
            let store = if use_env {
                match first_env_one("NANOMITE_HACKER_STORE") {
                    Some(v) => parse_bool(&v),
                    None => stored.hacker_store.unwrap_or(true),
                }
            } else {
                stored.hacker_store.unwrap_or(true)
            };
            let capability_token = if use_env {
                first_env_one("NANOMITE_HACKER_CAPABILITY_TOKEN")
            } else {
                None
            };
            let allowed_modules = if use_env {
                first_env_one("NANOMITE_HACKER_MODULES").map(|raw| {
                    raw.split([',', ' ', '\t', '\n'])
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect()
                })
            } else {
                stored.hacker_allowed_modules.clone()
            };
            HackerConfig {
                enabled,
                scope,
                assessment_id,
                session_minutes: session_minutes.max(1),
                auto_approve,
                audit,
                store,
                capability_token,
                allowed_modules,
            }
        };

        // Custom prompt file: the environment wins over the persisted path, and the
        // default location is always resolved so callers can report it.
        let prompt_file = (if use_env {
            first_env_one("NANOMITE_PROMPT_FILE")
        } else {
            None
        })
        .map(PathBuf::from)
        .or_else(|| stored.prompt_file.clone().map(PathBuf::from))
        .or_else(|| Some(default_prompt_path()));

        Self {
            provider,
            preset,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            provider_keys,
            model,
            default_model,
            reasoning,
            theme,
            disabled_tools,
            tool_policy,
            confirm_external,
            context_limit: {
                let raw = if use_env {
                    num_env("NANOMITE_CONTEXT_LIMIT", 200_000)
                } else {
                    200_000
                };
                raw.max(1_000)
            },
            max_steps: if use_env {
                num_env("NANOMITE_MAX_STEPS", usize::MAX)
            } else {
                usize::MAX
            },
            max_tool_calls: if use_env {
                num_env("NANOMITE_MAX_TOOL_CALLS", usize::MAX)
            } else {
                usize::MAX
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
            prompt_file,
            reduced_motion: if use_env {
                first_env_one("NANOMITE_REDUCED_MOTION")
                    .map(|v| v.parse().unwrap_or(false))
                    .unwrap_or(false)
            } else {
                stored.reduced_motion.unwrap_or(false)
            },
            followup_suggestions: if use_env {
                // `0`/`false` opt out; absent means on. (bool::from_str rejects
                // "0", so the match table keeps `NANOMITE_SUGGESTIONS=0` working.)
                match first_env_one("NANOMITE_SUGGESTIONS") {
                    Some(v) => {
                        matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
                    }
                    None => true,
                }
            } else {
                stored.suggestions.unwrap_or(true)
            },
            compact: if use_env {
                matches!(first_env_one("NANOMITE_COMPACT"), Some(v) if matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            } else {
                stored.compact.unwrap_or(false)
            },
            learning_daily: if use_env {
                match first_env_one("NANOMITE_LEARN_DAILY") {
                    Some(v) => parse_bool(&v),
                    None => stored.learning_daily.unwrap_or(false),
                }
            } else {
                stored.learning_daily.unwrap_or(false)
            },
            learning_topic: (if use_env {
                first_env_one("NANOMITE_LEARN_TOPIC")
            } else {
                None
            })
            .or_else(|| stored.learning_topic.clone())
            .map(|topic| topic.trim().to_string())
            .filter(|topic| !topic.is_empty()),
            hacker,
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

    /// Stores an API key for one provider (preset id). An empty key removes it.
    /// The active preset's key is mirrored in `api_key` so the running client
    /// keeps working; other providers keep theirs for later switches.
    pub fn set_provider_key(&mut self, provider: &str, key: &str) {
        let id = provider.trim().to_ascii_lowercase();
        if id.is_empty() {
            return;
        }
        let key = key.trim();
        if key.is_empty() {
            self.provider_keys.remove(&id);
        } else {
            self.provider_keys.insert(id.clone(), key.to_string());
        }
        if id == self.preset {
            self.api_key = (!key.is_empty()).then(|| key.to_string());
        }
    }

    /// The saved key for one provider, if any.
    pub fn provider_key(&self, provider: &str) -> Option<String> {
        self.provider_keys
            .get(&provider.trim().to_ascii_lowercase())
            .cloned()
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
        // Key precedence: this provider's env var, the generic env var, the key
        // saved for this provider, then the legacy active key.
        self.api_key = preset
            .key_env
            .and_then(first_env_one)
            .or_else(|| first_env_one("NANOMITE_API_KEY"))
            .or_else(|| self.provider_key(preset.id));
        if let Some(key) = &self.api_key {
            self.provider_keys
                .insert(preset.id.to_string(), key.clone());
        }
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
    fn hacker_store_roundtrips_and_env_overrides() {
        let path =
            std::env::temp_dir().join(format!("nanomite-hacker-{}.toml", std::process::id()));
        fs::write(
            &path,
            "model = \"gpt-4o-mini\"\n[hacker]\nenabled = true\nstore = false\naudit = false\n",
        )
        .expect("write config");
        let cfg = Config::load_from(&path);
        assert!(cfg.hacker.enabled);
        assert!(!cfg.hacker.store, "the file value must be honoured");
        assert!(!cfg.hacker.audit);

        // Environment overrides the file.
        std::env::set_var("NANOMITE_CONFIG", &path);
        std::env::set_var("NANOMITE_HACKER_STORE", "1");
        assert!(
            Config::from_env().hacker.store,
            "NANOMITE_HACKER_STORE must win over the file"
        );
        std::env::remove_var("NANOMITE_HACKER_STORE");
        std::env::remove_var("NANOMITE_CONFIG");

        // No `[hacker]` section at all -> the store defaults on.
        let plain =
            std::env::temp_dir().join(format!("nanomite-hacker-none-{}.toml", std::process::id()));
        fs::write(&plain, "model = \"gpt-4o-mini\"\n").expect("write config");
        assert!(Config::load_from(&plain).hacker.store);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&plain);
    }

    #[test]
    fn learning_section_roundtrips_defaults_off_and_env_overrides() {
        let path = std::env::temp_dir().join(format!("nanomite-learn-{}.toml", std::process::id()));
        let mut cfg = Config::from_env();
        cfg.learning_daily = true;
        cfg.learning_topic = Some("agent evaluation".into());
        cfg.save_to(&path);

        let loaded = Config::load_from(&path);
        assert!(loaded.learning_daily);
        assert_eq!(loaded.learning_topic.as_deref(), Some("agent evaluation"));

        // No `[learn]` section -> daily learning stays off.
        let plain =
            std::env::temp_dir().join(format!("nanomite-learn-none-{}.toml", std::process::id()));
        fs::write(&plain, "model = \"gpt-4o-mini\"\n").expect("write config");
        let plain_cfg = Config::load_from(&plain);
        assert!(!plain_cfg.learning_daily);
        assert!(plain_cfg.learning_topic.is_none());

        // Environment overrides the file.
        std::env::set_var("NANOMITE_CONFIG", &path);
        std::env::set_var("NANOMITE_LEARN_DAILY", "off");
        assert!(!Config::load().learning_daily);
        std::env::remove_var("NANOMITE_LEARN_DAILY");
        std::env::remove_var("NANOMITE_CONFIG");

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&plain);
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

        let path =
            std::env::temp_dir().join(format!("nanomite-config-{}.toml", std::process::id()));
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
    fn multiple_provider_keys_roundtrip_and_migrate() {
        let mut cfg = Config::from_env();
        cfg.apply_preset("deepseek");
        cfg.set_provider_key("deepseek", "sk-deepseek");
        cfg.set_provider_key("openai", "sk-openai");
        cfg.set_provider_key("zhipu", "sk-zhipu");
        assert_eq!(cfg.api_key.as_deref(), Some("sk-deepseek"));

        let path = std::env::temp_dir().join(format!("nanomite-keys-{}.toml", std::process::id()));
        cfg.save_to(&path);
        let loaded = Config::load_from(&path);
        assert_eq!(
            loaded.provider_key("deepseek").as_deref(),
            Some("sk-deepseek")
        );
        assert_eq!(loaded.provider_key("openai").as_deref(), Some("sk-openai"));
        assert_eq!(loaded.provider_key("zhipu").as_deref(), Some("sk-zhipu"));

        // Switching providers restores that provider's saved key, unless an
        // environment variable overrides it (env wins by design).
        if first_env_one("NANOMITE_API_KEY").is_none() && first_env_one("ZHIPUAI_API_KEY").is_none()
        {
            let mut switched = loaded.clone();
            assert!(switched.apply_preset("zhipu"));
            assert_eq!(switched.api_key.as_deref(), Some("sk-zhipu"));
            assert_eq!(switched.base_url, "https://open.bigmodel.cn/api/paas/v4");
        }

        // A legacy file with a single top-level key migrates into the active
        // provider's per-provider entry.
        fs::write(
            &path,
            "provider = \"deepseek\"\napi_key = \"sk-legacy\"\nmodel = \"deepseek-chat\"\n",
        )
        .unwrap();
        let migrated = Config::load_from(&path);
        assert_eq!(migrated.api_key.as_deref(), Some("sk-legacy"));
        assert_eq!(
            migrated.provider_key("deepseek").as_deref(),
            Some("sk-legacy")
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn theme_roundtrips_and_unknown_labels_fall_back() {
        let mut cfg = Config::from_env();
        cfg.theme = "umbrella".into();
        let path = std::env::temp_dir().join(format!("nanomite-theme-{}.toml", std::process::id()));
        cfg.save_to(&path);
        let loaded = Config::load_from(&path);
        assert_eq!(loaded.theme, "umbrella");
        let _ = fs::remove_file(&path);

        // An unknown or hand-mangled theme label degrades to the default skin
        // instead of poisoning the config.
        fs::write(
            &path,
            "provider = \"openai\"\nbase_url = \"https://api.openai.com/v1\"\nmodel = \"gpt-4o-mini\"\nreasoning = \"low\"\ntheme = \"seventies-neon\"\n",
        )
        .expect("write config");
        let loaded = Config::load_from(&path);
        assert_eq!(
            loaded.theme,
            crate::ui::theme::ThemeId::Umbrella.label(),
            "unknown label must fall back to the default skin"
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn disabled_tools_roundtrip_env_precedence_and_unknown_names() {
        let path = std::env::temp_dir().join(format!("nanomite-tools-{}.toml", std::process::id()));
        // Trimmed + lowercased on parse; unknown names are tolerated.
        fs::write(
            &path,
            "provider = \"openai\"\nbase_url = \"https://api.openai.com/v1\"\nmodel = \"gpt-4o-mini\"\ndisabled_tools = \" Read_File , web_search ,bogus_tool\"\n",
        )
        .expect("write config");
        let cfg = Config::load_from(&path);
        assert_eq!(
            cfg.disabled_tools,
            vec!["read_file", "web_search", "bogus_tool"]
        );

        // Absent key = empty list.
        let plain =
            std::env::temp_dir().join(format!("nanomite-plain-{}.toml", std::process::id()));
        fs::write(&plain, "model = \"gpt-4o-mini\"\n").expect("write config");
        assert!(Config::load_from(&plain).disabled_tools.is_empty());

        // Environment overrides the file.
        std::env::set_var("NANOMITE_CONFIG", &path);
        std::env::set_var(
            "NANOMITE_DISABLED_TOOLS",
            "run_command, , SSH_RUN,run_command",
        );
        let env_cfg = Config::load();
        assert_eq!(env_cfg.disabled_tools, vec!["run_command", "ssh_run"]);
        std::env::remove_var("NANOMITE_DISABLED_TOOLS");
        std::env::remove_var("NANOMITE_CONFIG");

        // A saved config writes the list back out.
        let mut saved = Config::from_env();
        saved.disabled_tools = vec!["edit_file".into()];
        saved.store_path = Some(path.clone());
        saved.save_to(&path);
        assert_eq!(
            Config::load_from(&path).disabled_tools,
            vec!["edit_file"],
            "comma list must survive a save/load round trip"
        );

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&plain);
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

    /// The custom prompt file defaults to the config dir, is overridable by the
    /// environment, and round-trips through the persisted setup.
    #[test]
    fn prompt_file_default_env_precedence_and_persistence() {
        let cfg = Config::from_env();
        assert_eq!(
            cfg.prompt_file,
            Some(crate::config::default_prompt_path()),
            "the default lives next to config.toml"
        );

        let path =
            std::env::temp_dir().join(format!("nanomite-prompt-{}.toml", std::process::id()));
        std::fs::write(&path, "prompt_file = \"/tmp/custom-prompt.md\"\n").unwrap();
        assert_eq!(
            Config::load_from(&path).prompt_file,
            Some(std::path::PathBuf::from("/tmp/custom-prompt.md"))
        );

        // The environment wins over the persisted value, like every other field.
        std::env::set_var("NANOMITE_PROMPT_FILE", "/tmp/env-prompt.md");
        assert_eq!(
            Config::from_env().prompt_file,
            Some(std::path::PathBuf::from("/tmp/env-prompt.md"))
        );
        std::env::remove_var("NANOMITE_PROMPT_FILE");

        // Saving keeps it, and the path survives a reload.
        let mut saved = Config::from_env();
        saved.prompt_file = Some(std::path::PathBuf::from("/tmp/saved-prompt.md"));
        saved.store_path = Some(path.clone());
        saved.save();
        assert_eq!(
            Config::load_from(&path).prompt_file,
            Some(std::path::PathBuf::from("/tmp/saved-prompt.md"))
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn model_context_limit_returns_known_windows() {
        // OpenAI
        assert_eq!(model_context_limit("gpt-5.4"), 1_050_000);
        assert_eq!(model_context_limit("gpt-5.5"), 1_050_000);
        assert_eq!(model_context_limit("gpt-5.6"), 1_050_000);
        assert_eq!(model_context_limit("gpt-6"), 1_050_000);
        assert_eq!(model_context_limit("gpt-5"), 400_000);
        assert_eq!(model_context_limit("gpt-5.1"), 400_000);
        assert_eq!(model_context_limit("gpt-5.2"), 400_000);
        assert_eq!(model_context_limit("gpt-5.3"), 400_000);
        assert_eq!(model_context_limit("gpt-4.1"), 1_047_576);
        assert_eq!(model_context_limit("gpt-4o"), 128_000);
        assert_eq!(model_context_limit("gpt-4o-mini"), 128_000);
        assert_eq!(model_context_limit("gpt-4"), 128_000);
        assert_eq!(model_context_limit("o1-preview"), 200_000);
        assert_eq!(model_context_limit("o3-mini"), 200_000);
        assert_eq!(model_context_limit("gpt-3.5-turbo"), 16_384);

        // Anthropic
        assert_eq!(model_context_limit("claude-5"), 1_000_000);
        assert_eq!(model_context_limit("claude-4-opus"), 1_000_000);
        assert_eq!(model_context_limit("claude-4-sonnet"), 1_000_000);
        assert_eq!(model_context_limit("claude-4-haiku"), 1_000_000);
        assert_eq!(model_context_limit("claude-fable-5"), 1_000_000);
        assert_eq!(model_context_limit("claude-mythos-5"), 1_000_000);
        assert_eq!(model_context_limit("claude-3.5-sonnet"), 200_000);
        assert_eq!(model_context_limit("claude-3.7-sonnet"), 200_000);
        assert_eq!(model_context_limit("claude-3-haiku"), 200_000);

        // Google
        assert_eq!(model_context_limit("gemini-4-argon"), 1_048_576);
        assert_eq!(model_context_limit("gemini-3-pro"), 1_048_576);
        assert_eq!(model_context_limit("gemini-2.5-pro"), 1_000_000);
        assert_eq!(model_context_limit("gemini-2.0-flash"), 2_000_000);
        assert_eq!(model_context_limit("gemini-1.5-pro"), 2_000_000);

        // xAI
        assert_eq!(model_context_limit("grok-4.7"), 1_000_000);
        assert_eq!(model_context_limit("grok-4.6"), 1_000_000);
        assert_eq!(model_context_limit("grok-4.5"), 1_000_000);
        assert_eq!(model_context_limit("grok-4.3"), 1_000_000);
        assert_eq!(model_context_limit("grok-4.2"), 1_000_000);
        assert_eq!(model_context_limit("grok-4"), 500_000);
        assert_eq!(model_context_limit("grok-3"), 128_000);

        // NVIDIA Nemotron
        assert_eq!(
            model_context_limit("nvidia/nemotron-3-ultra-550b-a55b"),
            1_000_000
        );
        assert_eq!(model_context_limit("nemotron-3-ultra"), 1_000_000);

        // DeepSeek
        assert_eq!(model_context_limit("deepseek-v4-pro"), 1_000_000);
        assert_eq!(model_context_limit("deepseek-r1"), 128_000);
        assert_eq!(model_context_limit("deepseek-v3"), 128_000);

        // Llama
        assert_eq!(model_context_limit("llama-4-scout"), 10_000_000);
        assert_eq!(model_context_limit("llama-4-maverick"), 1_048_576);
        assert_eq!(model_context_limit("llama-3.3-70b"), 128_000);
        assert_eq!(model_context_limit("llama-3.2-3b"), 128_000);
        assert_eq!(model_context_limit("llama-3.1-8b"), 128_000);
        assert_eq!(model_context_limit("llama-3-8b"), 8_192);

        // Mistral
        assert_eq!(model_context_limit("mistral-large-4"), 1_000_000);
        assert_eq!(model_context_limit("mistral-large"), 256_000);
        assert_eq!(model_context_limit("mistral-medium"), 256_000);
        assert_eq!(model_context_limit("mistral-small"), 256_000);

        // Qwen
        assert_eq!(model_context_limit("qwen3.8-max"), 1_000_000);
        assert_eq!(model_context_limit("qwen3.7-max"), 1_000_000);
        assert_eq!(model_context_limit("qwen3-max"), 262_144);
        assert_eq!(model_context_limit("qwen3-coder"), 262_144);
        assert_eq!(model_context_limit("qwen3.6"), 262_144);
        assert_eq!(model_context_limit("qwen3.5"), 262_144);
        assert_eq!(model_context_limit("qwen3"), 262_144);
        assert_eq!(model_context_limit("qwen-2.5-72b"), 128_000);

        // Test models
        assert_eq!(model_context_limit("scripted-model"), 200_000);

        // Generic pattern matching
        assert_eq!(model_context_limit("my-model-32k"), 32_768);
        assert_eq!(model_context_limit("my-model-128k"), 128_000);
        assert_eq!(model_context_limit("my-model-200k"), 200_000);
        assert_eq!(model_context_limit("my-model-256k"), 262_144);
        assert_eq!(model_context_limit("my-model-1m"), 1_000_000);
        assert_eq!(model_context_limit("my-model-2m"), 2_000_000);
        assert_eq!(model_context_limit("my-model-10m"), 10_000_000);

        // Unknown model gets default
        assert_eq!(model_context_limit("unknown-model-xyz"), 8_192);
    }
}

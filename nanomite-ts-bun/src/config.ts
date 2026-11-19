/**
 * Configuration module - ported from src/config.rs
 * 
 * Config precedence: built-in defaults < file (~/.config/nanomite/config.toml)
 * < NANOMITE_* environment variables
 */

import * as fs from "fs";
import * as path from "path";
import * as os from "os";

export enum Reasoning {
  Minimal = "minimal",
  Low = "low",
  Medium = "medium",
  High = "high",
}

export interface ProviderPreset {
  name: string;
  base_url: string;
  api_key_env: string;
  model: string;
  reasoning?: Reasoning;
  context_limit?: number;
}

export const DEFAULT_PRESETS: ProviderPreset[] = [
  // OpenCode
  {
    name: "opencode",
    base_url: "https://api.opencode.com/v1",
    api_key_env: "OPENCODE_API_KEY",
    model: "opencode-1.0",
  },
  // OpenAI
  {
    name: "openai",
    base_url: "https://api.openai.com/v1",
    api_key_env: "OPENAI_API_KEY",
    model: "gpt-4o-mini",
  },
  // Anthropic
  {
    name: "anthropic",
    base_url: "https://api.anthropic.com",
    api_key_env: "ANTHROPIC_API_KEY",
    model: "claude-3-haiku-20240307",
  },
  // Gemini
  {
    name: "gemini",
    base_url: "https://generativelanguage.googleapis.com/v1beta",
    api_key_env: "GEMINI_API_KEY",
    model: "gemini-1.5-flash",
  },
  // xAI
  {
    name: "xai",
    base_url: "https://api.x.ai/v1",
    api_key_env: "XAI_API_KEY",
    model: "grok-beta",
  },
  // Groq
  {
    name: "groq",
    base_url: "https://api.groq.com/openai/v1",
    api_key_env: "GROQ_API_KEY",
    model: "mixtral-8x7b-32768",
  },
  // Mistral
  {
    name: "mistral",
    base_url: "https://api.mistral.ai/v1",
    api_key_env: "MISTRAL_API_KEY",
    model: "mistral-large-2402",
  },
  // DeepSeek
  {
    name: "deepseek",
    base_url: "https://api.deepseek.com/v1",
    api_key_env: "DEEPSEEK_API_KEY",
    model: "deepseek-coder-v2",
  },
  // OpenRouter
  {
    name: "openrouter",
    base_url: "https://openrouter.ai/api/v1",
    api_key_env: "OPENROUTER_API_KEY",
    model: "anthropic/claude-3-haiku",
  },
  // Together
  {
    name: "together",
    base_url: "https://api.together.xyz/v1",
    api_key_env: "TOGETHER_API_KEY",
    model: "meta-llama/Llama-3.1-8B-Instruct",
  },
  // Fireworks
  {
    name: "fireworks",
    base_url: "https://api.fireworks.ai/v1",
    api_key_env: "FIREWORKS_API_KEY",
    model: "meta-llama/Meta-Llama-3.1-8B-Instruct",
  },
  // Cerebras
  {
    name: "cerebras",
    base_url: "https://api.cerebras.net/v1",
    api_key_env: "CEREBRAS_API_KEY",
    model: "cerebras-glm-1.5b",
  },
  // Perplexity
  {
    name: "perplexity",
    base_url: "https://api.perplexity.ai/v1",
    api_key_env: "PERPLEXITY_API_KEY",
    model: "perplexity-small",
  },
  // GitHub Models
  {
    name: "github_models",
    base_url: "https://models.github.inference.azure.com",
    api_key_env: "GITHUB_MODELS_KEY",
    model: "gpt-4o-mini",
  },
  // NVIDIA NIM
  {
    name: "nim",
    base_url: "https://integrate.api.nvidia.com/v1",
    api_key_env: "NVIDIA_API_KEY",
    model: "nemotron-beta",
  },
  // DashScope
  {
    name: "dashscope",
    base_url: "https://dashscope.aliyuncs.com/v1",
    api_key_env: "DASHSCOPE_API_KEY",
    model: "qwen-max",
  },
  // Moonshot
  {
    name: "moonshot",
    base_url: "https://api.moonshot.ai/v1",
    api_key_env: "MOONSHOT_API_KEY",
    model: "moonshot-v1-8k",
  },
  // Zhipu
  {
    name: "zhipu",
    base_url: "https://open.bmfs.club/v1",
    api_key_env: "ZHIPU_API_KEY",
    model: "glm-4",
  },
  // Ollama (local)
  {
    name: "ollama",
    base_url: "http://127.0.0.1:11434",
    api_key_env: "OLLAMA_API_KEY",
    model: "llama3",
  },
  // LM Studio
  {
    name: "lm_studio",
    base_url: "http://127.0.0.1:1234/v1",
    api_key_env: "LM_STUDIO_API_KEY",
    model: "lmstudio-model",
  },
  // vLLM
  {
    name: "vllm",
    base_url: "http://127.0.0.1:8000/v1",
    api_key_env: "VLLM_API_KEY",
    model: "vllm-model",
  },
  // llama.cpp
  {
    name: "llama_cpp",
    base_url: "http://127.0.0.1:8080/v1",
    api_key_env: "LLAMA_CPP_API_KEY",
    model: "llama-2-7b-chat",
  },
  // Brent 1.0
  {
    name: "brent_1",
    base_url: "http://127.0.0.1:8080/v1",
    api_key_env: "BRENT_API_KEY",
    model: "brent-1.0-4b",
  },
  // Scripted test preset
  {
    name: "scripted",
    base_url: "",
    api_key_env: "",
    model: "scripted",
  },
]

export interface Config {
  provider: string;
  base_url: string;
  api_key: string;
  model: string;
  reasoning: Reasoning;
  step_timeout_ms: number;
  max_steps: number;
  max_tool_calls: number;
  max_message_chars: number;
  offline: boolean;
  disabled_tools: string[];
  theme: string;
  hacker_enabled: boolean;
  hacker_scope: string;
  herdr_enabled: boolean;
  learn_daily: boolean;
}

// Config file location
const CONFIG_PATH = path.join(os.homedir(), ".config", "nanomite", "config.toml");
const DATA_DIR = path.join(os.homedir(), ".local", "share", "nanomite");

// Parse TOML config (simple parser for our format)
function parseTomlSimple(str: string): Record<string, string> {
  const result: Record<string, string> = {};
  for (const line of str.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#") || trimmed.startsWith("//")) continue;
    const eqIdx = trimmed.indexOf("=");
    if (eqIdx === -1) continue;
    const key = trimmed.slice(0, eqIdx).trim();
    const value = trimmed.slice(eqIdx + 1).trim();
    result[key] = value;
  }
  return result;
}

// Load config from file with env overrides
export function loadConfig(): Config {
  // Built-in defaults
  let cfg: Record<string, string> = {
    provider: "brent",
    base_url: "http://127.0.0.1:8080/v1",
    api_key: "",
    model: "full path to artifacts/final-4bit since mlx_lm uses the path as model ID",
    reasoning: Reasoning.Minimal,
    step_timeout_ms: "120000",
    max_steps: "usize::MAX",
    max_tool_calls: "usize::MAX",
    max_message_chars: "200000",
    offline: "false",
    disabled_tools: "[]",
    theme: "mono",
    hacker_enabled: "false",
    hacker_scope: "",
    herdr_enabled: "false",
    learn_daily: "false",
  };

  // Try to load from file
  if (fs.existsSync(CONFIG_PATH)) {
    try {
      const fileCfg = parseTomlSimple(fs.readFileSync(CONFIG_PATH, "utf-8"));
      // Apply file config (file < env override)
      for (const [key, value] of Object.entries(fileCfg)) {
        const lowerKey = key.toLowerCase();
        if (cfg.hasOwnProperty(lowerKey)) {
          cfg[lowerKey] = value;
        }
      }
    } catch {
      // If file is unreadable, continue with defaults
    }
  }

  // Apply environment overrides (highest precedence)
  const envOverrides: Record<string, string> = {
    NANOMITE_PROVIDER: "provider",
    NANOMITE_BASE_URL: "base_url",
    NANOMITE_API_KEY: "api_key",
    NANOMITE_MODEL: "model",
    NANOMITE_REASONING: "reasoning",
    NANOMITE_STEP_TIMEOUT_MS: "step_timeout_ms",
    NANOMITE_MAX_STEPS: "max_steps",
    NANOMITE_MAX_TOOL_CALLS: "max_tool_calls",
    NANOMITE_MAX_MESSAGE_CHARS: "max_message_chars",
    NANOMITE_OFFLINE: "offline",
    NANOMITE_THEME: "theme",
    NANOMITE_HACKER_ENABLED: "hacker_enabled",
    NANOMITE_HACKER_SCOPE: "hacker_scope",
    NANOMITE_HERDR_ENABLED: "herdr_enabled",
    NANOMITE_LEARN_DAILY: "learn_daily",
  };

  for (const [envVar, cfgKey] of Object.entries(envOverrides)) {
    if (process.env[envVar] !== undefined) {
      cfg[cfgKey] = process.env[envVar]!;
    }
  }

  // Parse disabled_tools from comma-separated list
  const disabled = cfg.disabled_tools.split(",")
    .map((s: string) => s.trim())
    .filter((s: string) => s.length > 0);
  cfg.disabled_tools = disabled;

  // Parse numbers
  cfg.step_timeout_ms = parseInt(cfg.step_timeout_ms) || 120000;
  cfg.max_steps = cfg.max_steps === "usize::MAX" ? Number.MAX_SAFE_INTEGER : parseInt(cfg.max_steps) || Number.MAX_SAFE_INTEGER;
  cfg.max_tool_calls = cfg.max_tool_calls === "usize::MAX" ? Number.MAX_SAFE_INTEGER : parseInt(cfg.max_tool_calls) || Number.MAX_SAFE_INTEGER;
  cfg.max_message_chars = cfg.max_message_chars === "200000" ? 200000 : parseInt(cfg.max_message_chars) || 200000;

  // Parse boolean strings
  cfg.offline = cfg.offline.toLowerCase() === "true";
  cfg.hacker_enabled = cfg.hacker_enabled.toLowerCase() === "true";
  cfg.herdr_enabled = cfg.herdr_enabled.toLowerCase() === "true";
  cfg.learn_daily = cfg.learn_daily.toLowerCase() === "true";

  // Select preset based on provider
  const presetName = cfg.provider.toLowerCase();
  const preset = DEFAULT_PRESETS.find(p => p.name.toLowerCase() === presetName) || DEFAULT_PRESETS[0];
  
  // Merge preset into config
  const mergedConfig: Config = {
    provider: preset.name,
    base_url: cfg.base_url || preset.base_url,
    api_key: cfg.api_key,
    model: cfg.model || preset.model,
    reasoning: cfg.reasoning !== undefined ? (cfg.reasoning as Reasoning) : preset.reasoning || Reasoning.Minimal,
    step_timeout_ms: cfg.step_timeout_ms,
    max_steps: cfg.max_steps,
    max_tool_calls: cfg.max_tool_calls,
    max_message_chars: cfg.max_message_chars,
    offline: cfg.offline,
    disabled_tools: cfg.disabled_tools,
    theme: cfg.theme || "mono",
    hacker_enabled: cfg.hacker_enabled,
    hacker_scope: cfg.hacker_scope,
    herdr_enabled: cfg.herdr_enabled,
    learn_daily: cfg.learn_daily,
  };

  // Ensure base_url ends properly
  if (!mergedConfig.base_url.endsWith("/")) {
    mergedConfig.base_url += "/";
  }

  return mergedConfig;
}

// Save config to file (atomic write)
export function saveConfig(cfg: Config): void {
  const dir = path.dirname(CONFIG_PATH);
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }

  // Write to temp file first, then rename for atomicity
  const tempPath = CONFIG_PATH + ".tmp";
  const toml = `
[nanomite]
provider = "${cfg.provider}"
base_url = "${cfg.base_url}"
api_key = "${cfg.api_key}"
model = "${cfg.model}"
reasoning = "${cfg.reasoning}"
step_timeout_ms = ${cfg.step_timeout_ms}
max_steps = ${cfg.max_steps}
max_tool_calls = ${cfg.max_tool_calls}
max_message_chars = ${cfg.max_message_chars}
offline = ${cfg.offline}
disabled_tools = [${cfg.disabled_tools.join(", ")}]"
theme = "${cfg.theme}"
hacker_enabled = ${cfg.hacker_enabled}
hacker_scope = "${cfg.hacker_scope}"
herdr_enabled = ${cfg.herdr_enabled}
learn_daily = ${cfg.learn_daily}
`;

  fs.writeFileSync(tempPath, toml);
  fs.renameSync(tempPath, CONFIG_PATH);

  // Also ensure data directory exists
  if (!fs.existsSync(DATA_DIR)) {
    fs.mkdirSync(DATA_DIR, { recursive: true });
  }
}

// Export preset lookup
export function findPreset(name: string): ProviderPreset | undefined {
  return DEFAULT_PRESETS.find(p => p.name.toLowerCase() === name.toLowerCase());
}

// Export all presets
export { DEFAULT_PRESETS };
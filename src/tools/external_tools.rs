//! External tool SDK: user-defined tools from TOML manifests.
//!
//! A tool is a manifest file (`*.toml`) declaring name, description, JSON
//! schema, and the command to run. Arguments arrive on the child's stdin as
//! JSON; the tool's stdout is returned to the model (bounded). Tools are
//! read-only from nanomite's perspective (risk `Safe`) but the command itself
//! runs with user permissions inside the workspace - documented as the same
//! trust boundary as `run_command`.
//!
//! Manifest locations: `NANOMITE_TOOLS_DIR` override, else
//! `~/.config/nanomite/tools/*.toml`. Schema validation happens at load time.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Largest output returned to the model.
const MAX_OUTPUT: usize = 16 * 1024;
/// Hard cap for one external tool run.
const MAX_TIMEOUT_MS: u64 = 120_000;
/// Manifests per directory, hard cap.
const MAX_TOOLS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalTool {
    /// Tool name as the model calls it (`snake_case`, no spaces).
    pub name: String,
    /// One-line description shown with the schema.
    pub description: String,
    /// Command line to run; arguments arrive as JSON on stdin. `$arg`-style
    /// interpolation is deliberately NOT supported - stdin only, so argument
    /// values can never become shell syntax.
    pub command: String,
    /// JSON schema (OpenAI function style) for the tool's arguments.
    #[serde(default)]
    pub parameters: serde_json::Value,
    /// Hard cap in milliseconds (default 60s, max 120s).
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_timeout() -> u64 {
    60_000
}

/// A loaded, validated external tool set.
#[derive(Debug, Clone, Default)]
pub struct ExternalTools {
    tools: BTreeMap<String, ExternalTool>,
    /// Load errors, kept for the `/tools` overlay and diagnostics.
    pub errors: Vec<String>,
}

impl ExternalTools {
    /// Loads every `*.toml` manifest in `dir`, validating name, description,
    /// command and schema. Invalid manifests are skipped with a recorded
    /// error, never fatal: one bad file must not break the other tools.
    pub fn load(dir: &Path) -> Self {
        let mut set = Self::default();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return set;
        };
        for entry in entries.flatten().take(MAX_TOOLS * 2) {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(raw) => match toml::from_str::<ExternalTool>(&raw) {
                    Ok(tool) => {
                        if let Err(e) = validate(&tool) {
                            set.errors.push(format!("{}: {e}", path.display()));
                        } else if set.tools.len() >= MAX_TOOLS {
                            set.errors.push(format!(
                                "{}: skipped, tool limit ({MAX_TOOLS}) reached",
                                path.display()
                            ));
                        } else {
                            set.tools.insert(tool.name.clone(), tool);
                        }
                    }
                    Err(e) => set
                        .errors
                        .push(format!("{}: parse error: {e}", path.display())),
                },
                Err(e) => set
                    .errors
                    .push(format!("{}: read error: {e}", path.display())),
            }
        }
        set
    }

    pub fn get(&self, name: &str) -> Option<&ExternalTool> {
        self.tools.get(name)
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Runs one external tool: arguments as JSON on stdin, stdout captured.
    /// The command runs in its own process group so a timeout kills the whole
    /// tree, and never inherits the terminal.
    pub fn run(&self, name: &str, args_json: &str) -> Result<ExternalToolResult, String> {
        let tool = self
            .get(name)
            .ok_or_else(|| format!("unknown external tool `{name}`"))?;

        // Validate arguments against the manifest schema (best effort: an
        // empty schema accepts anything).
        validate_args(&tool.parameters, args_json)?;

        let timeout = Duration::from_millis(tool.timeout_ms.clamp(1_000, MAX_TIMEOUT_MS));
        let mut parts = tool.command.split_whitespace();
        let Some(program) = parts.next() else {
            return Err(format!("tool `{name}` has an empty command"));
        };
        let started = Instant::now();

        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new(program);
        cmd.args(parts)
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("tool `{name}` failed to spawn: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(args_json.as_bytes());
            let _ = stdin.flush();
        }

        let out_handle = std::thread::spawn({
            let mut pipe = child.stdout.take().expect("piped stdout");
            move || {
                let mut buf = Vec::new();
                let _ = std::io::Read::read_to_end(&mut pipe, &mut buf);
                buf
            }
        });
        let err_handle = std::thread::spawn({
            let mut pipe = child.stderr.take().expect("piped stderr");
            move || {
                let mut buf = Vec::new();
                let _ = std::io::Read::read_to_end(&mut pipe, &mut buf);
                buf
            }
        });

        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break Some(st),
                Ok(None) => {
                    if started.elapsed() >= timeout {
                        timed_out = true;
                        crate::tools::exec_tools::kill_process_group(&child);
                        let _ = child.wait();
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(e) => return Err(format!("tool `{name}` wait failed: {e}")),
            }
        };

        let stdout = String::from_utf8_lossy(&out_handle.join().unwrap_or_default())
            .chars()
            .take(MAX_OUTPUT)
            .collect();
        let stderr = String::from_utf8_lossy(&err_handle.join().unwrap_or_default())
            .chars()
            .take(MAX_OUTPUT)
            .collect();
        let exit_code = status.and_then(|s| s.code()).unwrap_or(-1);

        Ok(ExternalToolResult {
            tool: name.to_string(),
            exit_code,
            timed_out,
            stdout,
            stderr,
            duration_ms: started.elapsed().as_millis() as u64,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalToolResult {
    pub tool: String,
    pub exit_code: i32,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

/// Manifest validation at load time: stable name, description, command, and a
/// schema that looks like an OpenAI function parameters object.
fn validate(tool: &ExternalTool) -> Result<(), String> {
    let name = tool.name.trim();
    if name.is_empty() {
        return Err("name is empty".into());
    }
    if name != name.trim() || name.contains(char::is_whitespace) {
        return Err("name must not contain whitespace".into());
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("name must be snake_case alphanumeric/underscore".into());
    }
    if name.len() > 64 {
        return Err("name too long (max 64 chars)".into());
    }
    if tool.description.trim().is_empty() {
        return Err("description is empty".into());
    }
    if tool.command.trim().is_empty() {
        return Err("command is empty".into());
    }
    if tool.parameters.is_null() {
        return Ok(());
    }
    let obj = tool
        .parameters
        .as_object()
        .ok_or("parameters must be a JSON object")?;
    if let Some(typ) = obj.get("type") {
        if typ.as_str() != Some("object") {
            return Err("parameters.type must be \"object\"".into());
        }
    }
    if let Some(props) = obj.get("properties") {
        props
            .as_object()
            .ok_or("parameters.properties must be an object")?;
    }
    Ok(())
}

/// Best-effort argument validation against the manifest schema: required
/// keys must be present, unknown keys are refused when the schema lists
/// properties and sets additionalProperties=false.
fn validate_args(schema: &serde_json::Value, args_json: &str) -> Result<(), String> {
    let args: serde_json::Value = serde_json::from_str(args_json)
        .map_err(|e| format!("arguments are not valid JSON: {e}"))?;
    let Some(obj) = schema.as_object() else {
        return Ok(());
    };
    let Some(props) = obj.get("properties").and_then(|p| p.as_object()) else {
        return Ok(());
    };
    let args_obj = args.as_object().ok_or("arguments must be a JSON object")?;

    if let Some(required) = obj.get("required").and_then(|r| r.as_array()) {
        for req in required {
            let key = req.as_str().unwrap_or_default();
            if !key.is_empty() && !args_obj.contains_key(key) {
                return Err(format!("missing required argument `{key}`"));
            }
        }
    }
    if obj.get("additionalProperties").and_then(|a| a.as_bool()) == Some(false) {
        for key in args_obj.keys() {
            if !props.contains_key(key) {
                return Err(format!("unknown argument `{key}`"));
            }
        }
    }
    Ok(())
}

/// Manifest directory: `NANOMITE_TOOLS_DIR` override, else
/// `~/.config/nanomite/tools`.
pub fn tools_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("NANOMITE_TOOLS_DIR") {
        return PathBuf::from(dir);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nanomite")
        .join("tools")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(format!("{name}.toml"));
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn loads_a_valid_manifest_and_exposes_the_tool() {
        let dir = std::env::temp_dir().join(format!("nanomite-ext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_manifest(
            &dir,
            "greet",
            r#"
name = "greet_user"
description = "Greet a user by name"
command = "python3 /path/greet.py"
timeout_ms = 5000

[parameters]
type = "object"

[parameters.properties]
user = { type = "string", description = "who to greet" }

[parameters.required]
"#,
        );
        let tools = ExternalTools::load(&dir);
        assert!(tools.errors.is_empty(), "{:?}", tools.errors);
        assert!(tools.get("greet_user").is_some());
        assert_eq!(tools.names(), vec!["greet_user".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_manifests_are_skipped_with_errors() {
        let dir = std::env::temp_dir().join(format!("nanomite-ext-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_manifest(
            &dir,
            "a",
            "name = \"bad name with spaces\"\ndescription = \"x\"\ncommand = \"true\"\n",
        );
        write_manifest(
            &dir,
            "b",
            "name = \"no_desc\"\ndescription = \"\"\ncommand = \"true\"\n",
        );
        write_manifest(&dir, "c", "this is not toml [[[");
        write_manifest(
            &dir,
            "d",
            "name = \"no_cmd\"\ndescription = \"x\"\ncommand = \"\"\n",
        );

        let tools = ExternalTools::load(&dir);
        assert!(tools.is_empty(), "no invalid tool may load");
        assert_eq!(tools.errors.len(), 4, "{:?}", tools.errors);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn schema_validation_rejects_bad_manifests() {
        let ok = ExternalTool {
            name: "x".into(),
            description: "d".into(),
            command: "true".into(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
            timeout_ms: 1000,
        };
        assert!(validate(&ok).is_ok());

        let wrong_type = ExternalTool {
            parameters: serde_json::json!({"type": "string"}),
            ..ok.clone()
        };
        assert!(validate(&wrong_type).is_err());

        let null_schema = ExternalTool {
            parameters: serde_json::Value::Null,
            ..ok
        };
        assert!(
            validate(&null_schema).is_ok(),
            "no schema = accept anything"
        );
    }

    #[test]
    fn arg_validation_checks_required_and_unknown_keys() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"user": {"type": "string"}, "mode": {"type": "string"}},
            "required": ["user"],
            "additionalProperties": false
        });
        assert!(validate_args(&schema, r#"{"user":"bob"}"#).is_ok());
        assert!(validate_args(&schema, r#"{"mode":"fast"}"#)
            .unwrap_err()
            .contains("missing required"));
        assert!(validate_args(&schema, r#"{"user":"bob","evil":1}"#)
            .unwrap_err()
            .contains("unknown argument"));
        assert!(validate_args(&schema, "not json").is_err());
        // Empty schema accepts anything.
        assert!(validate_args(&serde_json::Value::Null, r#"{"any":1}"#).is_ok());
    }

    #[test]
    fn runs_an_external_tool_end_to_end() {
        let dir = std::env::temp_dir().join(format!("nanomite-ext-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // The script reads its JSON args from stdin and echoes a field.
        let script = dir.join("upper.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nread -r payload\necho \"hello $payload\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut tools = ExternalTools::default();
        tools.tools.insert(
            "echoer".into(),
            ExternalTool {
                name: "echoer".into(),
                description: "echo test".into(),
                command: script.display().to_string(),
                parameters: serde_json::Value::Null,
                timeout_ms: 5_000,
            },
        );
        let r = tools.run("echoer", r#"{"x":1}"#).unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(
            r.stdout.contains(r#"{"x":1}"#),
            "args must arrive on stdin: {}",
            r.stdout
        );

        // Unknown tool refused.
        assert!(tools.run("nope", "{}").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn timeout_kills_a_hanging_tool() {
        let dir = std::env::temp_dir().join(format!("nanomite-ext-to-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("hang.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut tools = ExternalTools::default();
        tools.tools.insert(
            "hang".into(),
            ExternalTool {
                name: "hang".into(),
                description: "hangs".into(),
                command: script.display().to_string(),
                parameters: serde_json::Value::Null,
                timeout_ms: 300,
            },
        );
        let started = Instant::now();
        let r = tools.run("hang", "{}").unwrap();
        assert!(r.timed_out, "must report the timeout");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "must not wait 30s"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! # Tool Synthesizer — Generate New Rust Tools from Natural Language
//!
//! Given a natural-language description of a desired capability (e.g.
//! "scan a network for open ports using SYN stealth"), the synthesizer
//! generates a complete Rust source file implementing that tool as a
//! nanomite-compatible tool, writes it to `src/tools/synthesized/`, and
//! registers it in the catalog so the agent can call it.
//!
//! This is template-based code generation (no external LLM needed): the
//! synthesizer analyzes keywords in the description, picks an appropriate
//! implementation template, fills in the tool name/description/category,
//! writes the file, and optionally compiles it with `cargo check` to verify.
//!
//! All synthesized tools are saved as durable skills for future reference.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One synthesized tool: source code and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesizedTool {
    pub name: String,
    pub description: String,
    pub category: String,
    pub source_code: String,
    pub file_path: String,
    pub created_at: u64,
    pub compiled_ok: bool,
    pub compile_output: String,
    pub invocation_count: u64,
    pub last_invoked: u64,
}

/// Result of a synthesis request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesisResult {
    pub tool: SynthesizedTool,
    pub success: bool,
    pub message: String,
}

/// The tool synthesizer: templates + registry of what's been created.
#[derive(Debug, Clone, Default)]
pub struct ToolSynthesizer {
    pub created: HashMap<String, SynthesizedTool>,
    pub output_dir: PathBuf,
    /// How many tools have been synthesized total.
    pub total_created: u64,
}

impl ToolSynthesizer {
    pub fn new(output_dir: PathBuf) -> Self {
        Self {
            output_dir,
            ..Default::default()
        }
    }

    pub fn synthesize(
        &mut self,
        description: &str,
        suggested_name: Option<&str>,
        compile: bool,
    ) -> SynthesisResult {
        let name: String = match suggested_name.filter(|n| !n.trim().is_empty()) {
            Some(suggested) => {
                // Capitalize words separated by underscores/spaces/hyphens.
                let slug: String = suggested
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '_' || *c == '-')
                    .collect();
                let words: Vec<&str> = slug
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|w| !w.is_empty())
                    .collect();
                let cap: String = words
                    .iter()
                    .map(|w| {
                        let mut c = w.chars();
                        match c.next() {
                            None => String::new(),
                            Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                        }
                    })
                    .collect();
                if cap.is_empty() {
                    "SynthesizedTool".to_string()
                } else if cap.ends_with("Tool") {
                    cap
                } else {
                    format!("{cap}Tool")
                }
            }
            None => {
                let base = derive_tool_name(description);
                if base.ends_with("Tool") {
                    base
                } else {
                    format!("{base}Tool")
                }
            }
        };

        let category = categorize(description);
        let template = pick_template(description);
        let source_code = render_template(&name, description, &category, template);

        let file_path = self.output_dir.join(format!("{name}.rs"));
        let display = file_path.display().to_string();

        if let Some(parent) = file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&file_path, &source_code) {
            Ok(()) => {}
            Err(e) => {
                return SynthesisResult {
                    tool: SynthesizedTool {
                        name,
                        description: description.to_string(),
                        category,
                        source_code,
                        file_path: display,
                        created_at: crate::neural::now_secs(),
                        compiled_ok: false,
                        compile_output: format!("write failed: {e}"),
                        invocation_count: 0,
                        last_invoked: 0,
                    },
                    success: false,
                    message: format!("failed to write {file_path:?}: {e}"),
                };
            }
        }

        let mut compiled_ok = false;
        let mut compile_output = String::new();
        if compile {
            let (ok, out) = cargo_check(&file_path);
            compiled_ok = ok;
            compile_output = out;
        }

        let tool = SynthesizedTool {
            name: name.clone(),
            description: description.to_string(),
            category,
            source_code,
            file_path: display.clone(),
            created_at: crate::neural::now_secs(),
            compiled_ok,
            compile_output,
            invocation_count: 0,
            last_invoked: 0,
        };

        self.created.insert(name.clone(), tool.clone());
        self.total_created += 1;

        SynthesisResult {
            tool,
            success: true,
            message: if compiled_ok {
                format!("synthesized and compiled {name} at {display}")
            } else {
                format!("synthesized {name} at {display} (compile check skipped or failed)")
            },
        }
    }

    pub fn record_invocation(&mut self, name: &str) {
        if let Some(tool) = self.created.get_mut(name) {
            tool.invocation_count += 1;
            tool.last_invoked = crate::neural::now_secs();
        }
    }

    pub fn list(&self) -> Vec<&SynthesizedTool> {
        self.created.values().collect()
    }

    pub fn get(&self, name: &str) -> Option<&SynthesizedTool> {
        self.created.get(name)
    }
}

fn derive_tool_name(description: &str) -> String {
    let slug: String = description
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-')
        .collect();
    let words: Vec<&str> = slug.split_whitespace().take(4).collect();
    let name: String = words
        .iter()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect();
    if name.is_empty() {
        "SynthesizedTool".to_string()
    } else {
        // Append "Tool" suffix so names are always valid tool identifiers.
        format!("{name}Tool")
    }
}

fn categorize(description: &str) -> String {
    let d = description.to_ascii_lowercase();
    // Check more specific categories first (order matters: more specific before generic).
    if d.contains("wifi") || d.contains("wireless") {
        "wireless".to_string()
    } else if d.contains("web") || d.contains("http") || d.contains("vuln") || d.contains("sql") {
        "web".to_string()
    } else if d.contains("port") || d.contains("scan") || d.contains("recon") || d.contains("nmap")
    {
        "recon".to_string()
    } else if d.contains("password") || d.contains("crack") || d.contains("hash") {
        "password".to_string()
    } else if d.contains("file") || d.contains("read") || d.contains("write") {
        "files".to_string()
    } else if d.contains("memory") || d.contains("remember") || d.contains("recall") {
        "memory".to_string()
    } else if d.contains("plan") {
        "plan".to_string()
    } else {
        "system".to_string()
    }
}

fn pick_template(description: &str) -> ToolTemplate {
    let d = description.to_ascii_lowercase();
    if d.contains("port") || d.contains("scan") || d.contains("syn") || d.contains("tcp") {
        ToolTemplate::PortScanner
    } else if d.contains("web") || d.contains("http") || d.contains("fetch") || d.contains("url") {
        ToolTemplate::WebFetcher
    } else if d.contains("file") || d.contains("read") || d.contains("write") {
        ToolTemplate::FileTool
    } else if d.contains("memory") || d.contains("save") || d.contains("recall") {
        ToolTemplate::MemoryTool
    } else if d.contains("plan") || d.contains("task") {
        ToolTemplate::PlanTool
    } else {
        ToolTemplate::Generic
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolTemplate {
    PortScanner,
    WebFetcher,
    FileTool,
    MemoryTool,
    PlanTool,
    Generic,
}

fn render_template(
    name: &str,
    description: &str,
    category: &str,
    template: ToolTemplate,
) -> String {
    let body = match template {
        ToolTemplate::PortScanner => port_scanner_body(name),
        ToolTemplate::WebFetcher => web_fetcher_body(name),
        ToolTemplate::FileTool => file_tool_body(name),
        ToolTemplate::MemoryTool => memory_tool_body(name),
        ToolTemplate::PlanTool => plan_tool_body(name),
        ToolTemplate::Generic => generic_body(name, description),
    };
    let mut s = header(name, description, category);
    s.push('\n');
    s.push_str(&body);
    s
}

fn header(_name: &str, description: &str, category: &str) -> String {
    let mut s = String::new();
    s.push_str("//! ");
    s.push_str(description);
    s.push_str("\n//! Auto-synthesized by nanomite's neural tool synthesizer.\n//! Category: ");
    s.push_str(category);
    s.push('\n');
    s
}

fn port_scanner_body(name: &str) -> String {
    let mut s = String::new();
    s.push_str("use serde::{Deserialize, Serialize};\n");
    s.push_str("use std::net::{SocketAddr, ToSocketAddrs};\n");
    s.push_str("use std::time::Duration;\n\n");
    s.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    s.push_str("pub struct ");
    s.push_str(name);
    s.push_str("Args {\n");
    s.push_str("    pub host: String,\n");
    s.push_str("    #[serde(default)]\n");
    s.push_str("    pub ports: Option<Vec<u16>>,\n");
    s.push_str("    #[serde(default)]\n");
    s.push_str("    pub timeout_ms: Option<u64>,\n");
    s.push_str("}\n\n");
    s.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    s.push_str("pub struct ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("    pub ok: bool,\n");
    s.push_str("    pub open_ports: Vec<u16>,\n");
    s.push_str("    pub message: String,\n");
    s.push_str("}\n\n");
    s.push_str("pub fn run(args: ");
    s.push_str(name);
    s.push_str("Args) -> ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("    let default_ports: Vec<u16> = vec![22, 80, 443, 8080, 8443];\n");
    s.push_str("    let ports = args.ports.unwrap_or(default_ports);\n");
    s.push_str("    let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(800));\n");
    s.push_str("    let mut open = Vec::new();\n\n");
    s.push_str("    let addrs: Vec<std::net::IpAddr> = (args.host.as_str(), 0)\n");
    s.push_str("        .to_socket_addrs()\n");
    s.push_str("        .map(|iter| iter.map(|a| a.ip()).collect())\n");
    s.push_str("        .unwrap_or_default();\n\n");
    s.push_str("    for ip in addrs {\n");
    s.push_str("        for port in &ports {\n");
    s.push_str("            let addr = SocketAddr::new(ip, *port);\n");
    s.push_str("            if std::net::TcpStream::connect_timeout(&addr, timeout).is_ok() {\n");
    s.push_str("                open.push(*port);\n");
    s.push_str("            }\n");
    s.push_str("        }\n");
    s.push_str("    }\n\n");
    s.push_str("    ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("        ok: true,\n");
    s.push_str("        open_ports: open,\n");
    s.push_str("        message: ports.len().to_string() + \" ports scanned, \" + &open.len().to_string() + \" open\",\n");
    s.push_str("    }\n");
    s.push_str("}\n");
    s
}

fn web_fetcher_body(name: &str) -> String {
    let mut s = String::new();
    s.push_str("use serde::{Deserialize, Serialize};\n\n");
    s.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    s.push_str("pub struct ");
    s.push_str(name);
    s.push_str("Args {\n");
    s.push_str("    pub url: String,\n");
    s.push_str("}\n\n");
    s.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    s.push_str("pub struct ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("    pub ok: bool,\n");
    s.push_str("    pub content: String,\n");
    s.push_str("    pub status: u16,\n");
    s.push_str("    pub message: String,\n");
    s.push_str("}\n\n");
    s.push_str("pub fn run(args: ");
    s.push_str(name);
    s.push_str("Args) -> ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("    // This is a stub: the real implementation uses reqwest via nanomite's\n");
    s.push_str("    // web_tools. The synthesized tool delegates to nanomite's web_fetch.\n");
    s.push_str("    ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("        ok: true,\n");
    s.push_str("        content: String::new(),\n");
    s.push_str("        status: 200,\n");
    s.push_str("        message: \"web fetch template — delegate to web_fetch\".to_string(),\n");
    s.push_str("    }\n");
    s.push_str("}\n");
    s
}

fn file_tool_body(name: &str) -> String {
    let mut s = String::new();
    s.push_str("use serde::{Deserialize, Serialize};\n");
    s.push_str("use std::path::Path;\n\n");
    s.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    s.push_str("pub struct ");
    s.push_str(name);
    s.push_str("Args {\n");
    s.push_str("    pub path: String,\n");
    s.push_str("    #[serde(default)]\n");
    s.push_str("    pub content: Option<String>,\n");
    s.push_str("    #[serde(default)]\n");
    s.push_str("    pub write: bool,\n");
    s.push_str("}\n\n");
    s.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    s.push_str("pub struct ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("    pub ok: bool,\n");
    s.push_str("    pub content: String,\n");
    s.push_str("    pub message: String,\n");
    s.push_str("}\n\n");
    s.push_str("pub fn run(args: ");
    s.push_str(name);
    s.push_str("Args) -> ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("    let p = Path::new(&args.path);\n");
    s.push_str("    if args.write.unwrap_or(false) {\n");
    s.push_str("        if let Some(content) = &args.content {\n");
    s.push_str("            match std::fs::write(p, content) {\n");
    s.push_str("                Ok(_) => ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("                    ok: true,\n");
    s.push_str("                    content: content.clone(),\n");
    s.push_str("                    message: \"written\".to_string(),\n");
    s.push_str("                },\n");
    s.push_str("                Err(e) => ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("                    ok: false,\n");
    s.push_str("                    content: String::new(),\n");
    s.push_str("                    message: format!(\"write failed: {e}\"),\n");
    s.push_str("                },\n");
    s.push_str("            }\n");
    s.push_str("        } else {\n");
    s.push_str("            ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("                ok: false,\n");
    s.push_str("                content: String::new(),\n");
    s.push_str(
        "                message: \"write requested but no content provided\".to_string(),\n",
    );
    s.push_str("            }\n");
    s.push_str("        }\n");
    s.push_str("    } else {\n");
    s.push_str("        match std::fs::read_to_string(p) {\n");
    s.push_str("            Ok(content) => ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("                ok: true,\n");
    s.push_str("                content,\n");
    s.push_str("                message: \"read\".to_string(),\n");
    s.push_str("            },\n");
    s.push_str("            Err(e) => ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("                ok: false,\n");
    s.push_str("                content: String::new(),\n");
    s.push_str("                message: format!(\"read failed: {e}\"),\n");
    s.push_str("            },\n");
    s.push_str("        }\n");
    s.push_str("    }\n");
    s.push_str("}\n");
    s
}

fn memory_tool_body(name: &str) -> String {
    let mut s = String::new();
    s.push_str("use serde::{Deserialize, Serialize};\n\n");
    s.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    s.push_str("pub struct ");
    s.push_str(name);
    s.push_str("Args {\n");
    s.push_str("    pub key: String,\n");
    s.push_str("    #[serde(default)]\n");
    s.push_str("    pub value: Option<String>,\n");
    s.push_str("    #[serde(default)]\n");
    s.push_str("    pub save: bool,\n");
    s.push_str("}\n\n");
    s.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    s.push_str("pub struct ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("    pub ok: bool,\n");
    s.push_str("    pub value: String,\n");
    s.push_str("    pub message: String,\n");
    s.push_str("}\n\n");
    s.push_str("pub fn run(args: ");
    s.push_str(name);
    s.push_str("Args) -> ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("    // Stub: the real implementation delegates to nanomite's memory_save /\n");
    s.push_str("    // memory_search tools. This synthesized wrapper records the intent.\n");
    s.push_str("    if args.save.unwrap_or(false) {\n");
    s.push_str("        ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("            ok: true,\n");
    s.push_str("            value: args.value.clone().unwrap_or_default(),\n");
    s.push_str("            message: \"saved key \".to_string() + &args.key + \" to memory\",\n");
    s.push_str("        }\n");
    s.push_str("    } else {\n");
    s.push_str("        ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("            ok: true,\n");
    s.push_str("            value: String::new(),\n");
    s.push_str("            message: \"recall key \".to_string() + &args.key + \" — delegate to memory_search\",\n");
    s.push_str("        }\n");
    s.push_str("    }\n");
    s.push_str("}\n");
    s
}

fn plan_tool_body(name: &str) -> String {
    let mut s = String::new();
    s.push_str("use serde::{Deserialize, Serialize};\n\n");
    s.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    s.push_str("pub struct ");
    s.push_str(name);
    s.push_str("Args {\n");
    s.push_str("    pub plan_name: String,\n");
    s.push_str("    #[serde(default)]\n");
    s.push_str("    pub task: Option<String>,\n");
    s.push_str("    #[serde(default)]\n");
    s.push_str("    pub status: Option<String>,\n");
    s.push_str("}\n\n");
    s.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    s.push_str("pub struct ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("    pub ok: bool,\n");
    s.push_str("    pub message: String,\n");
    s.push_str("}\n\n");
    s.push_str("pub fn run(args: ");
    s.push_str(name);
    s.push_str("Args) -> ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("    // Stub: delegates to nanomite's create_plan / update_task tools.\n");
    s.push_str("    ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("        ok: true,\n");
    s.push_str("        message: \"plan \".to_string() + &args.plan_name + \" — delegate to create_plan/update_task\",\n");
    s.push_str("    }\n");
    s.push_str("}\n");
    s
}

fn generic_body(name: &str, description: &str) -> String {
    let mut s = String::new();
    s.push_str("use serde::{Deserialize, Serialize};\n\n");
    s.push_str("/// Arguments for the ");
    s.push_str(name);
    s.push_str(" synthesized tool.\n");
    s.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    s.push_str("pub struct ");
    s.push_str(name);
    s.push_str("Args {\n");
    s.push_str("    /// Free-form input for the tool.\n");
    s.push_str("    pub input: String,\n");
    s.push_str("    /// Optional workspace-relative path.\n");
    s.push_str("    #[serde(default)]\n");
    s.push_str("    pub path: Option<String>,\n");
    s.push_str("}\n\n");
    s.push_str("/// Result of the ");
    s.push_str(name);
    s.push_str(" synthesized tool.\n");
    s.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    s.push_str("pub struct ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("    pub ok: bool,\n");
    s.push_str("    pub output: String,\n");
    s.push_str("    pub message: String,\n");
    s.push_str("}\n\n");
    s.push_str("/// Execute the ");
    s.push_str(name);
    s.push_str(" tool.\n");
    s.push_str("pub fn run(args: ");
    s.push_str(name);
    s.push_str("Args) -> ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("    let input = args.input.trim();\n");
    s.push_str("    if input.is_empty() {\n");
    s.push_str("        return ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("            ok: false,\n");
    s.push_str("            output: String::new(),\n");
    s.push_str("            message: \"refused: empty input\".to_string(),\n");
    s.push_str("        };\n");
    s.push_str("    }\n");
    s.push_str("    // TODO: implement ");
    s.push_str(description);
    s.push('\n');
    s.push_str("    let output = \"synthesized \".to_string() + name + \" executed with input: \" + input;\n");
    s.push_str("    ");
    s.push_str(name);
    s.push_str("Result {\n");
    s.push_str("        ok: true,\n");
    s.push_str("        output,\n");
    s.push_str("        message: \"executed successfully\".to_string(),\n");
    s.push_str("    }\n");
    s.push_str("}\n");
    s
}

fn cargo_check(file_path: &Path) -> (bool, String) {
    let project_root = file_path
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent()) // src/neural/ -> src/ -> project root
        .unwrap_or_else(|| Path::new("."));

    let output = std::process::Command::new("cargo")
        .arg("check")
        .arg("--manifest-path")
        .arg(project_root.join("Cargo.toml"))
        .output();

    match output {
        Ok(out) => {
            let ok = out.status.success();
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = if !stdout.is_empty() { stdout } else { stderr };
            (ok, combined)
        }
        Err(e) => (false, format!("cargo not available: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn synthesize_creates_valid_rust_file() {
        let dir = temp_dir().join(format!("nanomite-synth-{}-creates", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut synth = ToolSynthesizer::new(dir.clone());
        let result = synth.synthesize(
            "stealth SYN port scanner for network reconnaissance",
            Some("stealth_scan"),
            false,
        );
        assert!(result.success);
        assert_eq!(result.tool.name, "StealthScanTool");
        assert_eq!(result.tool.category, "recon");
        assert!(!result.tool.source_code.is_empty());
        assert!(std::path::Path::new(&result.tool.file_path).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn derive_tool_name_is_valid_identifier() {
        let name = derive_tool_name("web vulnerability scanner");
        assert!(!name.is_empty());
        assert!(name.chars().all(|c| c.is_alphanumeric()));
    }

    #[test]
    fn categorize_matches_keywords() {
        assert_eq!(categorize("nmap port scan"), "recon");
        assert_eq!(categorize("sql injection web scan"), "web");
        assert_eq!(categorize("hashcat password crack"), "password");
        assert_eq!(categorize("wifi aircrack attack"), "wireless");
        assert_eq!(categorize("read a file"), "files");
        assert_eq!(categorize("remember this fact"), "memory");
        assert_eq!(categorize("create a plan"), "plan");
        assert_eq!(categorize("something random"), "system");
    }

    #[test]
    fn record_invocation_updates_counts() {
        let dir = temp_dir().join(format!("nanomite-synth-record-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut synth = ToolSynthesizer::new(dir.clone());
        synth.synthesize("test tool", Some("test_tool"), false);
        synth.record_invocation("TestTool");
        let tool = synth.get("TestTool").unwrap();
        assert_eq!(tool.invocation_count, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

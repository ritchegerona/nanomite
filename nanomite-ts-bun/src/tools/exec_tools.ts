/* Exec tools - ported from src/tools/exec_tools.rs */
export const runCommandSchema = {
  type: "object",
  properties: {
    command: { type: "string", description: "Shell command line, e.g. cargo test --quiet" },
    cwd: { type: "string", description: "Workspace-relative working directory (defaults to the workspace root)" },
    timeout_ms: { type: "integer", description: "Hard cap in milliseconds (max 120000, default 60000)" },
    sudo_password: { type: "string", description: "Optional sudo password for commands requiring elevation." }
  },
  required: ["command"],
  additionalProperties: false
};

export const sshRunSchema = {
  type: "object",
  properties: {
    host: { type: "string", description: "user@host or an ssh-config alias" },
    command: { type: "string", description: "Command line to run on the remote host" },
    timeout_ms: { type: "integer", description: "Hard cap in milliseconds (max 30000)" }
  },
  required: ["host", "command"],
  additionalProperties: false
};

export const scanPortsSchema = {
  type: "object",
  properties: {
    host: { type: "string", description: "Target host (hostname or IP)" },
    ports: { type: "array", items: { type: "integer" }, description: "Ports to scan (default: common service ports)" },
    timeout_ms: { type: "integer", description: "Per-connection timeout in milliseconds (default 800)" },
    concurrency: { type: "integer", description: "Maximum concurrent connections (default 100)" }
  },
  required: ["host"],
  additionalProperties: false
};
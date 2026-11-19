/* Test tools - ported from src/tools/test_tools.rs */
export const runTestsSchema = {
  type: "object",
  properties: {
    runner: { type: "string", description: "Test runner: cargo, npm, pytest, jest, go, or custom" },
    args: { type: "array", items: { type: "string" }, description: "Additional arguments for the runner" },
    cwd: { type: "string", description: "Workspace-relative working directory (defaults to workspace root)" },
    timeout_ms: { type: "integer", description: "Hard cap in milliseconds (max 300000, default 120000)" },
    command: { type: "string", description: "For custom runner: full command line to execute" }
  },
  required: ["runner"],
  additionalProperties: false
};
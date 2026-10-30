# Security

Security is a mandatory invariant: no evolution step may weaken these
controls to get a task done.

## Trust boundaries (verified in code)

1. **Workspace sandbox.** File tools resolve against the workspace root and
   refuse absolute paths, `..` escapes, and symlink traversals. Memory has
   its own independent root confinement (`memory/security.rs::
   resolve_within`, symlink-rejecting). Git, test, index, and plan tools
   share the same workspace confinement.
2. **Shell boundary.** `run_command` executes with the user's own permissions
   in the workspace — explicitly the same trust boundary as the user typing
   in a shell there. It is not a sandbox.
3. **SSH.** Key-based auth only, `BatchMode=yes` (never hangs on password
   prompts), host keys via the user's SSH config. No password or key material
   flows through the agent.
4. **Network.** `web_fetch`/`web_search` speak http(s) only; bodies are
   truncated before reaching the model.
5. **Secrets.** API key stored `0600`, never logged. Memory storage refuses
   anything shaped like a credential (detectors + placeholder allow-list so
   documentation samples don't trip the filter).
6. **Memory authorization.** `MemoryPolicy::authorize` gates every memory op
   (enabled/writable switches, sensitivity ceilings, bulk-confirm for
   delete/import). Destructive memory tools additionally require
   `confirm=true` at the tool layer.
7. **Self-edit denylist — removed (owner decision, 2026-10-30).** The hard
   denylist that refused any proposal touching `src/memory/security.rs`,
   `src/memory/policy.rs` or `src/main.rs` was deleted at the owner's explicit
   request for a private testing environment. Proposals against those files now
   stage like any other Source target; rule 8 (human approval, regression gate,
   byte-exact rollback, audit log) remains the control that decides what lands.
8. **Apply requires the human.** The agent can stage a proposal
   (`propose_self_edit`) but there is deliberately **no agent-callable apply**.
   `ImproveEngine::apply` is reachable only from the `/improve` review view on
   `a` (approve) or `r` (reject). Every apply writes a `.bak` backup, runs the
   regression gate (`cargo test --lib`), and restores the original bytes exactly
   if the suite goes red; every decision is appended to a JSONL audit log.
   Denied and refused proposals are recorded too.
9. **Board messages are untrusted data.** Personas are prompted by the agent and
   their output is stored verbatim in `board_messages`. Nothing a persona says
   can authorize a tool call, a self-edit or a file write.
10. **Tool library is enforced at both ends.** `disabled_tools` filters the
    schema list before the request and is re-checked at dispatch, so a disabled
    tool cannot be reached by guessing its name (`docs/TOOLS.md`).
11. **Clipboard.** `/copy` writes the last assistant message to the terminal's
    clipboard over OSC 52. The escape sequence is emitted to the user's own
    terminal, not to a network peer — and the copied text is model output, which
    is untrusted data.
12. **Central policy engine.** `agent/policy.rs` enforces `ToolPolicy`
    (Auto / Plan / Readonly) at dispatch. In Plan mode, only Safe tools +
    plan tools (`create_plan`, `update_task`, `get_plan`, `list_plans`) are
    permitted; `Write`/`External` tools are refused with a structured error
    telling the model to report its plan. Readonly mode allows only Safe tools.

## The remaining gap (honest)

What exists now: central `ToolPolicy` at dispatch + per-subsystem gating
(memory authorizes itself, disabled tools are refused, self-improvement has
a proposal gate, plan tools classified by risk). The standing rule until a
more granular engine exists: model requests are never authorization;
destructive, remote, credential, permission, or migration operations need
explicit user approval obtained outside the model transcript (prompt the user
in the TUI and record the decision).

## Non-negotiables for self-modification and updates

- Self-modification goes through the `/improve` gate or it does not happen. The
  agent stages; the user applies. Do not add an agent-callable apply path. The
  self-edit denylist was removed by owner decision (2026-10-30); re-adding it —
  or any change to the gate or the regression runner — is a human decision made
  outside the transcript.
- Model-generated code is untrusted until tests + review pass; never merge
  failed or unreviewed changes into the stable branch; never experiment on
  the live checkout (branches/worktrees only).
- Updates require authenticated releases (signatures or equivalent — HTTPS
  alone proves nothing); never execute fetched scripts; never overwrite user
  data or weaken controls during upgrade; rollback must be tested, not
  assumed.
- Audit records must be useful without containing secrets.
- Every security control ships with **negative tests** (refused traversal,
  refused credential, refused unconfirmed delete, failed-signature update),
  not just happy paths.
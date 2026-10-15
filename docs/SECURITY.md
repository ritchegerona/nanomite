# Security

Security is a mandatory invariant: no evolution step may weaken these
controls to get a task done.

## Trust boundaries (verified in code)

1. **Workspace sandbox.** File tools resolve against the workspace root and
   refuse absolute paths, `..` escapes, and symlink traversals. Memory has
   its own independent root confinement (`memory/security.rs::
   resolve_within`, symlink-rejecting).
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

## The missing piece (honest)

There is **no central policy/authorization engine**: model tool calls for the
9 core tools are not confirmed, logged to an audit trail, or allow-listed —
`runtime.rs::run_tool` dispatches directly. The standing rule until one
exists: model requests are never authorization; destructive, remote,
credential, permission, or migration operations need explicit user approval
obtained outside the model transcript (prompt the user in the TUI and record
the decision). Building the engine is Milestone 2's first item.

## Non-negotiables for self-modification and updates

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

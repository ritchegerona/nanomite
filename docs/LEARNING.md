# Daily learning

Status: **L3** — implemented and regression-tested (11 tests in the offline
suite). Updated 2026-11-04.

nanomite can study a topic on a schedule, persist what it learned, and stage
concrete improvements — without ever applying a change on its own. The loop is:

```
/learn <topic>  ──▶  normal agent turn
                       ├─ memory_search   recall prior lessons
                       ├─ web_fetch / browse / read_document / run_command
                       ├─ memory_save     lessons tagged learning + technique
                       └─ propose_self_edit  staged, inert until /improve
```

## Commands

| Command | What it does |
|---|---|
| `/learn <topic>` | Start a learning session now (alias `/study`). |
| `/learn status` | Last session, streak, total sessions, whether today is due. |
| `/learn digest [query]` | List saved lessons from memory (off the UI thread). |
| `/learn daily on\|off` | Opt into a once-per-calendar-day automatic session at startup. |
| `/learn topics` | Show the built-in rotating topic list. |
| `/learn topic <t>` | Pin a topic (clears with `/learn topic`). |

Shortcut: `alt+shift+l`.

A learning session is an ordinary agent turn whose prompt is
`learn::learning_prompt(topic)`. It uses the same tools, memory retrieval and
post-turn capture as any other turn. Lessons are saved with
`memory_save` under category `project` and tags `learning`, `technique` and a
slug of the topic. Concrete code or config changes are staged with
`propose_self_edit`; **there is no agent-callable apply** — a human approves in
`/improve` (see `docs/SECURITY.md`).

## Daily cadence

- Automatic sessions are **off by default**: each one spends model tokens.
  Enable with `/learn daily on` or `[learn] daily = true`.
- When enabled, the app checks once per local calendar day at startup. If no
  session has run that day, it starts one before the first user prompt.
- The streak advances only across consecutive days; a missed day resets it to 1.
  A second session on the same day refreshes the topic but not the counters.
- The topic is the pinned `[learn] topic`, else a deterministic daily rotation
  through `DEFAULT_TOPICS` (agent architectures, context engineering, agent
  evaluation, prompt design, tool-execution safety, multi-agent review).

## State and configuration

Cadence state lives in `<data>/nanomite/learn/state.json` (atomic write):

```json
{ "last_day": "2026-11-04", "streak": 3, "total_sessions": 12,
  "last_topic": "agent evaluation" }
```

`NANOMITE_LEARN_DIR` overrides the directory. The setup is persisted in the
config file:

```toml
[learn]
daily = false
# topic = "AI agent architectures and tool-use patterns"
```

Environment: `NANOMITE_LEARN_DAILY` (`1`/`0`) and `NANOMITE_LEARN_TOPIC` override
the file, as with every other setting.

## Why this is safe

- The learning turn is a normal turn: `ToolPolicy`, `disabled_tools`, the
  `Confirmer` gate and memory policy all still apply.
- Research output is **untrusted data**. It can write memory records and stage
  proposals, never authorize a privileged operation.
- `propose_self_edit` stages only. Applying requires the user's `a` key in
  `/improve`, behind the `.bak` backup, `cargo test --lib` gate and byte-exact
  rollback.
- Secrets are refused by the memory layer; the prompt tells the agent not to
  persist credentials or raw page dumps.

## Code

`src/learn/mod.rs` is pure logic — cadence state, topic rotation, slugging and
the instruction text. It performs no network I/O and never calls a model. The
app (`src/app.rs`) owns the UI wiring: `run_learn`, `start_learning`,
`maybe_daily_learning`, `run_learn_digest`, `learn_dir`, and the
`/learn digest` rendering. Tests cover due/streak/rotation/slug/prompt and the
command surface.

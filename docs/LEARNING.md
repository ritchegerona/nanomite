# Daily learning loop

Status: **L3** — implemented and regression-tested.

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
`learn::learning_prompt(topic)`. It uses the same tools, memory retrieval, and
post-turn capture as any other turn. Lessons are saved with `memory_save` under
category `project` and tags `learning`, `technique` and a slug of the topic.
Concrete code or config changes are staged with `propose_self_edit`; **there is
no agent-callable apply** — a human approves in `/improve` (see `docs/SECURITY.md`).

## Daily cadence

Automatic sessions are **off by default**: each one spends model tokens.
Enable with `/learn daily on` (`[learn] daily = true` in config). Nanomite
checks once per local calendar day at startup and runs one session if due.

Topic rotation uses `DEFAULT_TOPICS`; `/learn topics` lists them and
`/learn topic <t>` pins one. Pinned state lives in `state.json` under the data
dir.

The loop is pure logic: no network and no model calls inside `src/learn/`
itself — those happen through the normal agent turn.

# ⬡ nanomite

**A microscopic terminal coding agent.** Runs in your terminal, talks to any
OpenAI-compatible model, and works your workspace with real tools: files,
shell, SSH and the web.

Built with `ratatui` + `crossterm` + `tokio` in pure Rust.

```
 ⬡ NANOMITE [umbrella] › ~/dev/nanomite        medium ›  🧠 [██░░░░░░] 42.3k/200.0k · 21%
 ▌ list the files in src and read Cargo.toml

    → list_files src · 6 files + 3 dirs · 12ms
    → read_file Cargo.toml · 23 lines · 3ms

    The crate is a ratatui app with 3 layers:

    • ui/ — theme, transcript, composer
    • agent/ — providers and the tool loop
    • tools/ — workspace-scoped file access

    Key entry point is src/main.rs.

    ▏ let mut app = App::new(cfg, handle)?;

    Build · 8.4s · 45.2 tok/s
 ────────────────────────────────────────────────────────────────────────────
 ↑ 2.1k  ↓ 800  ·  ready
 ❯ Send a message or type / for commands
   enter send · ctrl+j newline · ctrl+o details
```

**Umbrella theme** (default) — blood-red accent on near-black

---

## Features

- **Real-time streaming** from any OpenAI-compatible endpoint (OpenAI, NVIDIA
  NIM, OpenRouter, Groq, Together, DeepSeek, Ollama, LM Studio, llama.cpp, …)
- **Full agent loop** — the model calls tools, nanomite executes them, results
  stream back until the task is done (unlimited steps by default, tunable via
  environment variables)
- **Persistent memory** — durable, searchable, model-independent memory
  (SQLite + human-readable markdown) that survives restarts and provider
  changes. It is on by default: records are retrieved before every turn and
  captured after, per policy (see [Persistent memory](#persistent-memory))
- **18 built-in tools** — file access, document extraction, a workspace shell,
  SSH, web access, six memory tools and two self-improvement tools
  (see [Tools](#tools))
- **Tool library** — `/tools` lists every tool grouped by category and lets you
  enable or disable any of them; disabled tools are hidden from the model and
  refused at dispatch
- **Self-improvement with a human in the loop** — the agent can propose an edit
  to its config, its prompt file or a source file; `/improve` shows the diff,
  and only you can approve it — behind a diff gate, a `.bak` backup, a
  `cargo test` regression check and automatic rollback on failure
- **Agent board** — `/panel <topic>` convenes four personas in parallel
  (Architect, Engineer, Scientist, Critic) plus a chair that synthesizes their
  answers, streamed live into an overlay
- **Follow-up suggestions** — when a turn finishes, a tiny side-call asks the
  model for three short next-step prompts and pops them up above the prompt:
  `Tab` cycles, `enter` sends the highlighted one, `esc` or typing dismisses
  (disable with `suggestions = false` in the config or `NANOMITE_SUGGESTIONS=0`)
- **Professional TUI** — virtualized transcript that stays fast with thousands
  of messages, markdown rendering with syntax-highlighted code blocks and
  grid tables, live tool-call rows, unified diffs in edit previews, context
  meter with a burn-rate projection, CJK/wide-character-safe composer
- **Perf hud** — `/hud` overlays frame time, peak, draw count and transcript
  stats into the footer, so "snappy" is a measurement, not a vibe
- **File-state time machine** — every `write_file`/`edit_file` snapshots the
  target's pre-change content per turn; `/revert [n]` restores the workspace
  to exactly before a turn (new files are removed again)
- **Session branching** — `/fork <n>` saves the full session, then continues
  on a branch trimmed to the first `n` turns
- **Cross-session search** — `/history <query>` full-text search across every
  saved session, results open as a resume picker
- **Effort A/B** — `/ab <prompt>` runs the same prompt at minimal and high
  reasoning effort side by side in the board overlay
- **Focus mode** — `f2` shows only the latest exchange, for long sessions on
  small terminals
- **`/doctor`** — pre-flight diagnostics: provider, key, memory store,
  sessions, workspace writability, tool toggles
- **4 themes** — Umbrella Corp (default, Resident Evil–inspired red), midnight,
  graphite and mono; the choice is persisted across sessions
- **Your own prompt** — drop extra instructions in `~/.config/nanomite/prompt.md`
  and they are appended to the system prompt on every turn
- **Persistent setup** — provider, key, model, theme and tool toggles are saved
  to `~/.config/nanomite/config.toml` and survive restarts
- **Offline-capable test suite** — 454 tests, zero network required

## Installation

### From source

Requires Rust 1.75+ (install via [rustup](https://rustup.rs)):

```bash
git clone https://github.com/ritchegerona/nanomite.git
cd nanomite
cargo build --release
```

The binary lands in `target/release/nanomite`. Put it on your `PATH`:

```bash
ln -sf "$(pwd)/target/release/nanomite" ~/.local/bin/nanomite
```

### From a release

Grab the prebuilt binary from the
[releases page](https://github.com/ritchegerona/nanomite/releases), unpack it
and move it somewhere on your `PATH`:

```bash
tar xzf nanomite-v0.1.0-x86_64-linux.tar.gz
install -m 755 nanomite ~/.local/bin/nanomite
```

### First run

```bash
cd your-project
nanomite
```

The app starts with no key configured and tells you what to do next:

1. Type `/` and pick **/connect** (or `/provider` to browse providers)
2. Choose a provider, paste your API key — it is saved to
   `~/.config/nanomite/config.toml` with owner-only permissions
3. Pick a model with `/models` and start working

> **No key? Keyless endpoints still work.** Ollama and LM Studio run locally
> with zero setup — point `/provider` at them and go.

## Configuration

Configuration is a TOML file written automatically whenever you change
provider, key or model: `~/.config/nanomite/config.toml`.

```toml
provider = "nvidia"
base_url = "https://integrate.api.nvidia.com/v1"
api_key = "your-key-here"          # never printed, 0600 perms
model = "openai/gpt-oss-20b"
reasoning = "medium"
theme = "umbrella"                 # persists your /themes choice
disabled_tools = "ssh_run,web_search"   # /tools toggles land here
prompt_file = "~/.config/nanomite/prompt.md"
suggestions = true                 # follow-up suggestion strip after each turn
```

Precedence: **built-in defaults < config file < environment variables**.

| Environment variable | Default | Purpose |
|---|---|---|
| `NANOMITE_API_KEY` / `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` | – | Enables the OpenAI-compatible provider |
| `NANOMITE_BASE_URL` / `OPENAI_BASE_URL` | `https://api.openai.com/v1` | Any `/v1` base URL |
| `NANOMITE_MODEL` / `OPENAI_MODEL` | `gpt-4o-mini` | Model id sent in requests |
| `NANOMITE_PROVIDER` | auto | Force a preset id (`openai`, `nvidia`, `groq`, `ollama`, …) |
| `NANOMITE_REASONING` | `medium` | `minimal` \| `low` \| `medium` \| `high` |
| `NANOMITE_CONTEXT_LIMIT` | `200000` | Token budget behind the context meter |
| `NANOMITE_MAX_STEPS` | unlimited | Agent steps per turn |
| `NANOMITE_MAX_TOOL_CALLS` | unlimited | Tool calls per turn |
| `NANOMITE_MAX_FILE_BYTES` | `200000` | Tool read cap |
| `NANOMITE_STEP_TIMEOUT_MS` | `120000` | Per-step ceiling |
| `NANOMITE_THEME` | `umbrella` | Startup palette (`umbrella`, `midnight`, `graphite`, `mono`) |
| `NANOMITE_DISABLED_TOOLS` | – | Comma-separated tool names to hide and refuse (overrides the config file) |
| `NANOMITE_PROMPT_FILE` | `~/.config/nanomite/prompt.md` | Extra instructions appended to the system prompt |
| `NANOMITE_SUGGESTIONS` | `true` | Follow-up suggestions after a finished turn (`0`/`false` disables) |
| `NANOMITE_BOARD_DIR` | `~/.local/share/nanomite/board` | Where the multi-agent board keeps its SQLite store |
| `NANOMITE_CONFIG` | `~/.config/nanomite/config.toml` | Alternate config path |

## Tools

The model sees these as callable functions — 18 in all, defined once in
`src/tools/catalog.rs` and rendered as OpenAI-function schemas. Everything
file-shaped is sandboxed to the workspace root — paths that escape are refused.
`/tools` turns any of them off.

| Tool | What it does |
|---|---|
| `list_files` | List files and directories at a workspace path |
| `read_file` | Read a UTF-8 file (size-capped, offset/limit supported) |
| `write_file` | Write or append content to a file |
| `edit_file` | Replace a string with another (first or all occurrences) |
| `search` | Recursive case-insensitive text search with hit caps |
| `read_document` | Read a PDF, DOCX, ODT, XLSX, ODS, RTF, CSV, Markdown or JSON document as bounded text via the `extract/` registry |
| `run_command` | Shell command via `sh -c` inside the workspace — builds, git, package managers, any Linux tooling. Hard timeout, output capped |
| `ssh_run` | Run a command on a remote host (`user@host`), key-based auth only, `BatchMode=yes` so it never hangs on a password prompt |
| `web_fetch` | GET an http(s) URL and return readable text (HTML stripped, entities decoded, size-capped) |
| `web_search` | Search the web (DuckDuckGo Lite) and return titles, URLs, snippets |
| `memory_search` | Search persistent memory (keyword + category/tag/project filters) |
| `memory_save` | Store a durable fact, decision, preference or lesson |
| `memory_get` | Fetch one memory by id |
| `memory_update` | Update fields of an existing memory (omitted fields stay) |
| `memory_delete` | Delete a memory — requires `confirm: true` |
| `memory_list` | List memories, newest or most important first |
| `propose_self_edit` | Stage a proposed change to the config, prompt file or a source file for you to review in `/improve` |
| `list_self_edits` | List proposals still waiting for your decision |

**Tool library**

`/tools` lists all 18 tools grouped by category (files · exec · web · memory ·
document · self-improve) with ✓ enabled / ✗ disabled. Toggles are written to
`disabled_tools` in `~/.config/nanomite/config.toml` and survive restarts;
`NANOMITE_DISABLED_TOOLS` overrides the file. A disabled tool is filtered out of
the schema list before it is ever sent to the provider *and* refused again at
dispatch, so it cannot be reached by guessing a call.

**Security notes**

- `run_command` executes with your user's permissions inside the workspace —
  it is the same trust boundary as you typing in a shell there
- `ssh_run` uses your existing SSH config/keys; password prompts are disabled
  (`BatchMode`) so a bad host fails instead of hanging the agent
- `web_fetch`/`web_search` only speak http(s); bodies are truncated before
  they reach the model
- `read_document` is read-only and workspace-confined, with bounded output
- The API key is stored with `0600` permissions and is never logged
- Memory tools authorize inside the engine; `memory_delete` needs an explicit
  confirmation, and credential-shaped content is refused on the way in
- `propose_self_edit` **cannot apply anything**: it only stages a proposal, and
  the safety-critical sources (`src/memory/security.rs`, `src/memory/policy.rs`,
  `src/main.rs`) are on a hard denylist. You approve in `/improve` or not at all

## Persistent memory

nanomite keeps durable memory between sessions — and independently of which
provider or model is configured. Nothing stored mentions a model, an endpoint
or a prompt format, so switching providers never invalidates what you have.

### What lives where

| Information | Home |
|---|---|
| Structured records, metadata, search indexes, retrieval statistics | SQLite (authoritative) |
| Durable prose you may want to hand-edit (facts, preferences, project notes, decisions) | markdown, mirrored from SQLite |
| High-volume working memory (episodic, session, task) | SQLite only |

```text
~/.config/nanomite/memory/          # hand-editable knowledge (XDG config home)
├── MEMORY.md                       # core facts and operating preferences
├── USER.md                         # user-approved preferences
├── projects/<project>.md           # per-project notes
└── decisions/<project>.md          # decision records
~/.local/share/nanomite/memory/     # generated state (XDG data home)
├── nanomite.db                     # SQLite: records, FTS5 index, markdown index
└── backups/                        # timestamped `nanomite.db` snapshots
```

Override the locations with `NANOMITE_MEMORY_DIR` (markdown root) and
`NANOMITE_MEMORY_DB` (database file). Markdown is a projection of SQLite, never
a second source of truth: editing a file by hand is safe — nanomite rewrites
only the blocks it owns (`<!-- nanomite:mem id=... -->` markers) and detects
external edits via `mtime`/`size` for reindexing.

### Categories

`core` (stable facts, operating preferences) · `user` (approved preferences) ·
`project` (architecture, status, constraints) · `episodic` (past tasks,
outcomes, lessons) · `decision` (decisions with rationale) · `session`
(checkpoints) · `task` (objectives, progress, results).

### How it is used

- **Before every model request**, a bounded, ranked set of memories (keyword
  matches, project decisions, always-relevant core/user facts) is injected as
  clearly framed, untrusted reference data — never as instructions.
- **After a finished turn**, the capture policy stores explicit user
  directives ("remember that …"), one session checkpoint and — for tool-using
  turns — a one-line outcome digest with a retention window. Assistant prose
  is never stored.
- Retrieval is bounded by policy (default: 8 records, 4 000 characters) and
  never grants tool permissions or overrides anything.

### Security rules

- Credential-shaped content (API keys, tokens, private keys, `password =`
  assignments, JWTs, …) is **refused on the way in** and redacted on the way
  out, in both SQLite and markdown
- Memory content is untrusted data: it cannot widen tool permissions or
  influence authorization
- Paths are confined to the memory root; `..` traversal and symlinks are
  refused
- Sensitivity (`public` < `internal` < `confidential` < `secret`) controls
  what may be injected into prompts

### Managing memory

- `/memory` — open the memory browser: recent records with category, project
  and time; select one to read its full body in an overlay. It runs async, so
  the UI never blocks on the database
- Ask the agent: "search your memory for X", "save that we deploy on
  fridays", "forget the note about Y" (it will ask before deleting)
- Memory attaches at startup by default. If it cannot, nanomite keeps running
  and says so in the footer — it never fails the launch. Set
  `NANOMITE_MEMORY=0` to run without it
- Backups: `MemoryEngine::backup()` writes a validated timestamped snapshot
  into `~/.local/share/nanomite/memory/backups/`; restore swaps a
  integrity-checked copy in only after validation, so a bad backup can never
  destroy working memory
- Exports: `export_to` writes a portable JSON bundle; `import_from` validates
  the whole bundle before writing a single row

| Environment variable | Default | Purpose |
|---|---|---|
| `NANOMITE_MEMORY` | `1` | Master switch (`0` disables the subsystem) |
| `NANOMITE_MEMORY_READONLY` | `0` | `1` makes memory read-only |
| `NANOMITE_MEMORY_AUTO` | `1` | `0` disables automatic post-turn capture |
| `NANOMITE_MEMORY_MARKDOWN` | `1` | `0` disables markdown mirroring |
| `NANOMITE_MEMORY_DIR` | platform | Markdown knowledge root |
| `NANOMITE_MEMORY_DB` | platform | SQLite database file |
| `NANOMITE_MEMORY_INJECT_RECORDS` | `8` | Max records injected per request |
| `NANOMITE_MEMORY_INJECT_CHARS` | `4000` | Max injected characters |
| `NANOMITE_MEMORY_INJECT_CEILING` | `internal` | Highest sensitivity injected into prompts |
| `NANOMITE_MEMORY_TOOL_CEILING` | `confidential` | Highest sensitivity tools may read |
| `NANOMITE_MEMORY_BULK_CONFIRM` | `10` | Deletions/imports above this need confirmation |
| `NANOMITE_MEMORY_MAX_ROWS` | `50` | Hard ceiling on query page size |
| `NANOMITE_MEMORY_RETENTION_DAYS` | `180` | Lifetime of auto-captured memories (`off` = keep) |

If the SQLite build lacks FTS5, nanomite detects it at startup and falls back
to `LIKE` search, reporting the index in use honestly.

## Usage

### Keys

| Key | Action |
|---|---|
| `enter` | Send · run the highlighted command |
| `/` or `ctrl+x` | Open the command menu |
| `ctrl+j` / `alt+enter` | Newline in the prompt |
| `esc` | Interrupt the agent · close an overlay · clear the prompt |
| `ctrl+t` / `ctrl+l` | New session · clear transcript |
| `ctrl+r` | Cycle reasoning effort |
| `ctrl+f` | Search the transcript (enter/↓ next · ↑ prev · esc closes) |
| `ctrl+o` | Toggle tool output details |
| `ctrl+p` / `ctrl+n` | Previous · next prompt from history |
| `tab` | Cycle follow-up suggestions (when shown) · accept ghost text |
| `ctrl+w` / `ctrl+u` / `ctrl+k` | Delete word · clear line · delete to end |
| `pgup` / `pgdn` / `ctrl+home` / `ctrl+end` | Scroll the transcript |
| `alt+↑` / `alt+↓` | Jump to the previous · next message |
| `home` / `end` | Jump to the start / end of the prompt |
| `ctrl+c` / `ctrl+d` | Quit (ctrl+d only when the prompt is empty) |
| `mouse wheel` · scrollbar | Scroll · click/drag the track to jump (bottom edge re-follows) |
| click a code block | Copy the block's source |
| click a failed message | Expand the full error |
| `f1` | This keysheet |
| `f2` | Focus mode — only the latest exchange |

While the agent works you can keep typing — the next prompt is queued and
sends automatically when the turn finishes.

### Commands

Type `/` to open the command menu — aliases included, listed grouped by
purpose: connect & model · agent scope · session · transcript · agent
features · appearance · output.

| Command | Aliases | Purpose |
|---|---|---|
| `/connect` | `/login`, `/auth` | Edit the endpoint URL and API key |
| `/provider` | `/providers` | Switch the provider — pick from the list |
| `/models` | `/model` | Switch the model — filter or type an id |
| `/reasoning` | `/effort` | Cycle reasoning effort: minimal · low · medium · high |
| `/policy` | `/mode` | Cycle tool mode: plan · coder · build · hacker |
| `/tools` | | Tool library — enable or disable the agent's tools |
| `/worktrees` | `/wt` | List git worktrees for parallel agents |
| `/new` | `/clear` | Start a new session |
| `/sessions` | `/resume`, `/continue` | List and switch sessions |
| `/history` | | Search all saved sessions — `/history <query>` |
| `/fork` | | Branch the session at turn N — `/fork <n>` |
| `/compact` | `/summarize` | Summarise the session to free up context |
| `/undo` | | Drop the last exchange |
| `/redo` | | Restore the last undone exchange |
| `/revert` | | Time machine — restore files to their state before a turn |
| `/prev` | | Browse to the previous turn |
| `/next` | | Browse to the next turn |
| `/copy` | | Copy the last assistant message |
| `/editor` | | Compose the prompt in `$EDITOR` |
| `/memory` | `/mem` | Browse persistent memory |
| `/panel` | | Convene the agent board on a topic (`/panel <topic>`) |
| `/ab` | | Compare minimal vs high reasoning side by side — `/ab <prompt>` |
| `/improve` | | Review self-edit proposals — `a` approve (with regression gate), `r` reject |
| `/improve-from-board` | | Stage the board's synthesis as a proposal |
| `/details` | | Toggle tool output details (`ctrl+o`) |
| `/thinking` | | Toggle the thinking indicator |
| `/focus` | | Show only the latest exchange (`f2`) |
| `/themes` | `/theme` | Switch colour theme (persisted) |
| `/hud` | | Toggle the performance hud (frame time, transcript stats) |
| `/export` | | Write the session to markdown |
| `/share` | | Export a shareable copy |
| `/unshare` | | Remove any shared copy |
| `/init` | | Create or update AGENTS.md |
| `/doctor` | | Run pre-flight diagnostics on the setup |
| `/help` | | Keysheet and command list |
| `/exit` | `/quit`, `/q` | Quit nanomite |

### Themes

`/themes` switches instantly: **umbrella** (default — Umbrella Corp red on
near-black), **midnight** (cool slate + periwinkle), **graphite** (QYVRANTH
teal/orange), **mono** (single-hue high contrast).

## Architecture

```
src/
├── main.rs          terminal setup, event loop
├── lib.rs           library root: runtime, tools and memory are testable
├── app.rs           state machine: keys, overlays, view model, command router
├── config.rs        defaults <- config file <- environment
├── commands.rs      the slash-command table
├── session.rs       session persistence + markdown export
├── extract/         universal file intelligence: per-format text extraction
│   ├── mod.rs       the extraction interface, limits, deadlines
│   ├── text.rs      text, markdown, html, code, jsonl
│   ├── structured.rs  xml, json, toml, yaml
│   ├── document.rs  rtf, docx, odt, pdf (feature-gated)
│   └── tabular.rs   csv, xlsx, ods (feature-gated)
├── agent/
│   ├── provider.rs  the provider trait: stream_chat, tool types
│   ├── openai.rs    OpenAI-compatible streaming client + SSE decoder
│   ├── runtime.rs   the agent loop: steps, tool dispatch, interrupts
│   ├── swarm.rs     parallel chat across several providers
│   └── testdouble.rs scripted provider (test builds only)
├── memory/
│   ├── mod.rs       the engine: paths, writes, mirroring, backups, reindexing
│   ├── model.rs     typed records, validation, taxonomy (provider-independent)
│   ├── store.rs     SQLite: schema, migrations, FTS5, search, import/export
│   ├── markdown.rs  knowledge files: blocks, atomic writes, indexing
│   ├── context.rs   retrieval for prompts + post-task capture policy
│   ├── policy.rs    limits, sensitivity ceilings, the authorization gate
│   └── security.rs  credential detection, path confinement, atomic IO
├── board/
│   ├── personas.rs  Architect, Engineer, Scientist, Critic + Chair
│   ├── engine.rs    convene(): parallel discussion rounds, then synthesis
│   └── store.rs     SQLite board_messages (untrusted content, verbatim)
├── improve/
│   ├── proposal.rs  proposals + validation, including the self-edit denylist
│   ├── diff.rs      unified diff artifacts
│   ├── runner.rs    .bak backup, regression check, byte-exact rollback
│   └── audit.rs     append-only JSONL decision log
├── tools/
│   ├── catalog.rs   the 18-tool inventory as data (name, description, category)
│   ├── file_tools.rs  list/read/write/edit/search (workspace-sandboxed)
│   ├── document_tools.rs  read_document over the extract/ registry
│   ├── exec_tools.rs  run_command, ssh_run
│   ├── web_tools.rs   web_fetch, web_search
│   ├── memory_tools.rs  the six memory_* tools (JSON in, bounded JSON out)
│   └── improve_tools.rs  propose_self_edit, list_self_edits (stage only)
└── ui/
    ├── mod.rs       frame layout
    ├── chat.rs      virtualized transcript + markdown
    ├── composer.rs  multi-line input, display-column caret math
    ├── panels.rs    header, footer, overlays, pickers
    ├── diff.rs      unified diff view (edit previews, /improve review)
    ├── theme.rs     the 4 palettes + shared glyph constants
    └── text.rs      wrapping, truncation, markdown renderer
```

- **Virtualized transcript** — only visible entries are rendered; cached
  render trees keep 2 000-message sessions smooth
- **Tool dispatch** — every call runs on the blocking pool with a hard
  timeout; the UI never blocks
- **SSE decoder** — incremental parser for streaming deltas, tool-call
  fragments and usage chunks
- **File-intelligence extractors** — bounded, read-only extraction of text,
  documents and tables so the agent can inspect rich files safely

## Development

```bash
cargo test --lib     # 370 tests, fully offline
cargo run            # dev build against your config
cargo build --release
```

## License

MIT

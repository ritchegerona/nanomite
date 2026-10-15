# ⬡ nanomite

**A microscopic terminal coding agent.** Runs in your terminal, talks to any
OpenAI-compatible model, and works your workspace with real tools: files,
shell, SSH and the web.

Built with `ratatui` + `crossterm` + `tokio` in pure Rust.

```
 ⬡ nanomite                                                model · medium
 ──────────────────────────────────────────────────────────────────────────
 › list the files in src and read Cargo.toml

   ✓ list_files  src · 6 files + 3 dirs
   ✓ read_file   Cargo.toml · 23 lines

 What I found
 The crate is a ratatui app with 3 layers:

 • ui/ — theme, transcript, composer
 • agent/ — providers and the tool loop
 • tools/ — workspace-scoped file access

 Key entry point is src/main.rs.

 ▏ let mut app = App::new(cfg, handle)?;
 ▏
 · ready                          640 [██░░░░░░░░] 3% ctx
──────────────────────────────────────────────────────────────────────────
 ❯ ask anything - / opens the command menu
   enter send · ctrl+j newline · / commands
```

---

## Features

- **Real-time streaming** from any OpenAI-compatible endpoint (OpenAI, NVIDIA
  NIM, OpenRouter, Groq, Together, DeepSeek, Ollama, LM Studio, llama.cpp, …)
- **Full agent loop** — the model calls tools, nanomite executes them, results
  stream back until the task is done (unlimited steps by default, tunable via
  environment variables)
- **Persistent memory** — durable, searchable, model-independent memory
  (SQLite + human-readable markdown) that survives restarts and provider
  changes (see [Persistent memory](#persistent-memory))
- **15 built-in tools** — file access, a workspace shell, SSH, web access and
  six memory tools (see [Tools](#tools))
- **Professional TUI** — virtualized transcript that stays fast with thousands
  of messages, markdown rendering, live tool-call rows, context meter
- **4 themes** — Umbrella Corp (default, Resident Evil–inspired red), midnight,
  graphite and mono
- **Persistent setup** — provider, key and model are saved to
  `~/.config/nanomite/config.toml` and survive restarts
- **Offline-capable test suite** — 261 tests, zero network required

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
| `NANOMITE_CONFIG` | `~/.config/nanomite/config.toml` | Alternate config path |

## Tools

The model sees these as callable functions. Everything file-shaped is
sandboxed to the workspace root — paths that escape are refused.

| Tool | What it does |
|---|---|
| `list_files` | List files and directories at a workspace path |
| `read_file` | Read a UTF-8 file (size-capped, offset/limit supported) |
| `write_file` | Write or append content to a file |
| `edit_file` | Replace a string with another (first or all occurrences) |
| `search` | Recursive case-insensitive text search with hit caps |
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

**Security notes**

- `run_command` executes with your user's permissions inside the workspace —
  it is the same trust boundary as you typing in a shell there
- `ssh_run` uses your existing SSH config/keys; password prompts are disabled
  (`BatchMode`) so a bad host fails instead of hanging the agent
- `web_fetch`/`web_search` only speak http(s); bodies are truncated before
  they reach the model
- The API key is stored with `0600` permissions and is never logged
- Memory tools authorize inside the engine; `memory_delete` needs an explicit
  confirmation, and credential-shaped content is refused on the way in

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

- `/memory` — one-line status: record count, index type, storage path
- Ask the agent: "search your memory for X", "save that we deploy on
  fridays", "forget the note about Y" (it will ask before deleting)
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
| `ctrl+p` / `ctrl+n` | Previous · next prompt from history |
| `ctrl+w` / `ctrl+u` / `ctrl+k` | Delete word · clear line · delete to end |
| `pgup` / `pgdn` / `home` / `end` | Scroll the transcript |
| `ctrl+c` / `ctrl+d` | Quit (ctrl+d only when the prompt is empty) |
| `f1` | This keysheet |

While the agent works you can keep typing — the next prompt is queued and
sends automatically when the turn finishes.

### Commands

Type `/` to open the command menu (aliases included):

| Command | Aliases | Purpose |
|---|---|---|
| `/connect` | `/login`, `/auth` | Edit the endpoint URL and API key |
| `/provider` | `/providers` | Switch provider — pick from the list |
| `/models` | `/model` | Switch model — filter or type an id |
| `/compact` | `/summarize` | Summarise the session to free context |
| `/new` | `/clear` | Start a new session |
| `/sessions` | `/resume`, `/continue` | List and switch sessions |
| `/reasoning` | `/effort` | Cycle minimal · low · medium · high |
| `/themes` | `/theme` | Switch colour theme |
| `/details` | | Toggle tool output details |
| `/thinking` | | Toggle the thinking indicator |
| `/editor` | | Compose the prompt in `$EDITOR` |
| `/export` | | Write the session to markdown |
| `/share` | | Export a shareable copy |
| `/undo` / `/redo` | | Drop / restore the last exchange |
| `/init` | | Create or update AGENTS.md |
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
│   └── testdouble.rs scripted provider (test builds only)
├── memory/
│   ├── mod.rs       the engine: paths, writes, mirroring, backups, reindexing
│   ├── model.rs     typed records, validation, taxonomy (provider-independent)
│   ├── store.rs     SQLite: schema, migrations, FTS5, search, import/export
│   ├── markdown.rs  knowledge files: blocks, atomic writes, indexing
│   ├── context.rs   retrieval for prompts + post-task capture policy
│   ├── policy.rs    limits, sensitivity ceilings, the authorization gate
│   └── security.rs  credential detection, path confinement, atomic IO
├── tools/
│   ├── file_tools.rs  list/read/write/edit/search (workspace-sandboxed)
│   ├── exec_tools.rs  run_command, ssh_run
│   ├── web_tools.rs   web_fetch, web_search
│   └── memory_tools.rs  the six memory_* tools (JSON in, bounded JSON out)
└── ui/
    ├── mod.rs       frame layout
    ├── chat.rs      virtualized transcript + markdown
    ├── composer.rs  multi-line input with cursor math
    ├── panels.rs    header, footer, overlays, pickers
    ├── theme.rs     the 4 palettes
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
cargo test --lib     # 261 tests, fully offline
cargo run            # dev build against your config
cargo build --release
```

## License

MIT

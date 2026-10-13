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
  stream back until the task is done (bounded by step limits you control)
- **9 built-in tools** — file access, a workspace shell, SSH and web access
  (see [Tools](#tools))
- **Professional TUI** — virtualized transcript that stays fast with thousands
  of messages, markdown rendering, live tool-call rows, context meter
- **4 themes** — Umbrella Corp (default, Resident Evil–inspired red), midnight,
  graphite and mono
- **Persistent setup** — provider, key and model are saved to
  `~/.config/nanomite/config.toml` and survive restarts
- **Offline-capable test suite** — 55 tests, zero network required

## Installation

### From source

Requires Rust 1.75+ (install via [rustup](https://rustup.rs)):

```bash
git clone https://github.com/YOUR_USERNAME/nanomite.git
cd nanomite
cargo build --release
```

The binary lands in `target/release/nanomite`. Put it on your `PATH`:

```bash
ln -sf "$(pwd)/target/release/nanomite" ~/.local/bin/nanomite
```

### From a release

Grab the prebuilt binary from the
[releases page](https://github.com/YOUR_USERNAME/nanomite/releases), unpack it
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
| `NANOMITE_MAX_STEPS` | `12` | Agent steps per turn |
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

**Security notes**

- `run_command` executes with your user's permissions inside the workspace —
  it is the same trust boundary as you typing in a shell there
- `ssh_run` uses your existing SSH config/keys; password prompts are disabled
  (`BatchMode`) so a bad host fails instead of hanging the agent
- `web_fetch`/`web_search` only speak http(s); bodies are truncated before
  they reach the model
- The API key is stored with `0600` permissions and is never logged

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
├── app.rs           state machine: keys, overlays, view model, command router
├── config.rs        defaults <- config file <- environment
├── session.rs       session persistence + markdown export
├── agent/
│   ├── provider.rs  the provider trait: stream_chat, tool types
│   ├── openai.rs    OpenAI-compatible streaming client + SSE decoder
│   ├── runtime.rs   the agent loop: steps, tool dispatch, interrupts
│   └── testdouble.rs scripted provider (test builds only)
├── tools/
│   ├── file_tools.rs  list/read/write/edit/search (workspace-sandboxed)
│   ├── exec_tools.rs  run_command, ssh_run
│   └── web_tools.rs   web_fetch, web_search
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

## Development

```bash
cargo test          # 55 tests, fully offline
cargo run           # dev build against your config
cargo build --release
```

## License

MIT

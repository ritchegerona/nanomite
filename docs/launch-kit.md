# nanomite v0.7.0 — launch kit

Ready-to-post announcements. Every claim is a verified feature of this
repository — no vaporware. Post as-is or trim to taste.

---

## X / Twitter (thread)

**1/**
Meet nanomite v0.7.0 — a microscopic terminal coding agent in one Rust
binary.

It remembers your projects between sessions, streams its reasoning, drives
a headless browser, and runs 500+ tests offline.

MIT. No telemetry. No account.

⬇ github.com/ritchegerona/nanomite

**2/**
The part nobody else does well: PERSISTENT MEMORY.

nanomite keeps a durable memory on your disk — SQLite + human-editable
markdown — that survives restarts AND model switches. Switch from NVIDIA
to Ollama to OpenRouter: your project knowledge stays.

**3/**
Selection accuracy that respects every character: wide glyphs, emoji
clusters, blank rows — the highlight starts exactly where your pointer
lands. Char-exact copying. No more broken terminal selection.

**4/**
Reasoning models streaming thoughts? You SEE them live (dim tail), then
they collapse to one line. A 4-minute reasoning phase never looks like a
hang again.

**5/**
Safety you can audit: 4 tool modes with policy gates, self-edit proposals
behind a human-in-the-loop gate, a workspace time
machine (`/revert` restores files to their pre-turn state), and crash
forensics written to disk before abort.

**6/**
Fast where it counts: engine events drained every loop pass (30 fps text),
0.12 ms full-frame renders measured, 500+ offline tests, `nanomite update`
moves you to the next version in one command.

Star it, read the README, break it, open issues.
github.com/ritchegerona/nanomite

---

## Reddit (r/rust, r/LocalLLaMA, r/programming) — title + body

**Title:** nanomite v0.7.0 — a tiny Rust terminal coding agent with
persistent memory, a workspace time machine and headless browser tools
(MIT, works with any OpenAI-compatible endpoint)

**Body:**
I have been building a terminal coding agent that treats three things as
first-class: privacy (everything stays on disk — code, SQLite memory,
session history; no telemetry), memory that survives restarts and model
switches (SQLite + human-editable markdown), and transparency (every tool
call, thought token and token budget is visible in the UI).

What is in v0.7.0:

- 34 tools, including headless browser automation (`browse` renders
  JavaScript-heavy pages, `browse_screenshot` saves PNGs)
- Reasoning streaming: thought tokens from nemotron/deepseek-r1-style
  models stream as a dim tail instead of an invisible hang
- A workspace time machine: `/revert` restores files to their pre-turn
  state from snapshots taken before every write
- Parallel analysts: `/team <goal>` splits a goal, runs three agents
  concurrently, synthesizes the reports
- Compact density, focus mode, transcript search, a perf HUD, and
  `nanomite update` for one-command self-updates

It speaks any OpenAI-compatible endpoint (Ollama, NVIDIA NIM, OpenRouter,
Groq, LM Studio, llama.cpp) — so local models work fully offline.

500+ offline tests, clippy clean, MIT.
Source + screenshots: github.com/ritchegerona/nanomite

---

## Hacker News (Show HN)

**Title:** Show HN: Nanomite — a 1-binary Rust coding agent with persistent
memory, a workspace time machine and headless browser tools

**Body:** I built a terminal coding agent because I wanted three things the
popular ones do not give me: everything stored locally (no telemetry, no
account), memory that survives restarts and model switches, and a UI that
shows exactly what the model is doing — including live reasoning tokens.

It is a single Rust binary speaking any OpenAI-compatible endpoint
(Ollama for fully offline use). 34 tools, four tool modes with policy
gates, a workspace time machine that restores files to their pre-turn
state, headless browser automation for JavaScript-heavy pages, three
parallel analysts on one goal, and a self-update command.

500+ tests run offline; the render pipeline is measured in the repo's
bench harness. MIT licensed — critique welcome, especially on the TUI
selection implementation and the tool policy gates.

github.com/ritchegerona/nanomite

---

## LinkedIn (short)

Shipping nanomite v0.7.0: a terminal coding agent in one Rust binary.

What makes it different: durable project memory that survives restarts and
model switches, a workspace time machine, headless browser automation, 4
auditable tool-modes with policy gates, and 500+ tests that run offline.

MIT. Any OpenAI-compatible endpoint. Your code never leaves your disk.
github.com/ritchegerona/nanomite

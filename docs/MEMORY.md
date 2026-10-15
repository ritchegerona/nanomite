# Memory

Provider-independent persistent memory: SQLite (bundled rusqlite) is the
authoritative store; Markdown documents are a one-way human-readable
projection. Nothing in `src/memory/` knows about any AI provider — records
are plain serde data, so changing models never touches stored knowledge.

Status: **L1** (implemented, repair in progress). Compiles; ~79 unit tests,
majority green; one failing test (`consolidation_merges_exact_duplicates_only`,
open); a store-wide `export()` deadlock was found and fixed during the
2026-10-14 audit. Runtime/UI integration is landing via `memory_tools.rs`
(concurrent workstream, unverified).

## Data model

Categories: `core`, `user`, `project`, `episodic`, `decision`, `session`,
`task` (fixed taxonomy — unknown categories are rejected). Each record
carries: content, summary, tags, provenance (`source.kind` + detail),
project slug, confidence (what the *source* vouches for, `None` = unstated),
importance (retrieval priority), sensitivity
(`public < internal < confidential < secret`), expiry, revision, and a
normalized-content hash for duplicate detection.

Provenance never grants authority: `source.kind = agent` is an unverified
model claim until confirmed. `secret` records never reach prompts or tool
output unless a caller explicitly opts in above the ceiling.

## Storage

- `MemoryStore` (sync `Mutex<Connection>`, WAL + busy timeout): CRUD,
  FTS5 keyword search with LIKE fallback (the index used is reported in
  results), filter listing, dedupe-on-save, deterministic `consolidate()`,
  expiry purge (retention only — unexpired data is never auto-removed),
  two-phase validated export/import, `VACUUM INTO` backup and
  integrity-checked restore with connection swap.
- `KnowledgeStore` (markdown): mirrored categories (`MEMORY.md`, `USER.md`,
  `projects/{p}.md`, `decisions/{p}.md`) hold nanomite-owned blocks marked
  `<!-- nanomite:mem id=… -->`. Rewrites touch only the record's own block;
  hand-written prose is preserved byte-for-byte. External edits are detected
  via `mtime`/`size` in the SQLite markdown index.
- `MemoryPolicy`: env-gated limits and the subsystem's authorization gate
  (`NANOMITE_MEMORY=0` off, `..._READONLY=1`, `..._AUTO=0`,
  inject/tool sensitivity ceilings, bulk-confirm threshold, inject budgets,
  retention days). Disabled or read-only policy refuses at the gate, including
  reads when disabled.
- `security.rs`: credential detectors (API keys, tokens, JWT, bearer,
  private blobs, `key = value` assignments with placeholder allow-list) —
  storage refuses credentials; plus FNV-1a content hashing (non-crypto),
  `mem_<hex>` ids, atomic writes, `0600` files, and root confinement for
  every path including symlink rejection.

## Rules for future work

- SQLite stays authoritative; markdown stays a projection (never a second
  source of truth, never rewritten on background reads alone).
- New categories, sensitivities, or auto-capture behaviors need policy flags,
  tests (including negative tests), and a CHANGELOG entry.
- Retrieval budgets (`max_inject_records/chars`) are ceilings, not targets;
  injection must pass `MemoryOp::Inject` authorization.

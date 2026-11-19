## Fix: Left Thinking Panel Shows Reasoning Words Again

The /thinking toggle extracted `show_thinking` but never used it — only `show_activity` controlled the left info column. This meant:

- **Thinking panel appeared blank/missing** when the user expected reasoning words in the left panel
- The `/thinking` toggle had no visible effect
- `draw_thinking_panel` was only reached when `show_activity` was false, ignoring `show_thinking`

### Change

`src/ui/mod.rs`: left info column now respects both flags — activity timeline when `show_activity` is on, thinking panel when `show_thinking` is on (default on).

### Also includes (from v1.3.8)

- Neural AI agent (network, memory, synthesizer, agent, planner under `src/neural/`)
- Zero refusals across all AI models and layers
- Auto-install missing security tools via Homebrew/apt
- Skills registry with `skills_list` / `skills_known`
- Performance improvements — removed enforcement overhead
- `/activity` toggle (alt+a) to switch left panel between thinking ↔ activity

All 847 lib tests pass. Cargo check clean.

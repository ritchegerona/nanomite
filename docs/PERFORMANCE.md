# Performance

Status: **L1** — microbenchmarked. A stable, zero-dependency benchmark harness
(`benches/perf.rs`, run with `cargo bench`) covers the tool/extract hot paths.
No runtime telemetry or profiling hooks exist yet.

## Benchmarks

Harness: `benches/perf.rs` — median of 7 in-process batches after warmup, zero
external deps. Machine noise between full runs is ±8–15% (all benches drift
together); treat sub-15% deltas as noise. 300-file / ~4 MB synthetic tree for
search; 1.3 MB file sliced to 4 KB for read_file.

| Benchmark | ns/op | ms/op |
|---|---:|---:|
| search / 300 files hit | ~670,000 | 0.67 |
| search / 300 files miss | ~5,170,000 | 5.17 |
| read_file 4 KB slice of 1.3 MB | ~27,000 | 0.03 |
| extract text 270 KB | ~262,000 | 0.26 |
| extract code 270 KB | ~324,000 | 0.32 |
| extract json ~160 KB | ~480,000 | 0.48 |
| extract toml ~20 KB | ~1,200,000 | 1.20 |
| extract csv 4k rows | ~515,000 | 0.52 |
| wrap_spans / 40 spans w=100 | ~7,300 | 0.01 |
| markdown / 24 KB parse+wrap | ~687,000 | 0.69 |
| entry.rebuild 24 KB (streaming frame) | ~1,020,000 | 1.02 |
| paint/clone 250 visible lines | ~147,000 | 0.15 |
| frame.render 300 entries (steady) | ~67,000 | 0.07 |
| fingerprint_file / 1 MB | ~286,000 | 0.29 |

Startup (3–6 ms), TUI rendering (dirty-flagged with cached renders), and the
SQLite memory store (WAL + `synchronous=NORMAL`) were inspected and left alone —
already fast by design.

## VPN / Tor feature-gating

Building with `--features vpn,tor` does not slow down nanomite's core runtime:

- **Build time:** only slightly longer (a few extra modules + the `nanomite-vpn`
  binary).
- **Binary size:** modestly larger, but idle feature code has zero runtime cost.
- **Idle:** if `/vpn start` or `/tor start` are never invoked, nanomite executes
  exactly as fast as without the features.
- **Active VPN:** the VPN tunnel is a separate process; system-wide network
  throughput may drop due to encryption overhead, but nanomite's TUI and local
  commands are unaffected.
- **Active Tor:** deepweb browsing/search latency increases because traffic
  traverses volunteer relays through the SOCKS5 proxy on `127.0.0.1:9050`. This
  is network latency, not nanomite code slowing down.

## Rules

1. **Profile before optimizing.** Keep the tokio runtime, the sync
   `Mutex<Connection>` memory design, and the render pipeline unless
   measurements justify change.
2. Every optimization ships with a reproducible benchmark and before/after
   numbers in the CHANGELOG entry. Estimated tokens are labeled estimates;
   provider-reported usage is labeled as such; savings claims require a
   comparable baseline.
3. Budgets and caps are load-bearing: tool output caps, page limits
   (`HARD_ROW_CAP`), bounded queues, timeouts, and cancellation must survive
   any change.

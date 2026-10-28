//! Stable profiling harness for nanomite's hot paths.
//!
//! Run with `cargo bench` (the `bench` profile is release-like). The harness is
//! dependency-free and measures median ns/op over repeated batches after a
//! warmup, which is stable enough for before/after optimization comparisons.
//!
//! Paths measured (chosen because real sessions hit them constantly):
//! - `file_tools::search`  — case-insensitive scan across a source tree
//! - `file_tools::read_file` — slice of a large file
//! - `extract`             — text / code / json / toml / csv extraction
//! - `ui::text::wrap_spans`— chat render wrapping cost
//! - `fingerprint`         — per-file content hashing

use nanomite::extract::{self, ExtractionLimits};
use nanomite::tools::file_tools::{FileTools, ReadFileArgs, SearchArgs};
use nanomite::ui::text::wrap_spans;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

fn bench<F: FnMut() + 'static>(name: &'static str, iters: usize, mut f: F) {
    // Warmup: page cache, allocator arenas, branch predictors.
    for _ in 0..iters.min(5) {
        f();
    }
    let batches = 7;
    let mut samples = Vec::with_capacity(batches);
    for _ in 0..batches {
        let start = Instant::now();
        for _ in 0..iters {
            f();
        }
        samples.push(start.elapsed().as_nanos() / iters as u128);
    }
    samples.sort_unstable();
    let med = samples[samples.len() / 2];
    println!(
        "{:<34} {:>10} ns/op   ({:.2} ms/op @ {} iters/batch)",
        name,
        med,
        med as f64 / 1_000_000.0,
        iters
    );
}

// ---------------------------------------------------------------------------
// Fixture builders (all under a scratch temp dir, removed at the end)
// ---------------------------------------------------------------------------

const LINES_PER_FILE: usize = 300;
const TREE_FILES: usize = 300;

/// One fake Rust file body: mixed case so a naive fast path would be wrong.
fn rust_source(idx: usize) -> String {
    let mut s = String::with_capacity(LINES_PER_FILE * 48);
    s.push_str(&format!("// module {idx} — Model Registry sample\n"));
    for i in 0..LINES_PER_FILE {
        match i % 8 {
            0 => s.push_str(&format!("pub struct Model{idx}_{i} {{ id: u64 }}\n")),
            1 => s.push_str(&format!(
                "    fn model_step(&self, delta: usize) -> usize {{ delta + {} }}\n",
                i % 7
            )),
            2 => s.push_str("    // TODO(Model): wire the streaming accumulator later\n"),
            3 => s.push_str(&format!("    let payload = Payload {{ weight: {i}.5 }};\n")),
            4 => s.push_str("        self.registry.insert(key, value.clone());\n"),
            5 => s.push_str("«» une ligne UNICODE ÜBER Alles — идёт дождь\n"),
            6 => s.push_str(&format!("}} // end Model{idx}_{i}\n")),
            _ => s.push_str("    #[derive(Debug, Clone)]\n"),
        }
    }
    s
}

fn prose(idx: usize) -> String {
    (0..LINES_PER_FILE)
        .map(|i| {
            format!("Paragraph {idx}.{i}: the Model registry keeps every Streaming Agent honest.\n")
        })
        .collect()
}

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("nanomite-bench-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("scratch dir");
        Scratch { root }
    }

    fn write(&self, name: &str, body: &str) -> PathBuf {
        let p = self.root.join(name);
        fs::write(&p, body).expect("write fixture");
        p
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn main() {
    println!("nanomite perf harness — stable, zero-dep\n");

    // -- search --------------------------------------------------------------
    let tree = Scratch::new("tree");
    let src = tree.root.join("src");
    fs::create_dir_all(&src).unwrap();
    for i in 0..TREE_FILES {
        fs::write(src.join(format!("mod{i}.rs")), rust_source(i)).unwrap();
    }
    let ft = Arc::new(FileTools::new(tree.root.clone(), 4 * 1024 * 1024));

    {
        let ft = Arc::clone(&ft);
        bench("search/300files hit", 3, move || {
            let r = ft
                .search(SearchArgs {
                    query: "model".into(),
                    path: None,
                })
                .expect("search");
            assert!(!r.hits.is_empty());
        });
    }
    {
        let ft = Arc::clone(&ft);
        bench("search/300files miss", 3, move || {
            let r = ft
                .search(SearchArgs {
                    query: "zzzqqqxxx".into(),
                    path: None,
                })
                .expect("search");
            assert!(r.hits.is_empty());
        });
    }

    // -- read_file -----------------------------------------------------------
    let big = Scratch::new("big");
    let big_path = big.write("big.rs", &rust_source(9_999).repeat(90)); // ~1.2 MB
    let ft2 = Arc::new(FileTools::new(big.root.clone(), 64 * 1024 * 1024));
    {
        let ft2 = Arc::clone(&ft2);
        let path = big_path.display().to_string();
        bench("read_file/1.3MB slice 4KB", 20, move || {
            let r = ft2
                .read_file(ReadFileArgs {
                    path: path.clone(),
                    offset: Some(600_000),
                    limit: Some(4_096),
                })
                .expect("read_file");
            assert!(!r.content.is_empty());
        });
    }

    // -- extract -------------------------------------------------------------
    let ext = Scratch::new("extract");
    let text_p = ext.write("doc.txt", &prose(1).repeat(9));
    let code_p = ext.write("code.rs", &rust_source(2).repeat(9));
    let json_v = serde_json::json!({
        "name": "nanomite", "presets": (0..400).map(|i|
            serde_json::json!({"id": i, "name": format!("Model{i}"), "tags": ["a","b"], "score": i as f64 / 7.0})
        ).collect::<Vec<_>>()
    });
    let json_p = ext.write("data.json", &serde_json::to_string_pretty(&json_v).unwrap());
    let mut toml = String::from("[package]\nname = \"nanomite\"\n");
    for i in 0..400 {
        toml.push_str(&format!(
            "[preset.{i}]\nname = \"Model{i}\"\nmodel = \"m/{i}\"\nactive = true\nscore = {}.5\n",
            i % 97
        ));
    }
    let toml_p = ext.write("cfg.toml", &toml);
    let mut csv = String::from("id,name,score,active\n");
    for i in 0..4_000 {
        csv.push_str(&format!("{i},Model{i},{},true\n", i as f64 / 13.0));
    }
    let csv_p = ext.write("rows.csv", &csv);
    let limits = ExtractionLimits::default();

    macro_rules! extract_bench {
        ($label:expr, $path:expr) => {{
            let path: PathBuf = $path.to_path_buf();
            let limits = limits.clone();
            bench($label, 10, move || {
                let d = extract::extract_path(&path, &limits).expect("extract");
                assert!(!d.text.is_empty());
            });
        }};
    }
    extract_bench!("extract/text 270KB", &text_p);
    extract_bench!("extract/code 270KB", &code_p);
    extract_bench!("extract/json ~160KB", &json_p);
    extract_bench!("extract/toml ~20KB", &toml_p);
    extract_bench!("extract/csv 4k rows", &csv_p);

    // -- wrap ----------------------------------------------------------------
    let mut chat_line: Vec<Span<'static>> = Vec::new();
    for i in 0..40 {
        if i % 3 == 0 {
            chat_line.push(Span::raw(format!("word{i} ")));
        } else if i % 3 == 1 {
            chat_line.push(Span::styled(
                format!("styled_token_{} ", i),
                Style::new().fg(Color::Cyan),
            ));
        } else {
            chat_line.push(Span::raw("a slightly longer run of plain text here "));
        }
    }
    {
        let base = chat_line;
        bench("wrap_spans/40spans w=100", 200, move || {
            let lines = wrap_spans(base.clone(), 100);
            assert!(!lines.is_empty());
        });
    }

    // -- transcript render paths (the streaming hot path) --------------------
    // A realistic long assistant reply: headings, lists, code fences, tables.
    let md = {
        let mut m = String::from("## Summary\n\nThe migration is complete:\n\n");
        for i in 0..420 {
            m.push_str(&format!(
                "- item {i} with `inline code` and **bold** text\n"
            ));
            if i % 8 == 0 {
                m.push_str(&format!("\n### Section {i}\n\n"));
                m.push_str(&format!(
                    "```rust\nfn step_{i}(&self) -> u32 {{ {i} }}\n```\n\n"
                ));
            }
            if i % 16 == 0 {
                m.push_str("| name | value |\n|---|---|\n| a | 1 |\n| b | 2 |\n\n");
            }
        }
        m
    };
    println!("markdown doc: {} bytes\n", md.len());
    {
        use nanomite::ui::theme::{Theme, ThemeId};
        let md = md.clone();
        bench("markdown/24KB parse+wrap", 20, move || {
            let theme = Theme::with_id(ThemeId::Umbrella);
            let st = nanomite::ui::text::MarkdownStyles::new(&theme);
            let (lines, blocks) = nanomite::ui::text::markdown_with_blocks(&md, 84, &st);
            assert!(lines.len() > 100);
            assert!(blocks.len() >= 8, "tables + fences: {}", blocks.len());
        });
    }
    {
        // The streaming rebuild: a cache-invalidated entry build - what every
        // delta frame pays while a long response streams in.
        use nanomite::app::{Entry, EntryRole, ToolDisplay};
        use nanomite::ui::theme::{Theme, ThemeId};
        let mut entry = Entry::new(EntryRole::Assistant, md.clone());
        let theme = Theme::with_id(ThemeId::Umbrella);
        let mode = (theme.accent(), "Build");
        bench("entry.rebuild 24KB (streaming frame)", 20, move || {
            entry.touch();
            let rendered = entry.ensure_rendered(&theme, 91, ToolDisplay::Hidden, mode);
            assert!(rendered.lines.len() > 100);
        });
    }
    {
        // Steady-state paint: cache hits only, the cost of every idle frame.
        use nanomite::app::{Entry, EntryRole, ToolDisplay};
        use nanomite::ui::theme::{Theme, ThemeId};
        let mut entry = Entry::new(EntryRole::Assistant, md.clone());
        let theme = Theme::with_id(ThemeId::Umbrella);
        let mode = (theme.accent(), "Build");
        let rendered = entry.ensure_rendered(&theme, 91, ToolDisplay::Hidden, mode);
        let lines = rendered.lines.clone();
        bench("paint/clone 250 visible lines", 200, move || {
            let cloned: Vec<_> = lines.clone();
            assert!(!cloned.is_empty());
        });
    }

    // -- full frame render (real App, memory disabled) -----------------------
    std::env::set_var("NANOMITE_MEMORY", "0");
    let mut frame_cfg = nanomite::config::Config::from_env();
    frame_cfg.apply_preset("openai");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("bench runtime");
    let handle = runtime.handle().clone();
    let mut frame_app = nanomite::app::App::new(frame_cfg, handle).expect("bench app");
    {
        use nanomite::app::{Entry, EntryRole};
        for i in 0..150 {
            frame_app.entries.push(Entry::new(
                EntryRole::User,
                format!("question {i} about module {i} and its streaming behaviour"),
            ));
            frame_app.entries.push(Entry::new(
                EntryRole::Assistant,
                format!("answer {i}: the module streams in chunks and renders fine."),
            ));
        }
    }
    {
        let backend = ratatui::backend::TestBackend::new(92, 24);
        let mut terminal = ratatui::Terminal::new(backend).expect("bench terminal");
        bench("frame.render 300 entries (steady)", 20, move || {
            terminal
                .draw(|frame| nanomite::ui::render(frame, &mut frame_app))
                .expect("draw");
        });
    }

    // -- fingerprint ---------------------------------------------------------
    let fp_body = rust_source(7).repeat(30); // ~1 MB
    let fp_p = ext.write("fp.txt", &fp_body);
    {
        let path = fp_p;
        bench("fingerprint_file/1MB", 20, move || {
            let fp = extract::fingerprint_file(&path).expect("fingerprint");
            assert!(fp.len() == 16);
        });
    }

    println!("\nDone.");
}

// -- temporary phase isolation for search -----------------------------------
#[allow(dead_code)]
fn phase_test() {
    use std::time::Instant;
    let tree = std::env::temp_dir().join(format!("nanomite-bench-{}-tree", std::process::id()));
    let mut bytes = Vec::new();
    for i in 0..TREE_FILES {
        bytes.extend_from_slice(rust_source(i).as_bytes());
    }
    println!("total bytes: {}", bytes.len());
    let t = Instant::now();
    let text = String::from_utf8_lossy(&bytes);
    println!("from_utf8_lossy: {:?}", t.elapsed());
    let t = Instant::now();
    let lower = text.to_lowercase();
    println!("whole to_lowercase: {:?}", t.elapsed());
    let t = Instant::now();
    let alower = text.to_ascii_lowercase();
    println!("whole to_ascii_lowercase: {:?}", t.elapsed());
    let t = Instant::now();
    let n: usize = text
        .lines()
        .filter(|l| l.to_lowercase().contains("model"))
        .count();
    println!(
        "per-line to_lowercase+contains: {:?} (hits {n})",
        t.elapsed()
    );
    let t = Instant::now();
    let n: usize = lower
        .lines()
        .zip(text.lines())
        .filter(|(lo, _)| lo.contains("model"))
        .count();
    println!("prelowered zip contains: {:?} (hits {n})", t.elapsed());
    let t = Instant::now();
    let n: usize = alower
        .lines()
        .zip(text.lines())
        .filter(|(lo, _)| lo.contains("model"))
        .count();
    println!("ascii-lowered zip contains: {:?} (hits {n})", t.elapsed());

    // hand-rolled case-insensitive contains directly on original bytes
    fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
        if needle.is_empty() {
            return true;
        }
        let n = needle.len();
        if n > hay.len() {
            return false;
        }
        let first = needle[0] | 32;
        let mut i = 0;
        let last = hay.len() - n;
        while i <= last {
            if hay[i] | 32 == first {
                let mut ok = true;
                for (h, w) in hay[i..i + n].iter().zip(needle) {
                    if h | 32 != w | 32 {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
    let t = Instant::now();
    let n: usize = text
        .lines()
        .filter(|l| contains_ci(l.as_bytes(), b"model"))
        .count();
    println!("hand-rolled ci contains: {:?} (hits {n})", t.elapsed());

    let t = Instant::now();
    let lower_a = text.to_ascii_lowercase();
    let n: usize = lower_a.lines().filter(|l| l.contains("model")).count();
    println!("ascii fold + plain contains: {:?} (hits {n})", t.elapsed());
    drop(lower_a);
    drop(tree);
}

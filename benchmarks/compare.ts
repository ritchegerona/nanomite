/**
 * Cross-framework benchmark: Pi agent vs Nanomite
 * 
 * This script benchmarks comparable operations in both frameworks:
 * 1. File read (with offset/limit)
 * 2. File search/grep
 * 3. File write
 * 
 * Run with: npx tsx benchmarks/compare.ts
 * Or: cargo bench --bench perf (for nanomite)
 */

import { performance } from "node:perf_hooks";
import { readFile, writeFile, mkdir, rm, readdir } from "node:fs/promises";
import { join } from "node:path";
import { createReadStream } from "node:fs";
import { createInterface } from "node:readline";

// ============================================================================
// Test Fixtures
// ============================================================================

const LINES_PER_FILE = 300;
const TREE_FILES = 300;
const TEST_DIR = "/tmp/bench-compare";

function rustSource(idx: number): string {
    let s = `// module ${idx} — Model Registry sample\n`;
    for (let i = 0; i < LINES_PER_FILE; i++) {
        switch (i % 8) {
            case 0: s += `pub struct Model${idx}_${i} { id: u64 }\n`; break;
            case 1: s += `    fn model_step(&self, delta: usize) -> usize { delta + ${i % 7} }\n`; break;
            case 2: s += `    // TODO(Model): wire the streaming accumulator later\n`; break;
            case 3: s += `    let payload = Payload { weight: ${i}.5 };\n`; break;
            case 4: s += `        self.registry.insert(key, value.clone());\n`; break;
            case 5: s += `«» une ligne UNICODE ÜBER Alles — идёт дождь\n`; break;
            case 6: s += `} // end Model${idx}_${i}\n`; break;
            default: s += `    #[derive(Debug, Clone)]\n`; break;
        }
    }
    return s;
}

function prose(idx: number): string {
    return Array.from({ length: LINES_PER_FILE }, (_, i) => 
        `Paragraph ${idx}.${i}: the Model registry keeps every Streaming Agent honest.\n`
    ).join("");
}

// ============================================================================
// Benchmark Helpers
// ============================================================================

async function setupFixtures() {
    await rm(TEST_DIR, { recursive: true, force: true });
    await mkdir(join(TEST_DIR, "src"), { recursive: true });
    await mkdir(join(TEST_DIR, "extract"), { recursive: true });
    
    // Create search tree
    for (let i = 0; i < TREE_FILES; i++) {
        await writeFile(join(TEST_DIR, "src", `mod${i}.rs`), rustSource(i));
    }
    
    // Create large file for read benchmarks
    await writeFile(join(TEST_DIR, "big.rs"), rustSource(9999).repeat(90));
    
    // Create extract test files
    await writeFile(join(TEST_DIR, "extract", "doc.txt"), prose(1).repeat(9));
    await writeFile(join(TEST_DIR, "extract", "code.rs"), rustSource(2).repeat(9));
    
    // JSON
    const jsonData = {
        name: "nanomite",
        presets: Array.from({ length: 400 }, (_, i) => ({
            id: i,
            name: `Model${i}`,
            tags: ["a", "b"],
            score: i / 7.0
        }))
    };
    await writeFile(join(TEST_DIR, "extract", "data.json"), JSON.stringify(jsonData, null, 2));
    
    // TOML
    let toml = "[package]\nname = \"nanomite\"\n";
    for (let i = 0; i < 400; i++) {
        toml += `[preset.${i}]\nname = "Model${i}"\nmodel = "m/${i}"\nactive = true\nscore = ${i % 97}.5\n`;
    }
    await writeFile(join(TEST_DIR, "extract", "cfg.toml"), toml);
    
    // CSV
    let csv = "id,name,score,active\n";
    for (let i = 0; i < 4000; i++) {
        csv += `${i},Model${i},${i / 13.0},true\n`;
    }
    await writeFile(join(TEST_DIR, "extract", "rows.csv"), csv);
    
    console.log("Fixtures created");
}

async function bench(name: string, iters: number, fn: () => Promise<void> | void): Promise<number> {
    // Warmup
    for (let i = 0; i < Math.min(5, iters); i++) {
        await fn();
    }
    
    const batches = 7;
    const samples: number[] = [];
    
    for (let b = 0; b < batches; b++) {
        const start = performance.now();
        for (let i = 0; i < iters; i++) {
            await fn();
        }
        const elapsed = performance.now() - start;
        samples.push((elapsed / iters) * 1_000_000); // ns/op
    }
    
    samples.sort((a, b) => a - b);
    const med = samples[samples.length / 2 | 0];
    console.log(`${name.padEnd(34)} ${med.toFixed(0).padStart(10)} ns/op   (${(med/1_000_000).toFixed(2)} ms/op @ ${iters} iters/batch)`);
    return med;
}

// ============================================================================
// Nanomite-style Operations (Native Node.js)
// ============================================================================

async function readFileSlice(path: string, offset: number, limit: number): Promise<string> {
    const handle = await import("node:fs/promises").then(fs => fs.open(path, "r"));
    try {
        const buffer = Buffer.alloc(limit);
        await handle.read(buffer, 0, limit, offset);
        return buffer.toString();
    } finally {
        await handle.close();
    }
}

async function searchFiles(query: string, dir: string): Promise<number> {
    let hits = 0;
    const files = await readdir(dir, { recursive: true });
    for (const file of files) {
        if (file.endsWith(".rs")) {
            const content = await readFile(join(dir, file), "utf-8");
            if (content.toLowerCase().includes(query.toLowerCase())) {
                hits++;
            }
        }
    }
    return hits;
}

async function grepSearch(query: string, dir: string): Promise<number> {
    // Using node's createReadStream for streaming grep (more memory efficient)
    let hits = 0;
    const files = await readdir(dir, { recursive: true });
    for (const file of files) {
        if (file.endsWith(".rs")) {
            const filePath = join(dir, file);
            const stream = createReadStream(filePath, { encoding: "utf-8" });
            const rl = createInterface({ input: stream });
            for await (const line of rl) {
                if (line.toLowerCase().includes(query.toLowerCase())) {
                    hits++;
                    break; // Count file as hit, not lines
                }
            }
        }
    }
    return hits;
}

async function writeFileTest(path: string, content: string): Promise<void> {
    await writeFile(path, content);
}

async function extractText(path: string): Promise<string> {
    return await readFile(path, "utf-8");
}

async function extractJson(path: string): Promise<object> {
    return JSON.parse(await readFile(path, "utf-8"));
}

async function extractToml(path: string): Promise<object> {
    // Simple TOML parse for benchmark (real impl would use @iarna/toml)
    return { parsed: (await readFile(path, "utf-8")).length };
}

async function extractCsv(path: string): Promise<string[][]> {
    const content = await readFile(path, "utf-8");
    return content.split("\n").map(line => line.split(","));
}

// ============================================================================
// Run Benchmarks
// ============================================================================

async function runBenchmarks() {
    console.log("=== Cross-Framework Benchmark: Pi-style vs Native ===\n");
    await setupFixtures();
    
    const bigFile = join(TEST_DIR, "big.rs");
    const srcDir = join(TEST_DIR, "src");
    const extractDir = join(TEST_DIR, "extract");
    
    // --- File Read ---
    console.log("-- File Read --");
    await bench("read_file/1.3MB slice 4KB", 20, () => readFileSlice(bigFile, 600_000, 4_096));
    
    // --- Search ---
    console.log("\n-- Search (300 files) --");
    await bench("search/300files hit (grep)", 3, () => grepSearch("model", srcDir));
    await bench("search/300files miss (grep)", 3, () => grepSearch("zzzqqqxxx", srcDir));
    
    // --- Extract ---
    console.log("\n-- Extract --");
    await bench("extract/text 270KB", 10, () => extractText(join(extractDir, "doc.txt")));
    await bench("extract/code 270KB", 10, () => extractText(join(extractDir, "code.rs")));
    await bench("extract/json ~160KB", 10, () => extractJson(join(extractDir, "data.json")));
    await bench("extract/toml ~20KB", 10, () => extractToml(join(extractDir, "cfg.toml")));
    await bench("extract/csv 4k rows", 10, () => extractCsv(join(extractDir, "rows.csv")));
    
    // --- Write ---
    console.log("\n-- Write --");
    const testWrite = join(TEST_DIR, "write_test.rs");
    await bench("write_file 270KB", 10, () => writeFileTest(testWrite, rustSource(1).repeat(9)));
    
    // Cleanup
    await rm(TEST_DIR, { recursive: true, force: true });
    
    console.log("\n=== Done ===");
}

runBenchmarks().catch(console.error);
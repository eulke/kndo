// kndo performance spike — validates the warm-run performance budget.
// Disposable by design; findings live in internal/spikes/0001-performance.md.
//
// Usage:
//   kndo-perf-spike gen <dir> <n-files>     generate a synthetic TS repo
//   kndo-perf-spike run <dir>               run all measurements against it

use rayon::prelude::*;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

const SYMS_PER_FILE: usize = 50;
const EDGES_PER_SYM: usize = 4;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("gen") => gen(Path::new(&args[2]), args[3].parse().unwrap()),
        Some("run") => run(Path::new(&args[2])),
        _ => eprintln!("usage: gen <dir> <n> | run <dir>"),
    }
}

// ---------------------------------------------------------------- synthetic repo

fn gen(root: &Path, n: usize) {
    // Deterministic pseudo-random file sizes/imports (no rand dep needed).
    let mut state = 0x9e3779b97f4a7c15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for i in 0..n {
        let dir = root.join(format!("src/mod{:02}/sub{:02}", i % 40, (i / 40) % 10));
        fs::create_dir_all(&dir).unwrap();
        let mut src = String::new();
        // ~8 imports per file, mostly local neighborhood (realistic locality)
        for k in 0..8 {
            let target = if k < 6 {
                i.saturating_sub((next() % 50) as usize + 1)
            } else {
                (next() % (n as u64)) as usize
            };
            let _ = writeln!(
                src,
                "import {{ f{}_0, T{} }} from '../../mod{:02}/sub{:02}/file{}';",
                target, target, target % 40, (target / 40) % 10, target
            );
        }
        let _ = writeln!(src, "import * as lib{} from 'lib{}';", i % 30, i % 30);
        // ~15 functions + types, some exported, realistic bodies
        for s in 0..15 {
            let _ = writeln!(
                src,
                "export interface T{i}_{s} {{ id: number; name: string; tags: string[]; }}"
            );
            let _ = writeln!(
                src,
                "export function f{i}_{s}(x: T{i}_{s}, n: number): number {{\n  let acc = 0;\n  for (let j = 0; j < n; j++) {{\n    if (x.tags.length > j && x.id % (j + 1) === 0) {{ acc += j * x.id; }} else {{ acc -= 1; }}\n  }}\n  const helper = (v: number) => v * 2 + x.name.length;\n  return acc > 0 ? helper(acc) : lib{}.deep?.value ?? f{i}_{s}Inner(acc);\n}}\nfunction f{i}_{s}Inner(v: number): number {{ return v < 0 ? -v : v; }}",
                i % 30
            );
        }
        fs::write(dir.join(format!("file{}.ts", i)), src).unwrap();
    }
    println!("generated {} files under {}", n, root.display());
}

// ---------------------------------------------------------------- measurements

fn run(root: &Path) {
    println!("cores: {}", rayon::current_num_threads());

    // -- discovery + hashing (cold: hash everything) ---------------------------
    let t = Instant::now();
    let files: Vec<PathBuf> = ignore::WalkBuilder::new(root)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.into_path())
        .collect();
    let discovery_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let contents: Vec<Vec<u8>> = files.par_iter().map(|p| fs::read(p).unwrap()).collect();
    let read_ms = t.elapsed().as_secs_f64() * 1e3;
    let total_bytes: usize = contents.iter().map(Vec::len).sum();

    let t = Instant::now();
    let hashes: Vec<blake3::Hash> =
        contents.par_iter().map(|c| blake3::hash(c)).collect();
    let hash_ms = t.elapsed().as_secs_f64() * 1e3;
    std::hint::black_box(&hashes);

    println!(
        "discovery: {} files | walk {:.1}ms | read {:.1}ms ({:.1} MB) | blake3 {:.1}ms",
        files.len(), discovery_ms, read_ms, total_bytes as f64 / 1e6, hash_ms
    );

    // -- extraction proxy: tree-sitter parse -----------------------------------
    let lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    let t = Instant::now();
    let node_counts: Vec<usize> = contents
        .par_iter()
        .map(|c| {
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&lang).unwrap();
            let tree = parser.parse(c.as_slice(), None).unwrap();
            // walk the tree to simulate fact extraction (visit every node)
            let mut count = 0usize;
            let mut cursor = tree.walk();
            let mut done = false;
            while !done {
                count += 1;
                if cursor.goto_first_child() { continue; }
                loop {
                    if cursor.goto_next_sibling() { break; }
                    if !cursor.goto_parent() { done = true; break; }
                }
            }
            count
        })
        .collect();
    let parse_all_ms = t.elapsed().as_secs_f64() * 1e3;
    let total_nodes: usize = node_counts.iter().sum();

    // warm-diff proxy: parse only 20 files, sequential (adaptive mode)
    let t = Instant::now();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&lang).unwrap();
    for c in contents.iter().take(20) {
        std::hint::black_box(parser.parse(c.as_slice(), None).unwrap());
    }
    let parse_20_ms = t.elapsed().as_secs_f64() * 1e3;

    println!(
        "parse: all {} files {:.1}ms ({:.1}k nodes visited) | 20-file warm diff {:.1}ms",
        contents.len(), parse_all_ms, total_nodes as f64 / 1e3, parse_20_ms
    );

    // -- graph: build CSR, persist, reload, BFS --------------------------------
    let n_files = files.len();
    let n_syms = n_files * SYMS_PER_FILE;
    let n_edges = n_syms * EDGES_PER_SYM;

    // deterministic synthetic edges with locality
    let t = Instant::now();
    let mut offsets = Vec::with_capacity(n_syms + 1);
    let mut targets = Vec::with_capacity(n_edges);
    let mut state = 0xdeadbeefcafef00du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    offsets.push(0u32);
    for s in 0..n_syms {
        for k in 0..EDGES_PER_SYM {
            let tgt = if k < 3 {
                (s + 1 + (next() % 2_000) as usize) % n_syms
            } else {
                (next() % n_syms as u64) as usize
            };
            targets.push(tgt as u32);
        }
        offsets.push(targets.len() as u32);
    }
    let build_ms = t.elapsed().as_secs_f64() * 1e3;

    // persist as flat little-endian u32 arrays (the rkyv/CSR persistence model)
    let cache = root.join(".spike-cache.bin");
    let t = Instant::now();
    let mut blob = Vec::with_capacity((offsets.len() + targets.len()) * 4 + 16);
    blob.extend_from_slice(&(offsets.len() as u64).to_le_bytes());
    blob.extend_from_slice(&(targets.len() as u64).to_le_bytes());
    for v in &offsets { blob.extend_from_slice(&v.to_le_bytes()); }
    for v in &targets { blob.extend_from_slice(&v.to_le_bytes()); }
    fs::write(&cache, &blob).unwrap();
    let persist_ms = t.elapsed().as_secs_f64() * 1e3;

    // reload via mmap (zero-copy path)
    let t = Instant::now();
    let file = fs::File::open(&cache).unwrap();
    let map = unsafe { memmap2::Mmap::map(&file).unwrap() };
    let n_off = u64::from_le_bytes(map[0..8].try_into().unwrap()) as usize;
    let n_tgt = u64::from_le_bytes(map[8..16].try_into().unwrap()) as usize;
    let off_bytes = &map[16..16 + n_off * 4];
    let tgt_bytes = &map[16 + n_off * 4..16 + (n_off + n_tgt) * 4];
    // force the pages in (worst case: touch every byte) and validate alignment-free reads
    let checksum: u64 = tgt_bytes.par_chunks(1 << 20).map(|c| c.iter().map(|&b| b as u64).sum::<u64>()).sum();
    let mmap_ms = t.elapsed().as_secs_f64() * 1e3;
    std::hint::black_box(checksum);

    // comparison point: bincode deserialize of the same data (owned Vec model)
    let t = Instant::now();
    let enc = bincode::serialize(&(offsets.clone(), targets.clone())).unwrap();
    let bincode_ser_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let (o2, t2): (Vec<u32>, Vec<u32>) = bincode::deserialize(&enc).unwrap();
    let bincode_de_ms = t.elapsed().as_secs_f64() * 1e3;
    std::hint::black_box((o2.len(), t2.len()));

    println!(
        "graph: {}k syms, {:.1}M edges | build {:.1}ms | persist {:.1}ms ({:.1} MB) | mmap+touch {:.1}ms | bincode ser {:.1}ms / de {:.1}ms",
        n_syms / 1_000, n_edges as f64 / 1e6, build_ms, persist_ms,
        blob.len() as f64 / 1e6, mmap_ms, bincode_ser_ms, bincode_de_ms
    );

    // -- reachability: full BFS + dirty-region BFS (reads through the mmap) ----
    let get_off = |i: usize| u32::from_le_bytes(off_bytes[i * 4..i * 4 + 4].try_into().unwrap()) as usize;
    let get_tgt = |i: usize| u32::from_le_bytes(tgt_bytes[i * 4..i * 4 + 4].try_into().unwrap()) as usize;

    let bfs = |roots: &[usize]| -> (usize, f64) {
        let t = Instant::now();
        let mut color = vec![false; n_syms];
        let mut frontier: Vec<usize> = roots.to_vec();
        for &r in roots { color[r] = true; }
        let mut visited = roots.len();
        while !frontier.is_empty() {
            let mut nextf = Vec::with_capacity(frontier.len() * 2);
            for &s in &frontier {
                for e in get_off(s)..get_off(s + 1) {
                    let t2 = get_tgt(e);
                    if !color[t2] { color[t2] = true; visited += 1; nextf.push(t2); }
                }
            }
            frontier = nextf;
        }
        (visited, t.elapsed().as_secs_f64() * 1e3)
    };

    let roots: Vec<usize> = (0..n_syms).step_by(20).collect(); // 5% roots
    let (visited, full_bfs_ms) = bfs(&roots);
    // run the production-color pass twice (prod roots, then all roots) = worst case full analysis
    let (_, full_bfs2_ms) = bfs(&roots);

    // dirty region: 200 changed symbols + reverse closure approx (re-color small region)
    let dirty: Vec<usize> = (1000..1200).collect();
    let (dvisited, dirty_ms) = bfs(&dirty);

    println!(
        "reachability: full BFS {:.1}ms ({}k reached) | second pass {:.1}ms | dirty-region(200) {:.1}ms ({}k touched)",
        full_bfs_ms, visited / 1_000, full_bfs2_ms, dirty_ms, dvisited / 1_000
    );

    // -- warm-run composite ----------------------------------------------------
    // stat-scan proxy: re-walk + metadata check (no rehash), then the warm pieces
    let t = Instant::now();
    let stat_count = ignore::WalkBuilder::new(root)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .filter(|e| e.metadata().is_ok())
        .count();
    let stat_ms = t.elapsed().as_secs_f64() * 1e3;
    std::hint::black_box(stat_count);

    let warm_total = stat_ms + parse_20_ms + mmap_ms + dirty_ms;
    println!(
        "WARM COMPOSITE: stat-scan {:.1}ms + parse(20) {:.1}ms + graph-load {:.1}ms + dirty-BFS {:.1}ms = {:.1}ms",
        stat_ms, parse_20_ms, mmap_ms, dirty_ms, warm_total
    );
    println!(
        "COLD COMPOSITE: walk {:.1} + read {:.1} + hash {:.1} + parse {:.1} + build {:.1} + persist {:.1} + 2×BFS {:.1} = {:.1}ms",
        discovery_ms, read_ms, hash_ms, parse_all_ms, build_ms, persist_ms,
        full_bfs_ms + full_bfs2_ms,
        discovery_ms + read_ms + hash_ms + parse_all_ms + build_ms + persist_ms + full_bfs_ms + full_bfs2_ms
    );
    let _ = fs::remove_file(cache);
}

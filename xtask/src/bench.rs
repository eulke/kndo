//! `cargo xtask bench` — the benchmark suite: fixture repos at 1k / 5k / 50k files,
//! the five scenarios (cold full, warm no-op, warm 1-file change, warm 100-file change,
//! `--staged`), a recorded baseline, and the >10% regression gate.
//!
//! Fixtures are **generated, deterministic, and disposable** (`target/bench-fixtures/`):
//! pure-arithmetic synthetic TypeScript — import chains reachable from `index.ts`, one dead
//! file per decade, exported functions with real branchy bodies (so extraction, metrics, and
//! every analysis do real work), one clone pair per 500 files. Nothing random: same
//! generator version ⇒ byte-identical fixture. Each fixture is a git repo (one initial
//! commit) so the `--staged` scenario runs the real diff path; the suite resets the tree
//! (`git checkout -- .`) before measuring, so mutation scenarios never accumulate across
//! invocations.
//!
//! Measurement is **end-to-end wall time** of the real release binary (`kndo check --format
//! json > /dev/null`) — process start, discovery, analysis, render, print; the user's
//! latency, not a flattering subset. Warm scenarios take the **minimum of N runs** (noise on
//! a shared machine only ever adds time; the minimum is the closest observable to the true
//! cost), cold takes the min of 2.
//!
//! The baseline (`xtask/perf-baseline.json`) is machine-specific by nature — it records where
//! it was measured and is re-recorded with `--update-baseline` when hardware changes. A
//! scenario counts as regressed when it is BOTH >10% and >10 ms over baseline (the absolute
//! floor keeps micro-scenario jitter from tripping the relative gate). `--gate` turns
//! regressions into a failing exit — the CI-blocking mode; without it
//! the suite reports and exits clean (exploration mode).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Bump when the generator's output changes — a mismatch regenerates the fixture.
const GENERATOR_VERSION: u32 = 1;
const WARM_REPS: usize = 5;
const COLD_REPS: usize = 2;
/// Regression gate: both thresholds must trip (the 10% relative bound, plus an absolute floor
/// so a 3 ms scenario can't fail the build over 0.4 ms of jitter).
const GATE_RELATIVE: f64 = 0.10;
const GATE_ABSOLUTE_MS: f64 = 10.0;

const SIZES: &[(&str, usize)] = &[("1k", 1_000), ("5k", 5_000), ("50k", 50_000)];

pub fn run(args: &[String]) -> Result<(), String> {
    let update_baseline = args.iter().any(|a| a == "--update-baseline");
    let gate = args.iter().any(|a| a == "--gate");
    let sizes: Vec<&(&str, usize)> = match args.iter().position(|a| a == "--sizes") {
        Some(i) => {
            let list = args
                .get(i + 1)
                .ok_or_else(|| "--sizes needs a value (e.g. 1k,5k)".to_string())?;
            SIZES
                .iter()
                .filter(|(name, _)| list.split(',').any(|s| s == *name))
                .collect()
        }
        None => SIZES.iter().collect(),
    };
    if sizes.is_empty() {
        return Err("no known sizes selected (known: 1k, 5k, 50k)".to_string());
    }

    let root = super::workspace_root()?;
    let kndo = build_release_kndo(&root)?;

    let mut results: Vec<(String, f64)> = Vec::new();
    for &&(name, files) in &sizes {
        let dir = fixture_dir(&root, name);
        ensure_fixture(&dir, files)?;
        reset_fixture(&dir);
        eprintln!("xtask bench: {name} ({files} files) at {}", dir.display());

        // Cold full: no cache at all.
        let cold = min_of(COLD_REPS, || {
            let _ = std::fs::remove_dir_all(dir.join(".kndo"));
            time_check(&kndo, &dir, &[])
        })?;
        results.push((format!("{name}/cold-full"), cold));

        // Warm no-op: settle once (writes the snapshot), then measure pure hits.
        time_check(&kndo, &dir, &[])?;
        let noop = min_of(WARM_REPS, || time_check(&kndo, &dir, &[]))?;
        results.push((format!("{name}/warm-noop"), noop));

        // Warm 1-file change: each rep appends to one file — every run is a genuine
        // 1-file-changed-since-last-run invocation.
        let one = min_of(WARM_REPS, || {
            touch_files(&dir, 1)?;
            time_check(&kndo, &dir, &[])
        })?;
        results.push((format!("{name}/warm-1-file"), one));

        // Warm 100-file change.
        let hundred = min_of(3, || {
            touch_files(&dir, 100)?;
            time_check(&kndo, &dir, &[])
        })?;
        results.push((format!("{name}/warm-100-file"), hundred));

        // --staged on a realistic staged diff (10 files).
        reset_fixture(&dir);
        time_check(&kndo, &dir, &[])?; // re-warm after reset
        touch_files(&dir, 10)?;
        git(&dir, &["add", "-A"])?;
        let staged = min_of(3, || time_check(&kndo, &dir, &["--staged"]))?;
        results.push((format!("{name}/staged"), staged));
        reset_fixture(&dir);
    }

    let baseline_path = root.join("xtask/perf-baseline.json");
    let baseline = load_baseline(&baseline_path);
    let mut report = String::new();
    let _ = writeln!(
        report,
        "{:<22} {:>10} {:>10} {:>8}",
        "scenario", "now", "baseline", "delta"
    );
    let mut regressions = Vec::new();
    for (scenario, ms) in &results {
        match baseline.as_ref().and_then(|b| b.get(scenario).copied()) {
            Some(base) => {
                let delta = ms - base;
                let pct = if base > 0.0 {
                    delta / base * 100.0
                } else {
                    0.0
                };
                let regressed = delta > GATE_ABSOLUTE_MS && delta / base > GATE_RELATIVE;
                if regressed {
                    regressions.push(format!(
                        "{scenario}: {ms:.1} ms vs baseline {base:.1} ms (+{pct:.0}%)"
                    ));
                }
                let _ = writeln!(
                    report,
                    "{:<22} {:>8.1}ms {:>8.1}ms {:>+7.0}%{}",
                    scenario,
                    ms,
                    base,
                    pct,
                    if regressed { "  REGRESSED" } else { "" }
                );
            }
            None => {
                let _ = writeln!(
                    report,
                    "{:<22} {:>8.1}ms {:>10} {:>8}",
                    scenario, ms, "—", "—"
                );
            }
        }
    }
    print!("{report}");

    if update_baseline {
        write_baseline(&baseline_path, &results)?;
        println!("baseline updated: {}", baseline_path.display());
        return Ok(());
    }
    if baseline.is_none() {
        println!(
            "no baseline at {} — record one with `cargo xtask bench --update-baseline`",
            baseline_path.display()
        );
        return Ok(());
    }
    if !regressions.is_empty() {
        for r in &regressions {
            eprintln!("xtask bench: REGRESSION {r}");
        }
        if gate {
            return Err(format!(
                "{} scenario(s) regressed past the benchmark gate",
                regressions.len()
            ));
        }
        eprintln!("xtask bench: (informational — pass --gate to make this fail the build)");
    }
    Ok(())
}

fn build_release_kndo(root: &Path) -> Result<PathBuf, String> {
    eprintln!("xtask bench: building release kndo…");
    let status = Command::new("cargo")
        .args(["build", "--release", "-p", "kndo-cli"])
        .current_dir(root)
        .status()
        .map_err(|e| format!("cargo build failed to start: {e}"))?;
    if !status.success() {
        return Err("cargo build --release -p kndo-cli failed".to_string());
    }
    Ok(root.join("target/release/kndo"))
}

fn fixture_dir(root: &Path, name: &str) -> PathBuf {
    root.join("target/bench-fixtures").join(name)
}

fn min_of(reps: usize, mut f: impl FnMut() -> Result<f64, String>) -> Result<f64, String> {
    let mut best = f64::INFINITY;
    for _ in 0..reps {
        best = best.min(f()?);
    }
    Ok(best)
}

fn time_check(kndo: &Path, dir: &Path, extra: &[&str]) -> Result<f64, String> {
    let start = Instant::now();
    let output = Command::new(kndo)
        .arg("check")
        .args(extra)
        .args(["--format", "json"])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("failed to run kndo: {e}"))?;
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    // Exit 0 (clean) and 1 (findings) are both successful analyses; anything else is a
    // broken run whose timing would be a lie.
    match output.status.code() {
        Some(0 | 1) => Ok(elapsed),
        code => Err(format!(
            "kndo check exited with {code:?} in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr)
        )),
    }
}

fn touch_files(dir: &Path, count: usize) -> Result<(), String> {
    use std::io::Write as _;
    for i in 0..count {
        // Deterministic spread across shards; appending a comment is a real content change
        // (new hash) with no semantic effect on the fixture's finding profile.
        let shard = (i * 37) % 10;
        let file = dir.join(format!("src/d{shard}/mod{}.ts", shard * 100 + (i % 100)));
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .map_err(|e| format!("cannot touch {}: {e}", file.display()))?;
        writeln!(f, "// touched").map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn reset_fixture(dir: &Path) {
    let _ = git(dir, &["checkout", "--", "."]);
    let _ = git(dir, &["reset", "-q"]);
}

fn git(dir: &Path, args: &[&str]) -> Result<(), String> {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|e| format!("git failed to start: {e}"))?;
    if !status.success() {
        return Err(format!("git {args:?} failed in {}", dir.display()));
    }
    Ok(())
}

/// Deterministic synthetic repo: `files` TypeScript modules in 100-file shards. Per decade of
/// ten modules: nine chain into each other (importable work for resolution + reachability),
/// the tenth is dead (real `unused` findings); decade heads are imported by `index.ts` (the
/// manifest root), so most of the tree is production-reachable. Every module exports one
/// branchy function (cyclomatic 4) so metrics/winnowing/crap do real work; every 500th pair
/// of modules shares a function body shape (structural-duplicate work). Pure arithmetic —
/// no randomness, no timestamps in content.
fn ensure_fixture(dir: &Path, files: usize) -> Result<(), String> {
    let marker = dir.join(".bench-fixture-version");
    if let Ok(v) = std::fs::read_to_string(&marker) {
        if v.trim() == GENERATOR_VERSION.to_string() && dir.join(".git").exists() {
            return Ok(());
        }
    }
    eprintln!("xtask bench: generating {files}-file fixture…");
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir.join("src")).map_err(|e| e.to_string())?;

    std::fs::write(
        dir.join("package.json"),
        format!(
            "{{\"name\": \"bench-{files}\", \"version\": \"1.0.0\", \"main\": \"src/index.ts\"}}\n"
        ),
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(dir.join(".gitignore"), ".kndo/\n").map_err(|e| e.to_string())?;

    let mut index = String::new();
    let mut index_calls = String::from("export function main(): number {\n  let total = 0;\n");
    for i in 0..files {
        let shard = i / 100;
        let shard_dir = dir.join(format!("src/d{shard}"));
        if i % 100 == 0 {
            std::fs::create_dir_all(&shard_dir).map_err(|e| e.to_string())?;
        }
        let mut content = String::new();
        // Chain within the decade: mod i imports mod i-1 unless it's a decade head; the
        // ninth of each decade (i % 10 == 9) is imported by nobody — deterministic dead code.
        if i % 10 != 0 && (i - 1) % 10 != 9 {
            let j = i - 1;
            let _ = writeln!(content, "import {{ fn{j} }} from '../d{}/mod{j}';", j / 100);
            let _ = writeln!(
                content,
                "export function chained{i}(x: number): number {{ return fn{j}(x) + {i}; }}"
            );
        }
        // The clone family: every 500th module repeats the same body shape (different
        // identifiers — a Type-2 clone).
        let body = if i % 500 == 250 {
            format!(
                "export function fn{i}(a: number, b: number): number {{\n  let acc = 0;\n  for (let k = 0; k < a; k += 1) {{\n    if (k % 2 === 0 && k > b) {{\n      acc += k * 2;\n    }} else if (k % 3 === 0) {{\n      acc -= k;\n    }}\n  }}\n  if (acc < 0) {{\n    return -acc;\n  }}\n  return acc + a + b;\n}}\n"
            )
        } else {
            format!(
                "export function fn{i}(x: number): number {{\n  if (x > {r}) {{\n    return x + {i};\n  }}\n  let s = 0;\n  for (let k = 0; k < x; k += 1) {{\n    s += k % 2 === 0 ? k : -k;\n  }}\n  return s;\n}}\n",
                r = i % 7
            )
        };
        content.push_str(&body);
        std::fs::write(shard_dir.join(format!("mod{i}.ts")), content).map_err(|e| e.to_string())?;

        if i % 10 == 0 {
            let _ = writeln!(index, "import {{ fn{i} }} from './d{}/mod{i}';", i / 100);
            let _ = writeln!(index_calls, "  total += fn{i}({});", i % 13);
        }
    }
    index_calls.push_str("  return total;\n}\n");
    index.push('\n');
    index.push_str(&index_calls);
    std::fs::write(dir.join("src/index.ts"), index).map_err(|e| e.to_string())?;

    git(dir, &["init", "-q"])?;
    git(dir, &["add", "-A"])?;
    let status = Command::new("git")
        .args([
            "-c",
            "user.email=bench@kndo",
            "-c",
            "user.name=bench",
            "commit",
            "-qm",
            "fixture",
        ])
        .current_dir(dir)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("git commit failed in fixture".to_string());
    }
    std::fs::write(&marker, GENERATOR_VERSION.to_string()).map_err(|e| e.to_string())?;
    Ok(())
}

fn load_baseline(path: &Path) -> Option<std::collections::BTreeMap<String, f64>> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let scenarios = v.get("scenarios")?.as_object()?;
    Some(
        scenarios
            .iter()
            .filter_map(|(k, v)| v.as_f64().map(|ms| (k.clone(), ms)))
            .collect(),
    )
}

fn write_baseline(path: &Path, results: &[(String, f64)]) -> Result<(), String> {
    let scenarios: serde_json::Map<String, serde_json::Value> = results
        .iter()
        .map(|(k, ms)| {
            (
                k.clone(),
                serde_json::Value::from((ms * 10.0).round() / 10.0),
            )
        })
        .collect();
    let doc = serde_json::json!({
        "comment": "Benchmark baseline — end-to-end wall ms, min-of-N, release build. Machine-specific: re-record with `cargo xtask bench --update-baseline` when the measuring machine changes; the gate compares same-machine runs only.",
        "generator_version": GENERATOR_VERSION,
        "scenarios": scenarios,
    });
    std::fs::write(
        path,
        serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| e.to_string())
}

//! `evals` — eval harness for the `zap` agent.
//!
//! Runs a catalogue of task definitions (`evals/tasks/*.json`) against a
//! compiled `zap` binary in SDK mode, runs each task's `check` script to
//! decide pass/fail, and produces a pass-rate + cost summary plus a results
//! JSON file under `evals/results/<timestamp>.json`.
//!
//! See `evals/README.md` for the task schema, how to run, and how to add
//! new tasks.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_TASKS_DIR: &str = "evals/tasks";
const DEFAULT_RESULTS_DIR: &str = "evals/results";

#[derive(Debug, Deserialize)]
struct TaskDef {
    id: String,
    #[serde(default)]
    description: String,
    prompt: String,
    /// Optional shell script run inside the temp project dir before the
    /// agent starts. Use to scaffold files the agent will edit / read.
    #[serde(default)]
    setup: String,
    /// Shell script run after the agent finishes. Non-zero exit = fail.
    check: String,
    #[serde(default = "default_timeout")]
    timeout_secs: u64,
    /// Currently advisory — `MAX_TURNS` in the agent caps the inner loop.
    /// Kept here so future runners can pass it through.
    #[serde(default = "default_max_turns")]
    #[allow(dead_code)]
    max_turns: u32,
    /// If set, skip the task unless this binary is on `PATH`. Used by
    /// language-specific tasks (python, node, etc.) so the harness stays
    /// hermetic on machines without that toolchain.
    #[serde(default)]
    requires: Vec<String>,
    /// Optional tag — "edit", "search", "shell", "refusal", etc. — for
    /// per-category reporting.
    #[serde(default)]
    category: String,
}

fn default_timeout() -> u64 { 180 }
fn default_max_turns() -> u32 { 20 }

#[derive(Debug, Serialize)]
struct TaskResult {
    id: String,
    category: String,
    pass: bool,
    skipped: bool,
    skip_reason: Option<String>,
    turns: u32,
    input_tokens: u64,
    output_tokens: u64,
    est_cost_usd: f64,
    wall_secs: f64,
    /// Empty on success; one-line summary of why it failed on failure.
    error: Option<String>,
}

#[derive(Debug)]
struct Args {
    zap_bin: PathBuf,
    tasks_dir: PathBuf,
    results_dir: PathBuf,
    model: Option<String>,
    list_only: bool,
    only_ids: Vec<String>,
    no_color: bool,
}

fn print_help() {
    println!("evals — eval harness for zap

USAGE:
    cargo run --release --bin evals -- [OPTIONS]

OPTIONS:
    --zap-bin <path>     Path to the compiled zap binary (default: target/release/zap)
    --tasks-dir <path>   Directory of *.json task files (default: evals/tasks)
    --results-dir <path> Output directory for results JSON (default: evals/results)
    --model <name>       Optional model name to record in results (zap reads its own
                         config from ~/.agent.toml; this is for reporting + cost calc).
    --list               Dry-run — print task IDs and skip reasons; never spawn zap.
    --only <id,id,...>   Run only the named tasks (comma-separated).
    --no-color           Don't colorize terminal output.
    --help, -h           Show this help.

EXIT:
    0 if all non-skipped tasks pass, 1 otherwise.
");
}

fn parse_args() -> Result<Args, String> {
    let mut zap_bin = PathBuf::from("target/release/zap");
    let mut tasks_dir = PathBuf::from(DEFAULT_TASKS_DIR);
    let mut results_dir = PathBuf::from(DEFAULT_RESULTS_DIR);
    let mut model: Option<String> = None;
    let mut list_only = false;
    let mut only_ids: Vec<String> = Vec::new();
    let mut no_color = false;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--help" | "-h" => { print_help(); std::process::exit(0); }
            "--list" => list_only = true,
            "--no-color" => no_color = true,
            "--zap-bin" => zap_bin = PathBuf::from(args.next().ok_or("--zap-bin needs value")?),
            "--tasks-dir" => tasks_dir = PathBuf::from(args.next().ok_or("--tasks-dir needs value")?),
            "--results-dir" => results_dir = PathBuf::from(args.next().ok_or("--results-dir needs value")?),
            "--model" => model = Some(args.next().ok_or("--model needs value")?),
            "--only" => {
                let v = args.next().ok_or("--only needs value")?;
                only_ids = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            }
            _ => return Err(format!("unknown arg: {a}")),
        }
    }

    Ok(Args { zap_bin, tasks_dir, results_dir, model, list_only, only_ids, no_color })
}

fn load_tasks(dir: &Path) -> Result<Vec<TaskDef>, String> {
    let entries = fs::read_dir(dir)
        .map_err(|e| format!("tasks dir {}: {e}", dir.display()))?;
    let mut tasks = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") { continue; }
        let raw = fs::read_to_string(&path)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        let task: TaskDef = serde_json::from_str(&raw)
            .map_err(|e| format!("parse {}: {e}", path.display()))?;
        tasks.push(task);
    }
    tasks.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(tasks)
}

fn binary_on_path(name: &str) -> bool {
    Command::new(name).arg("--version").stdout(Stdio::null()).stderr(Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)
}

/// USD per million tokens. Kept inline so this binary stays standalone.
fn cost_per_million(model: &str) -> (f64, f64) {
    let m = model.to_lowercase();
    if m.contains("opus-4") { (15.0, 75.0) }
    else if m.contains("sonnet-4") { (3.0, 15.0) }
    else if m.contains("haiku-4") { (0.8, 4.0) }
    else if m.contains("gpt-4o") { (5.0, 15.0) }
    else if m.contains("deepseek") { (0.27, 1.10) }
    else { (0.0, 0.0) }
}

/// One stdout-line event from the SDK.
#[derive(Default, Debug)]
struct SdkAccum {
    last_turn: u32,
    input_tokens: u64,
    output_tokens: u64,
    saw_error: Option<String>,
}

fn parse_sdk_stdout(stdout: &str) -> SdkAccum {
    let mut acc = SdkAccum::default();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') { continue; }
        let Ok(v): Result<Value, _> = serde_json::from_str(trimmed) else { continue };
        match v["type"].as_str() {
            Some("assistant") => {
                if let Some(t) = v["turn"].as_u64() { acc.last_turn = t as u32; }
                if let Some(u) = v["usage"].as_object() {
                    acc.input_tokens = u.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
                    acc.output_tokens = u.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
                }
            }
            Some("error") => {
                acc.saw_error = Some(v["message"].as_str().unwrap_or("").to_string());
            }
            _ => {}
        }
    }
    acc
}

fn run_shell(script: &str, cwd: &Path, timeout: Duration) -> Result<std::process::Output, String> {
    let mut child = Command::new("bash")
        .arg("-eu")
        .arg("-o").arg("pipefail")
        .arg("-c").arg(script)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn bash: {e}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                return Err(format!("timeout after {:?}", timeout));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    child.wait_with_output().map_err(|e| e.to_string())
}

/// Run one task end-to-end. Returns the structured result.
fn run_task(task: &TaskDef, args: &Args) -> TaskResult {
    let mut res = TaskResult {
        id: task.id.clone(),
        category: task.category.clone(),
        pass: false,
        skipped: false,
        skip_reason: None,
        turns: 0,
        input_tokens: 0,
        output_tokens: 0,
        est_cost_usd: 0.0,
        wall_secs: 0.0,
        error: None,
    };

    // Skip if required binaries aren't installed.
    for req in &task.requires {
        if !binary_on_path(req) {
            res.skipped = true;
            res.skip_reason = Some(format!("missing tool on PATH: {req}"));
            return res;
        }
    }

    // Set up the workspace in a temp dir.
    let tmp = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            res.error = Some(format!("tempdir: {e}"));
            return res;
        }
    };

    if !task.setup.trim().is_empty() {
        match run_shell(&task.setup, tmp.path(), Duration::from_secs(60)) {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                res.error = Some(format!(
                    "setup failed (exit {}): {}",
                    out.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or(""),
                ));
                return res;
            }
            Err(e) => { res.error = Some(format!("setup: {e}")); return res; }
        }
    }

    // Spawn zap in SDK + auto mode, scoped to the temp dir.
    let started = Instant::now();
    let mut child = match Command::new(&args.zap_bin)
        .args(["--sdk", "--auto"])
        .current_dir(tmp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            res.error = Some(format!("spawn zap ({}): {e}", args.zap_bin.display()));
            return res;
        }
    };

    {
        let stdin = match child.stdin.as_mut() {
            Some(s) => s,
            None => { res.error = Some("zap stdin not captured".into()); return res; }
        };
        let user_msg = serde_json::json!({"type":"user","text": task.prompt});
        let quit_msg = serde_json::json!({"type":"quit"});
        let _ = writeln!(stdin, "{}", user_msg);
        let _ = writeln!(stdin, "{}", quit_msg);
    }

    let deadline = Instant::now() + Duration::from_secs(task.timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                res.error = Some(format!("zap timed out after {}s", task.timeout_secs));
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => { res.error = Some(format!("wait: {e}")); break; }
        }
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => { res.error = Some(format!("wait_with_output: {e}")); return res; }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let acc = parse_sdk_stdout(&stdout);
    res.turns = acc.last_turn;
    res.input_tokens = acc.input_tokens;
    res.output_tokens = acc.output_tokens;
    if let Some(msg) = &acc.saw_error {
        res.error = Some(format!("agent error: {msg}"));
    }

    let model_for_cost = args.model.as_deref().unwrap_or("");
    let (cin, cout) = cost_per_million(model_for_cost);
    res.est_cost_usd = (acc.input_tokens as f64 * cin + acc.output_tokens as f64 * cout) / 1_000_000.0;
    res.wall_secs = started.elapsed().as_secs_f64();

    // Run the check.
    if res.error.is_some() && acc.last_turn == 0 {
        // No agent activity — leave the previous error in place.
        return res;
    }
    match run_shell(&task.check, tmp.path(), Duration::from_secs(60)) {
        Ok(out) if out.status.success() => { res.pass = true; res.error = None; }
        Ok(out) => {
            let detail = String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("").to_string();
            res.error = Some(format!("check failed (exit {}): {}",
                out.status.code().unwrap_or(-1), detail));
        }
        Err(e) => { res.error = Some(format!("check: {e}")); }
    }

    res
}

fn print_summary(results: &[TaskResult], no_color: bool) {
    let ran: Vec<&TaskResult> = results.iter().filter(|r| !r.skipped).collect();
    let passed = ran.iter().filter(|r| r.pass).count();
    let total_ran = ran.len();
    let skipped = results.len() - total_ran;
    let total_in: u64 = ran.iter().map(|r| r.input_tokens).sum();
    let total_out: u64 = ran.iter().map(|r| r.output_tokens).sum();
    let total_cost: f64 = ran.iter().map(|r| r.est_cost_usd).sum();
    let total_wall: f64 = ran.iter().map(|r| r.wall_secs).sum();

    let by_cat: BTreeMap<&str, (usize, usize)> = ran.iter().fold(BTreeMap::new(), |mut m, r| {
        let cat = if r.category.is_empty() { "uncategorized" } else { r.category.as_str() };
        let e = m.entry(cat).or_insert((0, 0));
        e.0 += if r.pass { 1 } else { 0 };
        e.1 += 1;
        m
    });

    let dim = |s: &str| if no_color { s.to_string() } else { format!("\x1b[2m{s}\x1b[0m") };
    let bold = |s: &str| if no_color { s.to_string() } else { format!("\x1b[1m{s}\x1b[0m") };
    let green = |s: &str| if no_color { s.to_string() } else { format!("\x1b[32m{s}\x1b[0m") };
    let red = |s: &str| if no_color { s.to_string() } else { format!("\x1b[31m{s}\x1b[0m") };
    let yellow = |s: &str| if no_color { s.to_string() } else { format!("\x1b[33m{s}\x1b[0m") };

    println!();
    println!("{}", bold("┌── Eval Results ──────────────────────────────────────────────"));
    println!("│");
    println!("│  {} {}/{} tasks passed   {} skipped",
        bold("Pass rate:"), passed, total_ran, skipped);
    println!("│  {} ~{:.4} USD   ({} in, {} out tokens)",
        bold("Cost:"), total_cost, total_in, total_out);
    println!("│  {} {:.1}s total wall time", bold("Time:"), total_wall);
    println!("│");
    if !by_cat.is_empty() {
        println!("│  {}", dim("By category:"));
        for (cat, (p, t)) in &by_cat {
            println!("│    {:<18} {}/{}", cat, p, t);
        }
        println!("│");
    }
    println!("│  {}", dim("Per task:"));
    println!("│    {:<28} {:>6} {:>6} {:>10}  {:<6}  detail",
        "id", "pass", "turns", "tokens", "wall");
    for r in results {
        if r.skipped {
            println!("│    {:<28} {} skip — {}", r.id, yellow("SKIP"),
                r.skip_reason.as_deref().unwrap_or(""));
            continue;
        }
        let status = if r.pass { green(" PASS") } else { red(" FAIL") };
        let detail = r.error.as_deref().unwrap_or("");
        println!("│    {:<28} {}  {:>5}  {:>9}  {:>5.1}s  {}",
            r.id, status, r.turns,
            r.input_tokens + r.output_tokens,
            r.wall_secs,
            detail.chars().take(40).collect::<String>(),
        );
    }
    println!("│");
    println!("{}", bold("└──────────────────────────────────────────────────────────────"));
}

fn write_results(results: &[TaskResult], dir: &Path, model: Option<&str>) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let path = dir.join(format!("{ts}.json"));
    let body = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "model": model,
        "results": results,
    });
    fs::write(&path, serde_json::to_string_pretty(&body).unwrap_or_default())
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => { eprintln!("error: {e}"); std::process::exit(2); }
    };

    let mut tasks = match load_tasks(&args.tasks_dir) {
        Ok(t) => t,
        Err(e) => { eprintln!("error: {e}"); std::process::exit(2); }
    };

    if !args.only_ids.is_empty() {
        tasks.retain(|t| args.only_ids.contains(&t.id));
    }

    if tasks.is_empty() {
        eprintln!("no tasks matched.");
        std::process::exit(2);
    }

    if args.list_only {
        println!("{} task(s):", tasks.len());
        for t in &tasks {
            let mut tags = Vec::new();
            if !t.category.is_empty() { tags.push(t.category.clone()); }
            for r in &t.requires {
                let ok = if binary_on_path(r) { "✓" } else { "✗" };
                tags.push(format!("needs {} {}", r, ok));
            }
            let tags = if tags.is_empty() { String::new() } else { format!("  [{}]", tags.join(", ")) };
            println!("  - {:<28} {}{}",
                t.id,
                if t.description.is_empty() { &t.prompt } else { &t.description },
                tags);
        }
        return;
    }

    if !args.zap_bin.exists() {
        eprintln!("zap binary not found at {}. Build with `cargo build --release` or pass --zap-bin.",
            args.zap_bin.display());
        std::process::exit(2);
    }

    let mut results = Vec::with_capacity(tasks.len());
    for task in &tasks {
        eprintln!("→ {} …", task.id);
        let r = run_task(task, &args);
        results.push(r);
    }

    match write_results(&results, &args.results_dir, args.model.as_deref()) {
        Ok(p) => eprintln!("results written: {}", p.display()),
        Err(e) => eprintln!("warning: results write failed: {e}"),
    }

    print_summary(&results, args.no_color);

    let any_failed = results.iter().any(|r| !r.skipped && !r.pass);
    std::process::exit(if any_failed { 1 } else { 0 });
}

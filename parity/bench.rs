//! Measure Examen 0.3.0 against the Relatum release binary on one machine.
//!
//! Prints a markdown table of medians. Exits 1 if any Rust median is not lower.
//! The memory row is peak resident memory: VmHWM from /proc on Linux, the
//! peak working set on Windows, ru_maxrss from wait4 on macOS. On Windows,
//! point PARITY_PYTHON at a real interpreter: a venv's python.exe is a
//! launcher that runs Python in a second process.
//!
//! Usage: cargo run --release --locked --example bench -- RELATUM RELATUM_MCP

#[path = "common.rs"]
mod common;

use common::{extract_legacy, fail, mkdtemp, path_str, python, write_text};
use relatum::pystr::py_list;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

const RUNS_STARTUP: usize = 11;
const RUNS_SMALL: usize = 9;
const RUNS_LARGE: usize = 7;
const RUNS_BIG: usize = 5;
const RUNS_MEMORY: usize = 5;

/// A program and its arguments, as the Python list the old script printed.
#[derive(Clone)]
struct Cmd(Vec<String>);

impl Cmd {
    fn with(&self, args: &[&str]) -> Cmd {
        let mut parts = self.0.clone();
        parts.extend(args.iter().map(|arg| arg.to_string()));
        Cmd(parts)
    }

    fn command(&self, cwd: &Path, env: &[(String, String)]) -> Command {
        let mut cmd = Command::new(&self.0[0]);
        cmd.args(&self.0[1..]).current_dir(cwd).env_clear().envs(env.iter().cloned());
        cmd
    }

    fn repr(&self) -> String {
        py_list(&self.0)
    }
}

struct Row {
    name: String,
    py: f64,
    rs: f64,
    unit: &'static str,
}

fn note(i: usize) -> String {
    format!(
        "---\nkind: decision\nsummary: bench note {i}\nauthor: bench\nupdated: 2026-10-02\ntags: [bench]\n---\n\nBody of note {i}. A short fact for the checker.\n"
    )
}

fn make_store(root: &Path, count: usize) {
    let shared = root.join("_shared");
    fs::create_dir_all(&shared).unwrap_or_else(|err| fail(&err.to_string()));
    write_text(&shared.join("report-format.md"), &note(0));
    for i in 0..count {
        let bucket = root.join(format!("b{}", i / 100));
        if !bucket.is_dir() {
            fs::create_dir(&bucket).unwrap_or_else(|err| fail(&err.to_string()));
        }
        write_text(&bucket.join(format!("n{i}.md")), &note(i + 1));
    }
}

/// `statistics.median`: the middle value, or the mean of the two middle values.
fn median(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        fail("no median for empty data");
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[mid]
    } else {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    }
}

fn ms_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

/// Run to completion with output discarded. Returns the exit code.
fn run_quiet(cmd: &Cmd, cwd: &Path, env: &[(String, String)]) -> i32 {
    cmd.command(cwd, env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap_or_else(|err| fail(&format!("{}: {err}", cmd.repr())))
        .code()
        .unwrap_or(1)
}

fn time_call(cmd: &Cmd, cwd: &Path, env: &[(String, String)], runs: usize) -> f64 {
    let mut samples = Vec::new();
    for i in 0..runs {
        let start = Instant::now();
        let code = run_quiet(cmd, cwd, env);
        let elapsed = ms_since(start);
        if code != 0 && code != 1 {
            fail(&format!("{} exited {code}", cmd.repr()));
        }
        if i == 0 {
            continue;
        }
        samples.push(elapsed);
    }
    median(&samples)
}

fn mcp_round(cmd: &Cmd, cwd: &Path, env: &[(String, String)], tool: &str) -> f64 {
    let start = Instant::now();
    let mut child = cmd
        .command(cwd, env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|err| fail(&format!("{}: {err}", cmd.repr())));
    let mut stdin = child.stdin.take().expect("stdin");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut send = |line: &str, expect: bool| {
        stdin.write_all(line.as_bytes()).unwrap_or_else(|err| fail(&err.to_string()));
        stdin.write_all(b"\n").unwrap_or_else(|err| fail(&err.to_string()));
        stdin.flush().unwrap_or_else(|err| fail(&err.to_string()));
        if expect {
            let mut reply = String::new();
            let _ = reader.read_line(&mut reply);
        }
    };
    send(r#"{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "b", "version": "0"}}}"#, true);
    send(r#"{"jsonrpc": "2.0", "method": "notifications/initialized"}"#, false);
    send(
        &format!(r#"{{"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {{"name": "{tool}", "arguments": {{"scope": "b0"}}}}}}"#),
        true,
    );
    let _ = child.kill();
    let _ = child.wait();
    ms_since(start)
}

/// Peak resident set of one run, in kilobytes. Linux samples VmHWM from
/// /proc while the process runs; Windows reads the exited process's peak
/// working set; macOS reaps it with wait4 and reads ru_maxrss.
fn rss_kb(cmd: &Cmd, cwd: &Path, env: &[(String, String)]) -> u64 {
    let child = cmd
        .command(cwd, env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|err| fail(&format!("{}: {err}", cmd.repr())));
    let peak = peak_kb(child);
    if peak == 0 {
        fail(&format!("no RSS sample for {}", cmd.repr()));
    }
    peak
}

#[cfg(target_os = "linux")]
fn peak_kb(mut child: std::process::Child) -> u64 {
    let status = format!("/proc/{}/status", child.id());
    let mut peak = 0u64;
    loop {
        let Ok(text) = fs::read_to_string(&status) else {
            break;
        };
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("VmHWM:") {
                if let Some(kb) = rest.split_whitespace().next().and_then(|v| v.parse().ok()) {
                    peak = peak.max(kb);
                }
            }
        }
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
    }
    let _ = child.wait();
    peak
}

#[cfg(windows)]
fn peak_kb(mut child: std::process::Child) -> u64 {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    /// PROCESS_MEMORY_COUNTERS.
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn K32GetProcessMemoryInfo(process: *mut c_void, counters: *mut Counters, cb: u32) -> i32;
    }

    // The handle stays open until `child` drops, so the exited process
    // can still be queried.
    if child.wait().is_err() {
        return 0;
    }
    let mut counters = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Counters::default()
    };
    // SAFETY: a live process handle and a correctly sized, writable struct.
    let ok = unsafe { K32GetProcessMemoryInfo(child.as_raw_handle(), &mut counters, counters.cb) };
    if ok == 0 {
        return 0;
    }
    (counters.peak_working_set_size / 1024) as u64
}

#[cfg(target_os = "macos")]
fn peak_kb(child: std::process::Child) -> u64 {
    /// struct rusage: two timevals (16 bytes each), then 14 longs.
    #[repr(C)]
    struct Rusage {
        utime: [i64; 2],
        stime: [i64; 2],
        maxrss: i64,
        rest: [i64; 13],
    }

    extern "C" {
        fn wait4(pid: i32, status: *mut i32, options: i32, rusage: *mut Rusage) -> i32;
    }

    let mut usage = Rusage {
        utime: [0; 2],
        stime: [0; 2],
        maxrss: 0,
        rest: [0; 13],
    };
    let mut status = 0;
    loop {
        // SAFETY: valid out-pointers; the pid is our unreaped child.
        let pid = unsafe { wait4(child.id() as i32, &mut status, 0, &mut usage) };
        if pid >= 0 {
            break;
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return 0;
        }
    }
    // ru_maxrss is in bytes on macOS. The child is reaped; dropping it
    // without wait is fine.
    (usage.maxrss.max(0) as u64) / 1024
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn peak_kb(mut child: std::process::Child) -> u64 {
    let _ = child.wait();
    0
}

fn copytree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap_or_else(|err| fail(&err.to_string()));
    for entry in fs::read_dir(from).unwrap_or_else(|err| fail(&err.to_string())) {
        let entry = entry.unwrap_or_else(|err| fail(&err.to_string()));
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copytree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap_or_else(|err| fail(&err.to_string()));
        }
    }
}

fn absolute(arg: &str) -> PathBuf {
    let path = PathBuf::from(arg);
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    fs::canonicalize(&path)
        .map(|found| {
            let text = path_str(&found);
            PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
        })
        .unwrap_or(path)
}

struct Bench {
    py: Cmd,
    rust: Cmd,
    env: Vec<(String, String)>,
    work: PathBuf,
    rows: Vec<Row>,
}

impl Bench {
    fn push(&mut self, name: &str, py: f64, rs: f64, unit: &'static str) {
        self.rows.push(Row {
            name: name.to_string(),
            py,
            rs,
            unit,
        });
    }

    fn print_last(&self) {
        let row = self.rows.last().expect("row");
        println!("{}: python {:.1} rust {:.1} {}", row.name, row.py, row.rs, row.unit);
    }

    fn add(&mut self, name: &str, args: &[&str], cwd: &Path, runs: usize) {
        let py_v = time_call(&self.py.with(args), cwd, &self.env, runs);
        let rs_v = time_call(&self.rust.with(args), cwd, &self.env, runs);
        self.push(name, py_v, rs_v, "ms");
        self.print_last();
    }

    /// init is measured in a new directory each run, so average the runs here.
    fn fresh(&mut self, label: &str, args: &[&str], runs: usize, scale: usize) {
        let (mut py_samples, mut rs_samples) = (Vec::new(), Vec::new());
        for i in 0..runs {
            let dpy = self.work.join(format!("{label}-py-{i}"));
            let drs = self.work.join(format!("{label}-rs-{i}"));
            fs::create_dir(&dpy).unwrap_or_else(|err| fail(&err.to_string()));
            fs::create_dir(&drs).unwrap_or_else(|err| fail(&err.to_string()));
            if scale > 0 {
                make_store(&dpy.join(".swarmnotes"), scale);
                make_store(&drs.join(".swarmnotes"), scale);
            }
            for (cmd, dir, bucket) in [
                (self.py.with(args), &dpy, &mut py_samples),
                (self.rust.with(args), &drs, &mut rs_samples),
            ] {
                let start = Instant::now();
                let code = run_quiet(&cmd, dir, &self.env);
                if code != 0 {
                    fail(&format!("Command {} returned non-zero exit status {code}.", cmd.repr()));
                }
                bucket.push(ms_since(start));
            }
        }
        self.push(label, median(&py_samples[1..]), median(&rs_samples[1..]), "ms");
        self.print_last();
    }

    fn both(&mut self, label: &str, args: &[&str], cwd: &Path, runs: usize) {
        let (mut py_samples, mut rs_samples) = (Vec::new(), Vec::new());
        for i in 0..runs {
            let mut order = [(self.py.with(args), 0usize), (self.rust.with(args), 1usize)];
            if i % 2 == 1 {
                order.reverse();
            }
            for (cmd, side) in order {
                let start = Instant::now();
                run_quiet(&cmd, cwd, &self.env);
                if i > 0 {
                    let bucket = if side == 0 { &mut py_samples } else { &mut rs_samples };
                    bucket.push(ms_since(start));
                }
            }
        }
        self.push(label, median(&py_samples), median(&rs_samples), "ms");
        self.print_last();
    }

    fn mutating(&mut self, label: &str, kind: &str, folder: &Path, runs: usize) {
        let py_copy = self.work.join(format!("{label}-py"));
        let rs_copy = self.work.join(format!("{label}-rs"));
        for copy in [&py_copy, &rs_copy] {
            if copy.exists() {
                fs::remove_dir_all(copy).unwrap_or_else(|err| fail(&err.to_string()));
            }
            copytree(folder, copy);
        }
        let (mut py_samples, mut rs_samples) = (Vec::new(), Vec::new());
        for i in 0..runs {
            let extra = format!("extra/e{i}");
            let old = format!("b0/n{i}");
            let args: Vec<&str> = if kind == "new" {
                vec!["new", &extra, "--kind", "finding", "--summary", "s", "--body", "b", "--author", "bench"]
            } else {
                vec!["supersede", &old, "live"]
            };
            let mut order = [
                (self.py.with(&args), &py_copy, 0usize),
                (self.rust.with(&args), &rs_copy, 1usize),
            ];
            if i % 2 == 1 {
                order.reverse();
            }
            for (cmd, cwd, side) in order {
                let start = Instant::now();
                let out = cmd
                    .command(cwd, &self.env)
                    .stdin(Stdio::null())
                    .output()
                    .unwrap_or_else(|err| fail(&format!("{}: {err}", cmd.repr())));
                let code = out.status.code().unwrap_or(1);
                if code != 0 {
                    fail(&format!(
                        "{label} {} -> {code} {}",
                        cmd.repr(),
                        String::from_utf8_lossy(&out.stderr)
                    ));
                }
                if i > 0 {
                    let bucket = if side == 0 { &mut py_samples } else { &mut rs_samples };
                    bucket.push(ms_since(start));
                }
            }
        }
        // The first run is already skipped above; the old script dropped one more.
        self.push(label, median(&py_samples[1..]), median(&rs_samples[1..]), "ms");
        self.print_last();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        fail("usage: bench RELATUM RELATUM_MCP");
    }
    let rust = absolute(&args[0]);
    let rust_mcp = absolute(&args[1]);
    let python = python();
    let work = mkdtemp("relatum-bench-");
    let legacy_dir = work.join("legacy");
    extract_legacy(&legacy_dir);
    let legacy = legacy_dir.join("examen.py");
    let mut env: Vec<(String, String)> = std::env::vars().collect();
    env.retain(|(key, _)| key != "TZ" && key != "PYTHONDONTWRITEBYTECODE");
    env.push(("TZ".into(), "UTC".into()));
    env.push(("PYTHONDONTWRITEBYTECODE".into(), "1".into()));
    let mut bench = Bench {
        py: Cmd(vec![python.clone(), path_str(&legacy)]),
        rust: Cmd(vec![path_str(&rust)]),
        env: env.clone(),
        work: work.clone(),
        rows: Vec::new(),
    };

    let help_dir = work.join("empty");
    fs::create_dir(&help_dir).unwrap_or_else(|err| fail(&err.to_string()));
    bench.add("cli startup (--help)", &["--help"], &help_dir, RUNS_STARTUP);

    let small = work.join("small");
    let large = work.join("large");
    let big = work.join("big");
    make_store(&small.join(".swarmnotes"), 30);
    make_store(&large.join(".swarmnotes"), 2000);
    make_store(&big.join(".swarmnotes"), 8000);
    // A live target for supersede, not part of the counted chain.
    for base in [&small, &large] {
        write_text(&base.join(".swarmnotes").join("live.md"), &note(99999));
        write_text(&base.join(".swarmnotes").join("old.md"), &note(99998));
    }

    let with_env = |extra: &[(&str, String)]| {
        let mut out = env.clone();
        for (key, value) in extra {
            out.retain(|(k, _)| k != key);
            out.push((key.to_string(), value.clone()));
        }
        out
    };
    let store = path_str(&small.join(".swarmnotes"));
    let py_mcp = Cmd(vec![python.clone(), "-m".into(), "examen_mcp".into()]);
    // Put the legacy dir first and keep any PYTHONPATH the caller set (for
    // example a site-packages that holds the mcp package).
    let python_path = match std::env::var_os("PYTHONPATH") {
        Some(extra) if !extra.is_empty() => std::env::join_paths(
            std::iter::once(legacy_dir.clone()).chain(std::env::split_paths(&extra)),
        )
        .map(|joined| joined.to_string_lossy().to_string())
        .unwrap_or_else(|_| path_str(&legacy_dir)),
        _ => path_str(&legacy_dir),
    };
    let mcp_env = with_env(&[("PYTHONPATH", python_path), ("SWARMNOTES_DIR", store.clone())]);
    let rs_env = with_env(&[("SWARMNOTES_DIR", store)]);
    let rs_mcp = Cmd(vec![path_str(&rust_mcp)]);
    let (mut py_samples, mut rs_samples) = (Vec::new(), Vec::new());
    for i in 0..RUNS_SMALL {
        let mut order = [
            (&py_mcp, &mcp_env, "examen_ls", 0usize),
            (&rs_mcp, &rs_env, "relatum_ls", 1usize),
        ];
        if i % 2 == 1 {
            order.reverse();
        }
        for (cmd, menv, tool, side) in order {
            let elapsed = mcp_round(cmd, &small, menv, tool);
            if i > 0 {
                let bucket = if side == 0 { &mut py_samples } else { &mut rs_samples };
                bucket.push(elapsed);
            }
        }
    }
    let (py_m, rs_m) = (median(&py_samples), median(&rs_samples));
    bench.push("mcp startup + ls round trip", py_m, rs_m, "ms");
    println!("mcp: python {py_m:.1} rust {rs_m:.1} ms");

    bench.fresh("init empty", &["init"], RUNS_SMALL, 0);
    bench.fresh("init on large store", &["init"], RUNS_SMALL, 2000);

    for (label, folder, runs) in [("small", &small, RUNS_SMALL), ("large", &large, RUNS_LARGE)] {
        bench.both(&format!("ls {label}"), &["ls"], folder, runs);
        bench.both(&format!("get {label}"), &["get", "b0/n0"], folder, runs);
        bench.both(&format!("check {label}"), &["check"], folder, runs);
    }

    bench.mutating("new small", "new", &small, RUNS_SMALL);
    bench.mutating("new large", "new", &large, RUNS_LARGE);
    bench.mutating("supersede small", "sup", &small, RUNS_SMALL);
    bench.mutating("supersede large", "sup", &large, RUNS_LARGE);
    bench.both("check big", &["check"], &big, RUNS_BIG);

    let py_check = bench.py.with(&["check"]);
    let rs_check = bench.rust.with(&["check"]);
    let py_rss: Vec<f64> = (0..RUNS_MEMORY).map(|_| rss_kb(&py_check, &big, &env) as f64).collect();
    let rs_rss: Vec<f64> = (0..RUNS_MEMORY).map(|_| rss_kb(&rs_check, &big, &env) as f64).collect();
    bench.push("memory check big (RSS)", median(&py_rss), median(&rs_rss), "KB");
    let row = bench.rows.last().expect("row");
    println!("memory: python {:.0} rust {:.0} KB", row.py, row.rs);

    println!("\n| Row | Python 0.3.0 | Relatum {} |", env!("CARGO_PKG_VERSION"));
    println!("|---|---:|---:|");
    let mut slower = Vec::new();
    for row in &bench.rows {
        if row.unit == "KB" {
            println!("| {} | {:.0} {} | {:.0} {} |", row.name, row.py, row.unit, row.rs, row.unit);
        } else {
            println!("| {} | {:.1} {} | {:.1} {} |", row.name, row.py, row.unit, row.rs, row.unit);
        }
        if !(row.rs < row.py) {
            slower.push(row.name.clone());
        }
    }
    if !slower.is_empty() {
        println!("NOT FASTER: {}", slower.join(", "));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A child that lives a moment and touches a few megabytes.
    fn busy_child() -> Command {
        if cfg!(windows) {
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", "ping -n 2 127.0.0.1 >NUL"]);
            cmd
        } else {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", "sleep 0.3"]);
            cmd
        }
    }

    #[test]
    fn peak_memory_of_a_child_is_measured_on_this_os() {
        let child = busy_child()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let kb = peak_kb(child);
        // Any real process maps at least a few hundred KB and far less than 1 GB.
        assert!((200..1_000_000).contains(&kb), "peak {kb} KB");
    }
}

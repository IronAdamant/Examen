//! Side-by-side parity: Examen 0.3.0 (python-legacy) against the Relatum binary.
//!
//! The Python program name is forced to the same string the Rust binary prints
//! (`relatum` or `swarmnotes`). That is the rename. File bytes, exit codes, and
//! the remaining text are compared as-is. MCP tool names are compared after
//! rewriting relatum_ to examen_, and pydantic's versioned error URL is
//! normalised on both sides.
//!
//! The frozen Python release is the reference oracle. A long-lived Python
//! process loads examen.py with `prog_name` patched and runs each CLI call
//! in-process, and the Python MCP server runs as `python -m examen_mcp`. Python is found through
//! `PARITY_PYTHON`, then `python3`, then `python`. The MCP half needs
//! `pip install "mcp==2.3.0"`.
//!
//! Usage: cargo run --release --locked --example parity -- RELATUM RELATUM_MCP

#[path = "common.rs"]
mod common;

use common::{extract_legacy, fail, mkdtemp, path_str, python, rmtree, root, write_text};
use relatum::json::{self, Value};
use relatum::pystr::{py_list, py_repr};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::cell::RefCell;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

/// The frozen release as a long-lived oracle. Each request runs one call
/// in-process, exactly as the old Python harness did: chdir, TZ=UTC,
/// SWARMNOTES_DIR set or removed, stdin/stdout/stderr swapped for StringIO,
/// and SystemExit turned into an exit code. Replies are one JSON line.
const ORACLE: &str = r#"
import contextlib, importlib.util, io, json, os, sys
legacy, prog = sys.argv[1], sys.argv[2]
spec = importlib.util.spec_from_file_location("examen_legacy", os.path.join(legacy, "examen.py"))
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
mod.prog_name = lambda argv0=None, prog=prog: prog
sys.modules["examen"] = mod
mcp_mod = None
requests, replies = sys.stdin, sys.stdout
while True:
    line = requests.readline()
    if not line:
        break
    req = json.loads(line)
    old_cwd = os.getcwd()
    old_env = {key: os.environ.get(key) for key in ("SWARMNOTES_DIR", "TZ")}
    try:
        os.chdir(req["cwd"])
        os.environ["TZ"] = "UTC"
        if hasattr(os, "tzset"):
            os.tzset()
        if "SWARMNOTES_DIR" in req["env"]:
            os.environ["SWARMNOTES_DIR"] = req["env"]["SWARMNOTES_DIR"]
        else:
            os.environ.pop("SWARMNOTES_DIR", None)
        if req["op"] == "cli":
            out, err = io.StringIO(), io.StringIO()
            sys.stdin = io.StringIO(req["stdin"])
            try:
                with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                    try:
                        code = mod.main(list(req["args"]))
                    except SystemExit as exc:
                        code = exc.code if isinstance(exc.code, int) else 1
            finally:
                sys.stdin = requests
            reply = {"code": code or 0, "out": out.getvalue(), "err": err.getvalue()}
        else:
            if mcp_mod is None:
                mspec = importlib.util.spec_from_file_location("examen_mcp_legacy", os.path.join(legacy, "examen_mcp.py"))
                mcp_mod = importlib.util.module_from_spec(mspec)
                mspec.loader.exec_module(mcp_mod)
            try:
                reply = {"ok": mcp_mod.examen_init(req["path"])}
            except Exception as exc:
                reply = {"err": f"ERR {exc}"}
    finally:
        os.chdir(old_cwd)
        for key, value in old_env.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
    replies.write(json.dumps(reply) + "\n")
    replies.flush()
"#;

type Call = (i32, String, String);
type Snap = Vec<(String, Vec<u8>)>;

struct Diffs {
    count: usize,
}

impl Diffs {
    fn add(&mut self, title: &str, left: &str, right: &str) {
        self.count += 1;
        println!("DIFF {title}");
        println!("--- python ---");
        println!("{left}");
        println!("--- rust ---");
        println!("{right}");
    }
}

struct Oracle {
    python: String,
    child: RefCell<Child>,
    reader: RefCell<BufReader<ChildStdout>>,
}

impl Oracle {
    fn start(python: &str, legacy: &Path, prog: &str) -> Oracle {
        let mut child = Command::new(python)
            .arg("-c")
            .arg(ORACLE)
            .arg(legacy)
            .arg(prog)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|err| fail(&format!("spawn {python}: {err}")));
        let reader = BufReader::new(child.stdout.take().expect("stdout"));
        Oracle {
            python: python.to_string(),
            child: RefCell::new(child),
            reader: RefCell::new(reader),
        }
    }

    fn request(&self, mut pairs: Vec<(&str, Value)>, cwd: &Path, env: &[(&str, String)]) -> Value {
        let env = json::obj(env.iter().map(|(key, value)| (*key, json::s(value))).collect());
        pairs.push(("cwd", json::s(&path_str(cwd))));
        pairs.push(("env", env));
        let line = format!("{}\n", json::write_compact(&json::obj(pairs)));
        let mut child = self.child.borrow_mut();
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.flush())
            .unwrap_or_else(|err| fail(&format!("python oracle: {err}")));
        let mut reply = String::new();
        self.reader
            .borrow_mut()
            .read_line(&mut reply)
            .unwrap_or_else(|err| fail(&format!("python oracle: {err}")));
        json::parse(reply.trim_end())
            .unwrap_or_else(|_| fail(&format!("python oracle replied {reply:?}")))
    }

    /// `py_call`: run `examen.main(args)` and return (code, stdout, stderr).
    fn call(&self, args: &[String], cwd: &Path, env: &[(&str, String)], stdin: &str) -> Call {
        let reply = self.request(
            vec![
                ("op", json::s("cli")),
                ("args", json::arr(args.iter().map(|arg| json::s(arg)).collect())),
                ("stdin", json::s(stdin)),
            ],
            cwd,
            env,
        );
        let code = match reply.get("code") {
            Some(Value::Number(n)) => n.parse().unwrap_or(1),
            _ => fail(&format!("python oracle replied {}", json::write_compact(&reply))),
        };
        let text = |key: &str| logical_newlines(reply.get(key).and_then(Value::as_str).unwrap_or(""));
        (code, text("out"), text("err"))
    }

    /// `mcp_mod.examen_init(path)`: `{"ok": dict}` or `{"err": "ERR ..."}`.
    fn init(&self, path: &str, cwd: &Path, env: &[(&str, String)]) -> Value {
        self.request(vec![("op", json::s("init")), ("path", json::s(path))], cwd, env)
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        let child = self.child.get_mut();
        drop(child.stdin.take());
        let _ = child.wait();
    }
}

fn run(mut cmd: Command, stdin: &str) -> Call {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .unwrap_or_else(|err| fail(&format!("spawn {:?}: {err}", cmd.get_program())));
    let mut input = child.stdin.take().expect("stdin");
    let data = stdin.as_bytes().to_vec();
    let writer = std::thread::spawn(move || {
        let _ = input.write_all(&data);
    });
    let out = child.wait_with_output().unwrap_or_else(|err| fail(&err.to_string()));
    let _ = writer.join();
    (
        out.status.code().unwrap_or(1),
        logical_newlines(&String::from_utf8_lossy(&out.stdout)),
        logical_newlines(&String::from_utf8_lossy(&out.stderr)),
    )
}

fn rust_call(binary: &Path, args: &[String], cwd: &Path, env: &[(&str, String)], stdin: &str) -> Call {
    let mut cmd = Command::new(binary);
    cmd.args(args)
        .current_dir(cwd)
        .env("TZ", "UTC")
        .env_remove("SWARMNOTES_DIR");
    for (key, value) in env {
        cmd.env(key, value);
    }
    run(cmd, stdin)
}

/// Every file under `root`, keyed by its POSIX relative path, in `os.walk`
/// order (a directory's files, then each subdirectory, names sorted).
fn snapshot(root: &Path) -> Snap {
    let mut found = Vec::new();
    if root.is_dir() {
        walk(root, root, &mut found);
    }
    found
}

fn walk(root: &Path, dir: &Path, found: &mut Snap) {
    let mut names: Vec<_> = fs::read_dir(dir)
        .map(|entries| entries.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    names.sort();
    let (dirs, files): (Vec<PathBuf>, Vec<PathBuf>) = names.into_iter().partition(|p| p.is_dir());
    for path in files {
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let key = rel
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        found.push((key, fs::read(&path).unwrap_or_default()));
    }
    for sub in dirs {
        walk(root, &sub, found);
    }
}

fn snap_eq(left: &Snap, right: &Snap) -> bool {
    as_map(left) == as_map(right)
}

fn as_map(snap: &Snap) -> BTreeMap<&str, &[u8]> {
    snap.iter().map(|(k, v)| (k.as_str(), v.as_slice())).collect()
}

/// Program path for a child process. A relative path is resolved from the
/// repository root before any child changes directory: on POSIX a relative
/// program path is looked up inside the child's working directory.
fn absolute_tool(arg: &str) -> PathBuf {
    let mut path = PathBuf::from(arg);
    if !path.is_absolute() {
        path = root().join(path);
    }
    if let Ok(resolved) = fs::canonicalize(&path) {
        path = strip_verbatim(resolved);
    }
    if !path.is_file() {
        let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut entries: Vec<String> = fs::read_dir(&parent)
            .map(|items| {
                items
                    .filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        entries.sort();
        let cwd = std::env::current_dir().map(|d| path_str(&d)).unwrap_or_default();
        fail(&format!(
            "missing tool binary {}; cwd={cwd}; entries={}",
            path_str(&path),
            py_list(&entries)
        ));
    }
    path
}

fn strip_verbatim(path: PathBuf) -> PathBuf {
    match path.to_string_lossy().strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path,
    }
}

/// Fold Windows text-mode newlines. CPython writes CRLF to a text-mode
/// stream on Windows. A carriage return is not a different line; a missing
/// or extra line still fails.
fn logical_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Replace only the unique final component of each side's temp directory.
/// The rest of the path stays, so a different drive, slash, prefix, or a
/// missing .swarmnotes segment still fails.
fn scrub_temp_names(text: &str, folders: &[&Path]) -> String {
    let mut text = text.to_string();
    for folder in folders {
        if let Some(name) = folder.file_name() {
            let name = name.to_string_lossy();
            if !name.is_empty() {
                text = text.replace(name.as_ref(), "$DIR");
            }
        }
    }
    text
}

fn norm_mcp(text: &str) -> String {
    let text = norm_pydantic_url(text);
    let text = text
        .replace("relatum_", "examen_")
        .replace("Relatum", "Examen")
        .replace("relatum", "examen");
    // serverInfo.version is the Cargo package version. Examen 0.3.0 reported
    // 0.3.0. Rewrite the Relatum versions this tree has shipped.
    text.replace("1.0.1", "0.3.0").replace("1.0.0", "0.3.0")
}

/// `https://errors.pydantic.dev/<digits>.<digits>/` -> `.../VER/`.
fn norm_pydantic_url(text: &str) -> String {
    const HEAD: &str = "https://errors.pydantic.dev/";
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(HEAD) {
        out.push_str(&rest[..at + HEAD.len()]);
        rest = &rest[at + HEAD.len()..];
        if let Some(len) = version_segment(rest) {
            out.push_str("VER/");
            rest = &rest[len..];
        }
    }
    out.push_str(rest);
    out
}

fn version_segment(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let digits = |from: usize| bytes[from..].iter().take_while(|b| b.is_ascii_digit()).count();
    let major = digits(0);
    if major == 0 || bytes.get(major) != Some(&b'.') {
        return None;
    }
    let minor = digits(major + 1);
    let end = major + 1 + minor;
    (minor > 0 && bytes.get(end) == Some(&b'/')).then_some(end + 1)
}

/// `json.dumps(value, sort_keys=True)` with Python's default separators and
/// ensure_ascii.
fn dumps_sorted(value: &Value) -> String {
    let mut out = String::new();
    dump_into(&mut out, value, true);
    out
}

/// `json.dumps(value)`: keys stay in insertion order.
fn dumps(value: &Value) -> String {
    let mut out = String::new();
    dump_into(&mut out, value, false);
    out
}

fn dump_into(out: &mut String, value: &Value, sort: bool) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(n),
        Value::String(s) => dump_str(out, s),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                dump_into(out, item, sort);
            }
            out.push(']');
        }
        Value::Object(pairs) => {
            let mut sorted: Vec<_> = pairs.iter().collect();
            if sort {
                sorted.sort_by(|a, b| a.0.cmp(&b.0));
            }
            out.push('{');
            for (i, (key, item)) in sorted.into_iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                dump_str(out, key);
                out.push_str(": ");
                dump_into(out, item, sort);
            }
            out.push('}');
        }
    }
}

fn dump_str(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

fn call_repr(call: &Call) -> String {
    format!("({}, {}, {})", call.0, py_repr(&call.1), py_repr(&call.2))
}

fn bytes_repr(data: &[u8]) -> String {
    let quote = if data.contains(&b'\'') && !data.contains(&b'"') { b'"' } else { b'\'' };
    let mut out = String::from("b");
    out.push(quote as char);
    for &byte in data {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            _ if byte == quote => {
                out.push('\\');
                out.push(quote as char);
            }
            0x20..=0x7e => out.push(byte as char),
            _ => out.push_str(&format!("\\x{byte:02x}")),
        }
    }
    out.push(quote as char);
    out
}

fn snap_repr(snap: &Snap) -> String {
    let body = snap
        .iter()
        .map(|(key, data)| format!("{}: {}", py_repr(key), bytes_repr(data)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{body}}}")
}

fn opt_bytes_repr(data: Option<&&[u8]>) -> String {
    data.map(|d| bytes_repr(d)).unwrap_or_else(|| "None".to_string())
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

fn cli_suite(oracle: &Oracle, binary: &Path, diffs: &mut Diffs) {
    let scenarios: Vec<Vec<(&str, Vec<&str>, &str)>> = vec![
        vec![
            ("init", vec![], ""),
            ("ls", vec![], ""),
            ("check", vec![], ""),
            ("new", vec!["net/retry", "--kind", "decision", "--summary", "backoff", "--body", "use jitter", "--author", "parent", "--tags", "net, retry"], ""),
            ("get", vec!["net/retry"], ""),
            ("ls", vec!["net", "--kind", "decision", "--tag", "net"], ""),
            ("new", vec!["net/linear", "--kind", "decision", "--summary", "old", "--body", "1s then 2s"], ""),
            ("supersede", vec!["net/linear", "net/retry"], ""),
            ("ls", vec![], ""),
            ("ls", vec!["--all"], ""),
            ("get", vec!["net/linear", "net/retry"], ""),
            ("check", vec![], ""),
        ],
        vec![
            ("init", vec!["nested"], ""),
            ("init", vec!["nested"], ""),
            ("new", vec!["Bad/ID", "--kind", "decision", "--summary", "s", "--body", "b"], ""),
            ("new", vec!["x", "--kind", "nope", "--summary", "s", "--body", "b"], ""),
            ("new", vec!["e", "--kind", "decision", "--summary", "s", "--body", "  \n"], ""),
            ("new", vec!["piped", "--kind", "context", "--summary", "from stdin"], "piped body\n"),
            ("ls", vec!["missing-scope"], ""),
            ("get", vec!["missing", "piped"], ""),
            ("check", vec!["extra"], ""),
            ("new", vec![], ""),
            ("ls", vec!["--kind"], ""),
            ("--help", vec![], ""),
            ("new", vec!["-h"], ""),
        ],
    ];
    for (index, steps) in scenarios.iter().enumerate() {
        let py_dir = mkdtemp("py-cli-");
        let rs_dir = mkdtemp("rs-cli-");
        let stop = cli_scenario(oracle, binary, diffs, index, steps, &py_dir, &rs_dir);
        rmtree(&py_dir);
        rmtree(&rs_dir);
        if stop {
            return;
        }
    }

    let example = root().join("examples").join("retry-swarm");
    for (title, args) in [
        ("example check", strings(&["check"])),
        ("example get", strings(&["get", "net/backoff-linear"])),
        ("example ls", strings(&["ls", "net", "--all"])),
    ] {
        let py = oracle.call(&args, &example, &[], "");
        let rs = rust_call(binary, &args, &example, &[], "");
        if py != rs {
            diffs.add(title, &call_repr(&py), &call_repr(&rs));
        }
    }
}

/// One CLI scenario. Returns true when a diff ends the suite.
fn cli_scenario(
    oracle: &Oracle,
    binary: &Path,
    diffs: &mut Diffs,
    index: usize,
    steps: &[(&str, Vec<&str>, &str)],
    py_dir: &Path,
    rs_dir: &Path,
) -> bool {
    for (command, args, stdin) in steps {
        let argv: Vec<String> = if *command == "--help" {
            strings(&["--help"])
        } else {
            std::iter::once(*command).chain(args.iter().copied()).map(String::from).collect()
        };
        let py = oracle.call(&argv, py_dir, &[], stdin);
        let rs = rust_call(binary, &argv, rs_dir, &[], stdin);
        let label = format!("cli[{index}] {}", py_list(&argv));
        if py != rs {
            diffs.add(&label, &call_repr(&py), &call_repr(&rs));
            return true;
        }
        // init nested writes under nested/.swarmnotes
        let files = |dir: &Path| {
            let mut snap = snapshot(&dir.join(".swarmnotes"));
            for (key, data) in snapshot(&dir.join("nested").join(".swarmnotes")) {
                let key = format!("nested/{key}");
                snap.retain(|(k, _)| *k != key);
                snap.push((key, data));
            }
            snap
        };
        let py_files = files(py_dir);
        let rs_files = files(rs_dir);
        if !snap_eq(&py_files, &rs_files) {
            let (py_map, rs_map) = (as_map(&py_files), as_map(&rs_files));
            let keys = |map: &BTreeMap<&str, &[u8]>| py_list(&map.keys().map(|k| k.to_string()).collect::<Vec<_>>());
            diffs.add(&format!("{label} files"), &keys(&py_map), &keys(&rs_map));
            let all: std::collections::BTreeSet<&str> = py_map.keys().chain(rs_map.keys()).copied().collect();
            for key in all {
                if py_map.get(key) != rs_map.get(key) {
                    diffs.add(
                        &format!("{label} file {key}"),
                        &opt_bytes_repr(py_map.get(key)),
                        &opt_bytes_repr(rs_map.get(key)),
                    );
                }
            }
            return true;
        }
    }
    false
}

fn extra_suite(oracle: &Oracle, binary: &Path, binary_mcp: &Path, diffs: &mut Diffs) {
    let py_dir = mkdtemp("py-extra-");
    let rs_dir = mkdtemp("rs-extra-");
    let stop = extra_steps(oracle, binary, diffs, &py_dir, &rs_dir);
    rmtree(&py_dir);
    rmtree(&rs_dir);
    if stop {
        return;
    }

    // swarmnotes alias: same bytes as prog_name swarmnotes.
    let swarm = binary.with_file_name(format!("swarmnotes{}", std::env::consts::EXE_SUFFIX));
    let legacy = mkdtemp("alias-");
    extract_legacy(&legacy);
    let swarm_oracle = Oracle::start(&oracle.python, &legacy, "swarmnotes");
    let py_dir = mkdtemp("py-alias-");
    let rs_dir = mkdtemp("rs-alias-");
    let mut stop = false;
    for argv in [vec!["init"], vec!["ls"], vec!["-h"], vec!["check"]] {
        let argv = strings(&argv);
        let py = swarm_oracle.call(&argv, &py_dir, &[], "");
        let rs = rust_call(&swarm, &argv, &rs_dir, &[], "");
        if py != rs || !snap_eq(&snapshot(&py_dir.join(".swarmnotes")), &snapshot(&rs_dir.join(".swarmnotes"))) {
            diffs.add(&format!("swarmnotes {}", py_list(&argv)), &call_repr(&py), &call_repr(&rs));
            stop = true;
            break;
        }
    }
    rmtree(&py_dir);
    rmtree(&rs_dir);
    rmtree(&legacy);
    if stop {
        return;
    }

    mcp_init_matrix(oracle, binary_mcp, diffs);
}

/// The extra CLI steps, hand-written notes, and SWARMNOTES_DIR. Returns true
/// when a diff ends the suite.
fn extra_steps(oracle: &Oracle, binary: &Path, diffs: &mut Diffs, py_dir: &Path, rs_dir: &Path) -> bool {
    let w300 = "word ".repeat(300);
    let w301 = "word ".repeat(301);
    let steps: Vec<Vec<&str>> = vec![
        vec!["init"],
        vec!["new", "a", "--kind", "decision", "--summary", "s", "--body", "body-a"],
        vec!["new", "b", "--kind", "decision", "--summary", "s", "--body", "body-b"],
        vec!["new", "c", "--kind", "decision", "--summary", "s", "--body", "body-c"],
        vec!["supersede", "b", "c"],
        vec!["supersede", "a", "b"],
        vec!["get", "a"],
        vec!["supersede", "c", "a"],
        vec!["new", "edge", "--kind", "decision", "--summary", "s", "--body", &w300, "--author", "p"],
        vec!["new", "long", "--kind", "decision", "--summary", "s", "--body", &w301, "--author", "p"],
        vec!["check"],
        vec!["ls", "--all"],
    ];
    for argv in steps {
        let argv = strings(&argv);
        let py = oracle.call(&argv, py_dir, &[], "");
        let rs = rust_call(binary, &argv, rs_dir, &[], "");
        let py_snap = snapshot(&py_dir.join(".swarmnotes"));
        let rs_snap = snapshot(&rs_dir.join(".swarmnotes"));
        let files_differ = !snap_eq(&py_snap, &rs_snap);
        if py != rs || files_differ {
            let left = format!(
                "({}, {})",
                call_repr(&py),
                py_repr(if files_differ { "files-differ" } else { "text" })
            );
            let right = format!(
                "({}, {})",
                call_repr(&rs),
                if files_differ { snap_repr(&rs_snap) } else { py_repr("") }
            );
            diffs.add(&format!("extra {}", py_list(&argv)), &left, &right);
            return true;
        }
    }
    // Hand-written notes, then check.
    for folder in [py_dir, rs_dir] {
        let store = folder.join(".swarmnotes");
        write_text(&store.join("empty.md"), "---\nkind: decision\nsummary: s\nauthor: p\nupdated: 2026-10-02\n---\n\n");
        write_text(&store.join("bad.md"), "---\nkind: decision\nsummary: s\nauthor: p\nstatus: stale\nupdated: 2026-13-40\n---\n\nbody\n");
        write_text(&store.join("open.md"), "---\nkind: decision\nsummary: s\n");
        fs::write(store.join("bin.md"), b"\xff\xfe not utf-8").unwrap_or_else(|err| fail(&err.to_string()));
        write_text(&store.join("dup.md"), "---\nkind: decision\nkind: finding\nsummary: s\nauthor: p\nupdated: 2026-10-02\nowner: me\ntags: nope\n---\n\nbody\n");
    }
    let check = strings(&["check"]);
    let py = oracle.call(&check, py_dir, &[], "");
    let rs = rust_call(binary, &check, rs_dir, &[], "");
    if py != rs {
        diffs.add("handwritten check", &call_repr(&py), &call_repr(&rs));
    }
    let other_py = py_dir.join("elsewhere");
    let other_rs = rs_dir.join("elsewhere");
    fs::create_dir(&other_py).unwrap_or_else(|err| fail(&err.to_string()));
    fs::create_dir(&other_rs).unwrap_or_else(|err| fail(&err.to_string()));
    let argv = strings(&["new", "x", "--kind", "finding", "--summary", "s", "--body", "b"]);
    let py = oracle.call(&argv, py_dir, &[("SWARMNOTES_DIR", path_str(&other_py))], "");
    let rs = rust_call(binary, &argv, rs_dir, &[("SWARMNOTES_DIR", path_str(&other_rs))], "");
    let renamed = (rs.0, rs.1.clone(), rs.2.replace("relatum", "examen"));
    // stderr carries the program name only when it fails. Success prints the id.
    if py != renamed && py != rs {
        diffs.add("env new", &call_repr(&py), &call_repr(&rs));
    }
    let (snap_py, snap_rs) = (snapshot(&other_py), snapshot(&other_rs));
    if !snap_eq(&snap_py, &snap_rs) {
        diffs.add("env files", &snap_repr(&snap_py), &snap_repr(&snap_rs));
    }
    false
}

/// Compare examen_init path rules. Roots are rewritten to $DIR.
fn mcp_init_matrix(oracle: &Oracle, binary_mcp: &Path, diffs: &mut Diffs) {
    let legacy = mkdtemp("init-legacy-");
    extract_legacy(&legacy);
    let init_oracle = Oracle::start(&oracle.python, &legacy, "relatum");
    let cases: [(&str, Option<&str>, Option<&str>); 5] = [
        ("cwd", Some("."), None),
        ("outside-store", None, Some("store")),
        ("project-of-store", None, Some("parent")),
        ("refuse-nested", Some("nested"), None),
        ("blank", Some("   "), None),
    ];
    for (name, path, mode) in cases {
        let py_dir = mkdtemp(&format!("py-{name}-"));
        let rs_dir = mkdtemp(&format!("rs-{name}-"));
        // The Python release and the server get the same path rules.
        let setup = |dir: &Path| -> (Option<String>, Vec<(&'static str, String)>) {
            let parent = dir.parent().map(Path::to_path_buf).unwrap_or_default();
            match mode {
                Some("store") => {
                    let store = path_str(&parent.join(format!("{name}-store")));
                    (Some(store.clone()), vec![("SWARMNOTES_DIR", store)])
                }
                Some("parent") => {
                    let store = parent.join(format!("{name}-store")).join(".swarmnotes");
                    let project = store.parent().map(path_str).unwrap_or_default();
                    (Some(project), vec![("SWARMNOTES_DIR", path_str(&store))])
                }
                _ => (path.map(str::to_string), Vec::new()),
            }
        };
        let (py_path, py_env) = setup(&py_dir);
        let py_result = match &py_path {
            None => Value::String("skip".to_string()),
            Some(p) => init_oracle.init(p, &py_dir, &py_env),
        };
        let (rs_path, rs_env) = setup(&rs_dir);
        let arguments = json::obj(vec![(
            "path",
            rs_path.as_deref().map(json::s).unwrap_or(Value::Null),
        )]);
        let (rs_result, _rs_err) = mcp_one(binary_mcp, &rs_dir, &rs_env, "relatum_init", arguments);
        let folders = [py_dir.as_path(), rs_dir.as_path()];
        let (py_text, rs_text) = match py_result.get("ok") {
            Some(dict) => {
                let inner = rs_result
                    .get("result")
                    .and_then(|r| r.get("structuredContent"))
                    .and_then(|s| s.get("result"))
                    .cloned()
                    .unwrap_or(Value::Null);
                (norm_mcp(&dumps_sorted(dict)), norm_mcp(&dumps_sorted(&inner)))
            }
            None => {
                let raw = match py_result.get("err") {
                    Some(Value::String(text)) => text.clone(),
                    _ => py_result.as_str().unwrap_or("").to_string(),
                };
                let py_text = norm_mcp(&raw);
                let py_text = py_text.strip_prefix("ERR ").unwrap_or(&py_text).to_string();
                let mut rs_text = rs_result
                    .get("result")
                    .and_then(|r| r.get("content"))
                    .and_then(Value::as_array)
                    .and_then(|items| items.first())
                    .and_then(|item| item.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if rs_text.starts_with("Error executing tool ") {
                    rs_text = rs_text.split_once(": ").map(|(_, tail)| tail.to_string()).unwrap_or(rs_text);
                }
                (py_text, norm_mcp(&rs_text))
            }
        };
        let py_text = scrub_temp_names(&py_text, &folders);
        let rs_text = scrub_temp_names(&rs_text, &folders);
        if py_text != rs_text {
            diffs.add(&format!("init {name}"), &py_text, &rs_text);
        }
        rmtree(&py_dir);
        rmtree(&rs_dir);
    }
    rmtree(&legacy);
}

struct Session {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
    stderr: std::thread::JoinHandle<String>,
}

impl Session {
    fn start(mut cmd: Command) -> Session {
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .unwrap_or_else(|err| fail(&format!("spawn {:?}: {err}", cmd.get_program())));
        let reader = BufReader::new(child.stdout.take().expect("stdout"));
        let mut err_pipe = child.stderr.take().expect("stderr");
        let stderr = std::thread::spawn(move || {
            let mut data = Vec::new();
            let _ = err_pipe.read_to_end(&mut data);
            String::from_utf8_lossy(&data).to_string()
        });
        Session { child, reader, stderr }
    }

    fn send(&mut self, message: &Value, expect: bool) -> Option<Value> {
        let stdin = self.child.stdin.as_mut().expect("stdin");
        let line = format!("{}\n", dumps(message));
        stdin.write_all(line.as_bytes()).unwrap_or_else(|err| fail(&format!("mcp write: {err}")));
        stdin.flush().unwrap_or_else(|err| fail(&format!("mcp flush: {err}")));
        if !expect {
            return None;
        }
        let mut reply = String::new();
        self.reader.read_line(&mut reply).unwrap_or_else(|err| fail(&format!("mcp read: {err}")));
        Some(json::parse(reply.trim_end()).unwrap_or_else(|_| fail(&format!("mcp replied {reply:?}"))))
    }

    fn initialize(&mut self, client: &str) {
        let init = json::parse(&format!(
            r#"{{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {{"protocolVersion": "2024-11-05", "capabilities": {{}}, "clientInfo": {{"name": "{client}", "version": "0"}}}}}}"#
        ))
        .expect("init json");
        self.send(&init, true);
        let note = json::parse(r#"{"jsonrpc": "2.0", "method": "notifications/initialized"}"#).expect("note json");
        self.send(&note, false);
    }

    /// Close stdin, wait up to 10 s, return stderr.
    fn finish(mut self) -> String {
        drop(self.child.stdin.take());
        let start = Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if start.elapsed() < Duration::from_secs(10) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                _ => {
                    let _ = self.child.kill();
                    fail("MCP server did not exit within 10 s");
                }
            }
        }
        self.stderr.join().unwrap_or_default()
    }
}

fn call_message(id: usize, name: &str, arguments: Value) -> Value {
    json::obj(vec![
        ("jsonrpc", json::s("2.0")),
        ("id", Value::Number(id.to_string())),
        ("method", json::s("tools/call")),
        ("params", json::obj(vec![("name", json::s(name)), ("arguments", arguments)])),
    ])
}

fn mcp_round(cmd: Command, calls: &[(String, Value)]) -> (Vec<Value>, String) {
    let mut session = Session::start(cmd);
    session.initialize("parity");
    let mut responses = Vec::new();
    let list = json::parse(r#"{"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}"#).expect("list json");
    let listed = session.send(&list, true).unwrap_or(Value::Null);
    responses.push(json::arr(vec![json::s("list"), listed]));
    for (offset, (name, arguments)) in calls.iter().enumerate() {
        let reply = session
            .send(&call_message(3 + offset, name, arguments.clone()), true)
            .unwrap_or(Value::Null);
        responses.push(json::arr(vec![json::s(name), reply]));
    }
    let err = session.finish();
    (responses, err)
}

fn mcp_one(binary: &Path, cwd: &Path, env: &[(&str, String)], name: &str, arguments: Value) -> (Value, String) {
    let mut cmd = Command::new(binary);
    cmd.current_dir(cwd).env("TZ", "UTC").env_remove("SWARMNOTES_DIR");
    for (key, value) in env {
        cmd.env(key, value);
    }
    let mut session = Session::start(cmd);
    session.initialize("p");
    let result = session
        .send(&call_message(2, name, arguments), true)
        .unwrap_or(Value::Null);
    let err = session.finish();
    (result, err)
}

fn arg(json_text: &str) -> Value {
    json::parse(json_text).expect("arguments json")
}

/// Relatum's intentional MCP output changes, folded on both sides:
/// relatum_ls's opt-in limit, since, and details arguments are dropped from
/// its input schema, tool descriptions are compared after docstring dedent
/// (Python 3.13 does that at compile time; 3.12 keeps the indentation), and
/// a tool result's
/// JSON text is compared as data, because Relatum writes it compact where
/// Examen indented it. `structuredContent` is still compared as-is.
fn fold_intentional(responses: &mut [Value]) {
    for entry in responses.iter_mut() {
        let Value::Array(pair) = entry else { continue };
        let Some(reply) = pair.get_mut(1) else { continue };
        let Some(result) = member(reply, "result") else { continue };
        if let Some(Value::Array(tools)) = member(result, "tools") {
            for tool in tools.iter_mut() {
                if let Some(Value::String(text)) = member(tool, "description") {
                    *text = dedent_doc(text);
                }
                let is_ls = matches!(member(tool, "name"), Some(Value::String(name)) if name.ends_with("_ls"));
                if is_ls {
                    let properties = member(tool, "inputSchema").and_then(|schema| member(schema, "properties"));
                    if let Some(Value::Object(pairs)) = properties {
                        pairs.retain(|(key, _)| !RELATUM_LS_ARGS.contains(&key.as_str()));
                    }
                }
            }
        }
        if let Some(Value::Array(content)) = member(result, "content") {
            for item in content.iter_mut() {
                if let Some(Value::String(text)) = member(item, "text") {
                    if let Ok(parsed) = json::parse(text) {
                        *text = dumps_sorted(&parsed);
                    }
                }
            }
        }
    }
}

/// Opt-in relatum_ls arguments Examen does not have.
const RELATUM_LS_ARGS: [&str; 3] = ["limit", "since", "details"];

fn member<'a>(value: &'a mut Value, key: &str) -> Option<&'a mut Value> {
    match value {
        Value::Object(pairs) => pairs.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// CPython 3.13's compile-time docstring cleanup: strip the first line's
/// leading whitespace, remove the common indentation of the other lines,
/// and empty whitespace-only lines.
fn dedent_doc(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let indent = |line: &str| line.len() - line.trim_start_matches([' ', '\t']).len();
    let margin = lines
        .iter()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| indent(line))
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if i == 0 {
                line.trim_start_matches([' ', '\t']).to_string()
            } else if line.trim().is_empty() {
                String::new()
            } else {
                line[margin.min(indent(line))..].to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn mcp_suite(python: &str, legacy: &Path, binary_mcp: &Path, diffs: &mut Diffs) {
    let py_dir = mkdtemp("py-mcp-");
    let rs_dir = mkdtemp("rs-mcp-");
    let calls_py: Vec<(String, Value)> = vec![
        ("examen_init", arg(r#"{"path": "."}"#)),
        ("examen_ls", arg("{}")),
        ("examen_check", arg("{}")),
        ("examen_new", arg(r#"{"id": "net/retry", "kind": "decision", "summary": "backoff", "body": "use jitter", "author": "parent", "tags": ["net", "retry"]}"#)),
        ("examen_new", arg(r#"{"id": "net/linear", "kind": "decision", "summary": "old", "body": "1s then 2s", "author": "parent"}"#)),
        ("examen_supersede", arg(r#"{"old": "net/linear", "new": "net/retry"}"#)),
        ("examen_ls", arg(r#"{"scope": "net", "include_superseded": true}"#)),
        ("examen_get", arg(r#"{"ids": ["net/linear", "missing"]}"#)),
        ("examen_new", arg(r#"{"id": "x", "kind": "nope", "summary": "s", "body": "b"}"#)),
        ("examen_get", arg(r#"{"ids": []}"#)),
        ("examen_new", arg(r#"{"id": "only"}"#)),
        ("examen_init", arg(r#"{"path": "nested"}"#)),
    ]
    .into_iter()
    .map(|(name, args)| (name.to_string(), args))
    .collect();
    let calls_rs: Vec<(String, Value)> = calls_py
        .iter()
        .map(|(name, args)| (name.replace("examen_", "relatum_"), args.clone()))
        .collect();

    let mut py_cmd = Command::new(python);
    py_cmd
        .args(["-m", "examen_mcp"])
        .current_dir(&py_dir)
        .env("PYTHONPATH", legacy)
        .env("TZ", "UTC");
    let mut rs_cmd = Command::new(binary_mcp);
    rs_cmd.current_dir(&rs_dir).env("TZ", "UTC");
    let (mut py_resp, py_err) = mcp_round(py_cmd, &calls_py);
    let (mut rs_resp, rs_err) = mcp_round(rs_cmd, &calls_rs);
    fold_intentional(&mut py_resp);
    fold_intentional(&mut rs_resp);
    let folders = [py_dir.as_path(), rs_dir.as_path()];
    let py_norm = scrub_temp_names(&norm_mcp(&dumps_sorted(&json::arr(py_resp))), &folders);
    let rs_norm = scrub_temp_names(&norm_mcp(&dumps_sorted(&json::arr(rs_resp))), &folders);
    if py_norm != rs_norm {
        diffs.add("mcp responses", &py_norm, &rs_norm);
    }
    let py_err_n = scrub_temp_names(&logical_newlines(&norm_mcp(&py_err)), &folders);
    let rs_err_n = scrub_temp_names(&logical_newlines(&norm_mcp(&rs_err)), &folders);
    if py_err_n != rs_err_n {
        diffs.add("mcp stderr", &py_err_n, &rs_err_n);
    }
    let (snap_py, snap_rs) = (snapshot(&py_dir.join(".swarmnotes")), snapshot(&rs_dir.join(".swarmnotes")));
    if !snap_eq(&snap_py, &snap_rs) {
        diffs.add("mcp files", &snap_repr(&snap_py), &snap_repr(&snap_rs));
    }
    rmtree(&py_dir);
    rmtree(&rs_dir);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let default = |name: &str| path_str(&root().join("target").join("debug").join(name));
    let binary = absolute_tool(&args.first().cloned().unwrap_or_else(|| default("relatum")));
    let binary_mcp = absolute_tool(&args.get(1).cloned().unwrap_or_else(|| default("relatum-mcp")));
    std::env::set_var("TZ", "UTC");
    let python = python();
    let legacy = mkdtemp("examen-legacy-");
    extract_legacy(&legacy);
    let oracle = Oracle::start(&python, &legacy, "relatum");
    let mut diffs = Diffs { count: 0 };
    cli_suite(&oracle, &binary, &mut diffs);
    extra_suite(&oracle, &binary, &binary_mcp, &mut diffs);
    mcp_suite(&python, &legacy, &binary_mcp, &mut diffs);
    rmtree(&legacy);
    println!("diffs {}", diffs.count);
    std::process::exit(if diffs.count > 0 { 1 } else { 0 });
}

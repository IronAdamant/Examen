//! ls --limit / --since / --details (and the MCP equivalents). Without them,
//! ls output is unchanged.

use relatum::clock;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

static CWD: Mutex<()> = Mutex::new(());

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "relatum-lsopt-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run the CLI in `dir` with no SWARMNOTES_DIR.
fn cli(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let _guard = CWD.lock().unwrap_or_else(|poison| poison.into_inner());
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    std::env::remove_var("SWARMNOTES_DIR");
    let owned: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    let result = relatum::cli::run(&owned, "relatum", "", true);
    std::env::set_current_dir(old).unwrap();
    result
}

/// A store with four notes; `old/a` was last written in 2001.
fn store(tag: &str) -> PathBuf {
    let dir = scratch(tag);
    assert_eq!(cli(&dir, &["init"]).0, 0);
    for (id, author) in [("old/a", Some("parent")), ("w/b", Some("worker-a")), ("w/c", None)] {
        let mut args = vec!["new", id, "--kind", "finding", "--summary", id, "--body", "b"];
        if let Some(author) = author {
            args.extend(["--author", author]);
        }
        let (code, _, err) = cli(&dir, &args);
        assert_eq!(code, 0, "{err}");
    }
    let old = dir.join(".swarmnotes").join("old").join("a.md");
    let file = fs::OpenOptions::new().write(true).open(old).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(978_307_200)).unwrap(); // 2001-01-01
    dir
}

#[test]
fn utc_times_parse_and_format() {
    assert_eq!(clock::parse_utc("1970-01-01"), Some(0));
    assert_eq!(clock::parse_utc("2001-01-01"), Some(978_307_200));
    assert_eq!(clock::parse_utc("2026-11-05T14:30:09Z"), Some(1_793_889_009));
    assert_eq!(clock::format_utc(1_793_889_009), "2026-11-05T14:30:09Z");
    assert_eq!(clock::format_utc(951_782_400), "2000-02-29T00:00:00Z");
    for bad in ["2026-02-30", "2026-11-05T24:00:00Z", "2026-11-05T14:30:00", "yesterday", "2026-11-5"] {
        assert_eq!(clock::parse_utc(bad), None, "{bad}");
    }
}

#[test]
fn cli_limit_since_and_details() {
    let dir = store("cli");
    let (code, all, _) = cli(&dir, &["ls"]);
    assert_eq!(code, 0);
    assert_eq!(all.lines().count(), 4);
    assert!(!all.contains(" at ") && !all.contains("more"), "{all}");

    let (code, out, _) = cli(&dir, &["ls", "--limit", "2"]);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[..2], all.lines().take(2).collect::<Vec<_>>()[..]);
    assert_eq!(lines[2], "... 2 more (raise --limit, narrow the scope, or use --since)");
    assert_eq!(cli(&dir, &["ls", "--limit", "9"]).1, all);

    let (code, out, _) = cli(&dir, &["ls", "--since", "2020-01-01"]);
    assert_eq!(code, 0);
    assert!(!out.contains("old/a") && out.contains("w/b") && out.contains("_shared/report-format"), "{out}");
    assert_eq!(cli(&dir, &["ls", "--since", "2000-01-01"]).1, all);
    assert_eq!(cli(&dir, &["ls", "--since", "2999-01-01T00:00:00Z"]).1, "");

    let (code, out, _) = cli(&dir, &["ls", "w", "--details"]);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with("w/b") && lines[0].contains("  by worker-a  at 20"), "{out}");
    assert!(lines[1].starts_with("w/c") && !lines[1].contains(" by "), "{out}");
    assert!(lines[1].ends_with('Z'), "{out}");

    let (code, _, err) = cli(&dir, &["ls", "--limit", "0"]);
    assert_eq!((code, err.as_str()), (1, "relatum: limit must be at least 1\n"));
    let (code, _, err) = cli(&dir, &["ls", "--limit", "two"]);
    assert_eq!(code, 2);
    assert!(err.ends_with("relatum ls: error: argument --limit: invalid int value: 'two'\n"), "{err}");
    let (code, _, err) = cli(&dir, &["ls", "--since", "last week"]);
    assert_eq!(code, 1);
    assert_eq!(
        err,
        "relatum: since must be YYYY-MM-DD or YYYY-MM-DDTHH:MM:SSZ (UTC), not 'last week'\n"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn mcp_limit_since_and_details() {
    use relatum::json::{self, Value};
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let dir = store("mcp");
    let mut child = Command::new(env!("CARGO_BIN_EXE_relatum-mcp"))
        .current_dir(&dir)
        .env_remove("SWARMNOTES_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut call = |args: &str| -> Value {
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"relatum_ls","arguments":{args}}}}}"#
        )
        .unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        json::parse(line.trim_end()).unwrap().get("result").cloned().unwrap()
    };
    let structured = |result: &Value| result.get("structuredContent").and_then(|s| s.get("result")).cloned().unwrap();
    let ids = |result: &Value| -> Vec<String> {
        structured(result).get("notes").and_then(Value::as_array).unwrap().iter()
            .map(|note| note.get("id").and_then(Value::as_str).unwrap().to_string())
            .collect()
    };

    let plain = call("{}");
    assert_eq!(ids(&plain).len(), 4);
    let text = json::write_compact(&structured(&plain));
    assert!(!text.contains("modified") && !text.contains("more"), "{text}");

    let limited = call(r#"{"limit":3}"#);
    assert_eq!(ids(&limited), ids(&plain)[..3].to_vec());
    assert_eq!(structured(&limited).get("more"), Some(&Value::Number("1".into())));

    let recent = call(r#"{"since":"2020-01-01"}"#);
    assert!(!ids(&recent).contains(&"old/a".to_string()));
    assert_eq!(ids(&recent).len(), 3);

    let detailed = call(r#"{"scope":"w","details":true}"#);
    let notes = structured(&detailed).get("notes").and_then(Value::as_array).unwrap().to_vec();
    assert_eq!(notes[0].get("author").and_then(Value::as_str), Some("worker-a"));
    assert_eq!(notes[0].get("updated").and_then(Value::as_str), Some(clock::today().as_str()));
    assert!(notes[1].get("author").is_none());
    let modified = notes[1].get("modified").and_then(Value::as_str).unwrap().to_string();
    assert!(clock::parse_utc(&modified).is_some(), "{modified}");
    // A modified value works as the next since.
    assert!(ids(&call(&format!(r#"{{"since":"{modified}"}}"#))).contains(&"w/c".to_string()));

    for (args, needle) in [
        (r#"{"limit":0}"#, "limit must be at least 1"),
        (r#"{"limit":"5"}"#, "Input should be a valid integer"),
        (r#"{"since":"soon"}"#, "since must be YYYY-MM-DD or YYYY-MM-DDTHH:MM:SSZ (UTC)"),
        (r#"{"details":"yes"}"#, "Input should be a valid boolean"),
    ] {
        let result = call(args);
        assert_eq!(result.get("isError").and_then(Value::as_bool), Some(true), "{args}");
        let text = result.get("content").and_then(Value::as_array).unwrap()[0].get("text").and_then(Value::as_str).unwrap();
        assert!(text.contains(needle), "{args}: {text}");
    }
    drop(stdin);
    child.wait().unwrap();
    let _ = fs::remove_dir_all(dir);
}

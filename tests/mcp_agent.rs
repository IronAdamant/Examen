//! What an agent sees over MCP: lean descriptions, compact results, and
//! errors that name the next step.

use relatum::json::{self, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "relatum-agent-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run one stdio session in `cwd` and return the result of each tools/call.
fn session(cwd: &Path, calls: &[(&str, &str)]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_relatum-mcp"))
        .current_dir(cwd)
        .env_remove("SWARMNOTES_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn relatum-mcp");
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut send = |line: String| -> Value {
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        let mut reply = String::new();
        reader.read_line(&mut reply).unwrap();
        json::parse(reply.trim_end()).unwrap()
    };
    send(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#.to_string());
    let results = calls
        .iter()
        .enumerate()
        .map(|(i, (name, args))| {
            let reply = send(format!(
                r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"{name}","arguments":{args}}}}}"#,
                i + 1
            ));
            reply.get("result").cloned().expect("result")
        })
        .collect();
    drop(stdin);
    child.wait().unwrap();
    results
}

fn text(result: &Value) -> &str {
    result.get("content").and_then(Value::as_array).unwrap()[0]
        .get("text")
        .and_then(Value::as_str)
        .unwrap()
}

fn is_error(result: &Value) -> bool {
    result.get("isError").and_then(Value::as_bool).unwrap()
}

#[test]
fn tool_descriptions_carry_no_docstring_indentation() {
    let tools = relatum::mcp_schema::tools();
    for tool in tools.as_array().unwrap() {
        let description = tool.get("description").and_then(Value::as_str).unwrap();
        for line in description.lines() {
            assert_eq!(line, line.trim(), "padded line in {description:?}");
        }
        assert!(description.ends_with(".\n"), "{description:?}");
    }
}

#[test]
fn results_are_compact_json_matching_structured_content() {
    let dir = scratch("compact");
    let results = session(
        &dir,
        &[
            ("relatum_init", "{}"),
            ("relatum_ls", "{}"),
            ("relatum_get", r#"{"ids":["_shared/report-format"]}"#),
        ],
    );
    for result in &results {
        assert!(!is_error(result), "{}", text(result));
        let structured = result.get("structuredContent").and_then(|s| s.get("result")).unwrap();
        assert_eq!(text(result), json::write_compact(structured));
        assert!(!text(result).contains("\n  "), "{}", text(result));
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn errors_name_the_next_step() {
    let dir = scratch("errors");
    let results = session(
        &dir,
        &[
            ("relatum_ls", "{}"),
            ("relatum_init", "{}"),
            ("relatum_ls", r#"{"scope":"work/a"}"#),
            ("relatum_ls", r#"{"scope":"Not A Scope"}"#),
            ("relatum_new", r#"{"id":"Plan/Bad Id","kind":"context","summary":"s","body":"b"}"#),
            ("relatum_get", r#"{"ids":["Bad Id"]}"#),
            ("relatum_new", r#"{"id":"work/a/first","kind":"finding","summary":"s","body":"b"}"#),
            ("relatum_ls", r#"{"scope":"work/a"}"#),
        ],
    );
    let expect_error = |i: usize, needle: &str| {
        assert!(is_error(&results[i]), "call {i}: {}", text(&results[i]));
        assert!(text(&results[i]).contains(needle), "call {i}: {}", text(&results[i]));
    };
    expect_error(0, "Call relatum_init to create one there");
    assert!(text(&results[0]).contains("SWARMNOTES_DIR"));
    assert!(!is_error(&results[1]));
    expect_error(2, "no such scope: work/a. Nothing is written there yet; relatum_new with an id like work/a/<slug> creates it.");
    expect_error(3, "no such scope: Not A Scope. relatum_ls with no scope lists every note.");
    expect_error(4, "invalid note id: 'Plan/Bad Id'. Ids are lowercase segments joined by /, each matching _?[a-z0-9][a-z0-9-]*, e.g. net/retry-decision.");
    let got = results[5].get("structuredContent").and_then(|s| s.get("result")).unwrap();
    let error = got.get("errors").and_then(Value::as_array).unwrap()[0]
        .get("error")
        .and_then(Value::as_str)
        .unwrap();
    assert!(error.ends_with("e.g. net/retry-decision."), "{error}");
    assert!(!is_error(&results[6]), "{}", text(&results[6]));
    assert!(!is_error(&results[7]), "{}", text(&results[7]));
    assert!(text(&results[7]).contains("work/a/first"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn validation_errors_are_one_actionable_line() {
    let dir = scratch("validation");
    let long = "x".repeat(60);
    let results = session(
        &dir,
        &[
            ("relatum_init", "{}"),
            ("relatum_new", r#"{"id":"only"}"#),
            ("relatum_new", r#"{"id":"a","kind":"finding","summary":"s"}"#),
            ("relatum_new", r#"{"id":"a","kind":"finding","summary":5,"body":"b"}"#),
            ("relatum_new", r#"{"id":"a","kind":"finding","summary":"s","body":"b","tags":"net"}"#),
            ("relatum_new", r#"{"id":"a","kind":"finding","summary":"s","body":"b","tags":["net",3]}"#),
            ("relatum_get", r#"{"ids":"net/retry"}"#),
            ("relatum_get", "{}"),
            ("relatum_supersede", r#"{"old":"a"}"#),
            ("relatum_ls", r#"{"include_superseded":"yes"}"#),
            ("relatum_ls", &format!(r#"{{"scope":["{long}"]}}"#)),
            ("relatum_init", r#"{"path":null}"#),
        ],
    );
    let expected = [
        "missing required arguments kind, summary, body. relatum_new needs id, kind, summary, and body.",
        "missing required argument body. relatum_new needs id, kind, summary, and body.",
        "summary must be a string, got 5 (number). Call relatum_new again with summary like \"text\".",
        "tags must be a list of strings, got \"net\" (string). Call relatum_new again with tags like [\"net/retry\"].",
        "tags must be a list of strings, got [\"net\",3] (list). Call relatum_new again with tags like [\"net/retry\"].",
        "ids must be a list of strings, got \"net/retry\" (string). Call relatum_get again with ids like [\"net/retry\"].",
        "missing required argument ids. relatum_get needs ids.",
        "missing required argument new. relatum_supersede needs old and new.",
        "include_superseded must be true or false, got \"yes\" (string). Call relatum_ls again with include_superseded like true.",
        "scope must be a string, got a long list. Call relatum_ls again with scope like \"text\".",
        "path must be a string, got null. Call relatum_init again with path like \"text\".",
    ];
    assert!(!is_error(&results[0]));
    for (result, want) in results[1..].iter().zip(expected) {
        assert!(is_error(result), "{want}");
        let got = text(result);
        let tool = got.strip_prefix("Error executing tool ").unwrap().split(':').next().unwrap();
        assert_eq!(got, format!("Error executing tool {tool}: invalid arguments: {want}"));
        assert!(!got.contains('\n') && !got.contains("pydantic") && !got.contains("http"), "{got}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn edit_corrects_own_live_notes_and_points_elsewhere_otherwise() {
    let dir = scratch("edit");
    let results = session(
        &dir,
        &[
            ("relatum_init", "{}"),
            ("relatum_new", r#"{"id":"plan/split","kind":"decision","summary":"Split work","body":"a does x","author":"parent","tags":["plan"]}"#),
            ("relatum_edit", r#"{"id":"plan/split","author":"parent","summary":"Split work two ways","body":"a does x, b does y","tags":["plan","v2"]}"#),
            ("relatum_get", r#"{"ids":["plan/split"]}"#),
            ("relatum_edit", r#"{"id":"plan/split","author":"worker-a","body":"I would rather do y"}"#),
            ("relatum_edit", r#"{"id":"plan/split","author":"parent"}"#),
            ("relatum_edit", r#"{"id":"plan/split","author":"parent","tags":[]}"#),
            ("relatum_new", r#"{"id":"plan/split","kind":"decision","summary":"again","body":"b"}"#),
            ("relatum_new", r#"{"id":"plan/v3","kind":"decision","summary":"Three ways","body":"c too"}"#),
            ("relatum_supersede", r#"{"old":"plan/split","new":"plan/v3"}"#),
            ("relatum_edit", r#"{"id":"plan/split","author":"parent","body":"late fix"}"#),
            ("relatum_edit", r#"{"id":"plan/v3","author":"worker-b","summary":"Three ways, b owns c"}"#),
            ("relatum_edit", r#"{"id":"plan/v3"}"#),
            ("relatum_edit", r#"{"id":"plan/v3","author":"worker-b","summary":"  "}"#),
            ("relatum_check", "{}"),
        ],
    );
    let structured = |i: usize| results[i].get("structuredContent").and_then(|s| s.get("result")).cloned().unwrap();
    let ok = |i: usize| assert!(!is_error(&results[i]), "call {i}: {}", text(&results[i]));
    let err = |i: usize, needle: &str| {
        assert!(is_error(&results[i]), "call {i}: {}", text(&results[i]));
        assert!(text(&results[i]).contains(needle), "call {i}: {}", text(&results[i]));
    };
    ok(2);
    let today = relatum::clock::today();
    assert_eq!(structured(2).get("updated").and_then(Value::as_str), Some(today.as_str()));
    let note = structured(3).get("notes").and_then(Value::as_array).unwrap()[0].clone();
    assert_eq!(note.get("summary").and_then(Value::as_str), Some("Split work two ways"));
    assert_eq!(note.get("body").and_then(Value::as_str), Some("a does x, b does y"));
    assert_eq!(note.get("kind").and_then(Value::as_str), Some("decision"));
    assert_eq!(note.get("author").and_then(Value::as_str), Some("parent"));
    assert_eq!(json::write_compact(note.get("tags").unwrap()), r#"["plan","v2"]"#);
    err(4, "plan/split was written by parent, not worker-a; only its author edits it in place. Write a finding with your view, or write a replacement under a new id and call relatum_supersede with old set to this note.");
    err(5, "nothing to change: pass summary, body, or tags");
    ok(6);
    let file = std::fs::read_to_string(dir.join(".swarmnotes/plan/split.md")).unwrap();
    assert!(!file.contains("tags:"), "{file}");
    err(7, "note already exists: plan/split. Pick another id. To change plan/split: relatum_edit for a small fix by its author, or write the replacement under a new id and call relatum_supersede with old=plan/split.");
    ok(8);
    ok(9);
    err(10, "plan/split is superseded by plan/v3; edit the live note plan/v3 instead");
    ok(11); // plan/v3 had no author; the editor becomes its author
    let file = std::fs::read_to_string(dir.join(".swarmnotes/plan/v3.md")).unwrap();
    assert!(file.contains("author: worker-b") && file.contains("summary: Three ways, b owns c"), "{file}");
    err(12, "invalid arguments: missing required argument author. relatum_edit needs id and author.");
    err(13, "missing summary");
    ok(14);
    assert_eq!(structured(14).get("ok"), Some(&Value::Bool(true)));
    let _ = std::fs::remove_dir_all(dir);
}

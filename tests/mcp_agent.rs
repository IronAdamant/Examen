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

//! RELATUM_TOOLS / RELATUM_DISABLE (the suite's MCP tool-groups convention).

use relatum::json::{self, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#;
const LIST: &str = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;

struct Run {
    /// Raw response lines, in order.
    lines: Vec<String>,
    stderr: String,
    code: i32,
}

/// One server session with the given variables and extra request lines.
fn serve(env: &[(&str, &str)], extra: &[String]) -> Run {
    // A fresh directory each time, so no AGENTS.md or store is inherited.
    let dir = std::env::temp_dir().join(format!(
        "relatum-groups-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_relatum-mcp"));
    cmd.current_dir(&dir)
        .env_remove("RELATUM_TOOLS")
        .env_remove("RELATUM_DISABLE")
        .env_remove("SWARMNOTES_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn().expect("spawn relatum-mcp");
    {
        let mut stdin = child.stdin.take().unwrap();
        // The server may already have exited on a bad variable.
        let _ = writeln!(stdin, "{INIT}\n{LIST}");
        for line in extra {
            let _ = writeln!(stdin, "{line}");
        }
    }
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    Run {
        lines: BufReader::new(&out.stdout[..]).lines().map(Result::unwrap).collect(),
        stderr: String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n"),
        code: out.status.code().unwrap_or(-1),
    }
}

/// The raw `result` of the tools/list response line.
fn list_result(run: &Run) -> &str {
    let line = &run.lines[1];
    let prefix = r#"{"jsonrpc":"2.0","id":2,"result":"#;
    assert!(line.starts_with(prefix), "{line}");
    &line[prefix.len()..line.len() - 1]
}

fn names(run: &Run) -> Vec<String> {
    let value = json::parse(list_result(run)).unwrap();
    value.get("tools").and_then(Value::as_array).unwrap().iter()
        .map(|tool| tool.get("name").and_then(Value::as_str).unwrap().to_string())
        .collect()
}

fn instructions(run: &Run) -> String {
    let value = json::parse(&run.lines[0]).unwrap();
    value.get("result").and_then(|r| r.get("instructions")).and_then(Value::as_str).unwrap().to_string()
}

fn call(id: usize, tool: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{tool}","arguments":{{}}}}}}"#)
}

fn call_text(run: &Run, index: usize) -> (bool, String) {
    let value = json::parse(&run.lines[index]).unwrap();
    let result = value.get("result").unwrap();
    let text = result.get("content").and_then(Value::as_array).unwrap()[0]
        .get("text").and_then(Value::as_str).unwrap().to_string();
    (result.get("isError").and_then(Value::as_bool).unwrap(), text)
}

fn tools(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| format!("relatum_{name}")).collect()
}

const ALL: [&str; 7] = ["init", "ls", "get", "new", "edit", "supersede", "check"];

/// The groups line when every group is on (only shown once something is configured).
const ALL_ON: &str = "Tool groups on: core, revise, lint, setup. Off: none. To change them, add a line such as \"relatum: disable lint\" to an agent-tools block in AGENTS.md";

#[test]
fn default_surface_is_1_1_0_except_list_changed() {
    let run = serve(&[], &[]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(list_result(&run), include_str!("fixtures/mcp/tools_list_1.1.0.json"));
    assert_eq!(instructions(&run), include_str!("fixtures/mcp/instructions_1.1.0.txt"));
    // The only change to default initialize output: tools.listChanged is true.
    let init = json::parse(&run.lines[0]).unwrap();
    let capabilities = init.get("result").and_then(|r| r.get("capabilities")).unwrap();
    assert_eq!(
        json::write_compact(capabilities),
        r#"{"prompts":{"listChanged":false},"resources":{"listChanged":false,"subscribe":false},"tools":{"listChanged":true}}"#
    );
    // Empty values count as unset.
    let empty = serve(&[("RELATUM_TOOLS", ""), ("RELATUM_DISABLE", "  ")], &[]);
    assert_eq!(list_result(&empty), list_result(&run));
    assert_eq!(instructions(&empty), instructions(&run));
    // `all` lists everything and only adds the one-line summary.
    let all = serve(&[("RELATUM_TOOLS", "all")], &[]);
    assert_eq!(list_result(&all), list_result(&run));
    assert_eq!(
        instructions(&all),
        format!(
            "{}{ALL_ON}; RELATUM_TOOLS / RELATUM_DISABLE are set and take precedence.\n",
            include_str!("fixtures/mcp/instructions_1.1.0.txt")
        )
    );
}

#[test]
fn tools_and_disable_filter_by_group_and_by_tool() {
    let cases: [(&[(&str, &str)], &[&str]); 7] = [
        (&[("RELATUM_TOOLS", "lint")], &["ls", "get", "new", "check"]),
        (&[("RELATUM_TOOLS", " Relatum_Edit , INIT ")], &["init", "ls", "get", "new", "edit"]),
        (&[("RELATUM_TOOLS", "supersede")], &["ls", "get", "new", "supersede"]),
        (&[("RELATUM_DISABLE", "revise")], &["init", "ls", "get", "new", "check"]),
        (&[("RELATUM_DISABLE", "relatum_check,setup")], &["ls", "get", "new", "edit", "supersede"]),
        (
            &[("RELATUM_TOOLS", "revise,lint"), ("RELATUM_DISABLE", "relatum_supersede")],
            &["ls", "get", "new", "edit", "check"],
        ),
        (&[("RELATUM_TOOLS", "core")], &["ls", "get", "new"]),
    ];
    for (env, want) in cases {
        let run = serve(env, &[]);
        assert_eq!(run.code, 0, "{env:?}: {}", run.stderr);
        assert_eq!(names(&run), tools(want), "{env:?}");
    }
    let summary = instructions(&serve(&[("RELATUM_TOOLS", "relatum_edit"), ("RELATUM_DISABLE", "lint")], &[]));
    assert!(
        summary.ends_with(
            "Tool groups on: core, revise (only relatum_edit). Off: lint, setup. To change them, add a line such as \"relatum: disable lint\" to an agent-tools block in AGENTS.md; RELATUM_TOOLS / RELATUM_DISABLE are set and take precedence.\n"
        ),
        "{summary}"
    );
    assert_eq!(names(&serve(&[], &[])), tools(&ALL));
}

#[test]
fn a_disabled_tool_says_which_switch_to_change() {
    let run = serve(
        &[("RELATUM_TOOLS", "revise,setup"), ("RELATUM_DISABLE", "relatum_edit")],
        &[call(3, "relatum_check"), call(4, "relatum_edit"), call(5, "relatum_ls")],
    );
    assert_eq!(
        call_text(&run, 2),
        (true, r#"tool "relatum_check" is disabled (group "lint") by RELATUM_TOOLS; add "lint" to it to enable"#.to_string())
    );
    assert_eq!(
        call_text(&run, 3),
        (true, r#"tool "relatum_edit" is disabled (group "revise") by RELATUM_DISABLE; remove "relatum_edit" from it to enable"#.to_string())
    );
    // Core tools still run (here: no store, so a normal tool error).
    let (is_error, text) = call_text(&run, 4);
    assert!(is_error && text.contains("Call relatum_init"), "{text}");
    let by_group = serve(&[("RELATUM_DISABLE", "revise")], &[call(3, "relatum_supersede")]);
    assert_eq!(
        call_text(&by_group, 2).1,
        r#"tool "relatum_supersede" is disabled (group "revise") by RELATUM_DISABLE; remove "revise" from it to enable"#
    );
}

#[test]
fn unknown_names_stop_the_server_with_exit_2() {
    let catalogue = "groups: core, revise, lint, setup; tools: relatum_ls, relatum_get, relatum_new, relatum_edit, relatum_supersede, relatum_check, relatum_init";
    for (var, value, bad) in [
        ("RELATUM_DISABLE", "lokcs", "lokcs"),
        ("RELATUM_TOOLS", "lint, Notes", "notes"),
        ("RELATUM_DISABLE", "all", "all"),
    ] {
        let run = serve(&[(var, value)], &[]);
        assert_eq!(run.code, 2, "{var}={value}");
        assert!(run.lines.is_empty(), "{var}={value}: {:?}", run.lines);
        assert_eq!(run.stderr, format!("{var}: unknown group \"{bad}\"; {catalogue}\n"));
    }
}

#[test]
fn core_cannot_be_disabled() {
    for value in ["core", "relatum_get", "lint, ls"] {
        let run = serve(&[("RELATUM_DISABLE", value)], &[]);
        assert_eq!(run.code, 2, "{value}");
        assert!(
            run.stderr.starts_with("RELATUM_DISABLE: group \"core\" is always on and cannot be disabled"),
            "{value}: {}",
            run.stderr
        );
    }
    // An allowlist without core still keeps it.
    assert_eq!(names(&serve(&[("RELATUM_TOOLS", "setup")], &[])), tools(&["init", "ls", "get", "new"]));
}

#[test]
fn the_cli_ignores_the_switches() {
    let out = Command::new(env!("CARGO_BIN_EXE_relatum"))
        .args(["--help"])
        .env("RELATUM_TOOLS", "nonsense")
        .env("RELATUM_DISABLE", "core")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("{init,new,ls,get,supersede,check}"));
}

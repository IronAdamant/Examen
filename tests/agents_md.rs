//! Agent-chosen tool groups: the `agent-tools` block in the project's
//! AGENTS.md, env precedence per key, warnings instead of failures, and
//! live tools/list_changed notifications.

use relatum::json::{self, Value};
use relatum::mcp_groups::parse_agents_md;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const GROUPS: &str = "groups: core, revise, lint, setup";

fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "relatum-agentsmd-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn agents_md(dir: &Path, block: &str) {
    fs::write(dir.join("AGENTS.md"), format!("# Project\n\nNotes for agents.\n\n```agent-tools\n{block}```\n")).unwrap();
}

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: usize,
}

impl Session {
    fn start(cwd: &Path, env: &[(&str, &str)]) -> Session {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_relatum-mcp"));
        cmd.current_dir(cwd)
            .env_remove("RELATUM_TOOLS")
            .env_remove("RELATUM_DISABLE")
            .env_remove("SWARMNOTES_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, value) in env {
            cmd.env(key, value);
        }
        let mut child = cmd.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Session { child, stdin, stdout, next: 1 }
    }

    /// Send a request; return (notifications seen first, the response).
    fn request(&mut self, method: &str, params: &str) -> (Vec<String>, Value) {
        let id = self.next;
        self.next += 1;
        writeln!(self.stdin, r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#).unwrap();
        self.stdin.flush().unwrap();
        let mut notes = Vec::new();
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).unwrap();
            let value = json::parse(line.trim_end()).unwrap();
            match value.get("method").and_then(Value::as_str) {
                Some(method) => notes.push(method.to_string()),
                None => return (notes, value.get("result").cloned().unwrap()),
            }
        }
    }

    fn initialize(&mut self) -> Value {
        self.request(
            "initialize",
            r#"{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}"#,
        )
        .1
    }

    fn tools(&mut self) -> (Vec<String>, Vec<String>) {
        let (notes, result) = self.request("tools/list", "{}");
        let names = result.get("tools").and_then(Value::as_array).unwrap().iter()
            .map(|tool| tool.get("name").and_then(Value::as_str).unwrap().trim_start_matches("relatum_").to_string())
            .collect();
        (notes, names)
    }

    fn call_text(&mut self, tool: &str) -> String {
        let (_, result) = self.request("tools/call", &format!(r#"{{"name":"{tool}","arguments":{{}}}}"#));
        result.get("content").and_then(Value::as_array).unwrap()[0]
            .get("text").and_then(Value::as_str).unwrap().to_string()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn instructions(init: &Value) -> String {
    init.get("instructions").and_then(Value::as_str).unwrap().to_string()
}

/// The instructions lines after the 1.1.0 text.
fn tail(init: &Value) -> Vec<String> {
    instructions(init)
        .strip_prefix(include_str!("fixtures/mcp/instructions_1.1.0.txt"))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

#[test]
fn block_parsing() {
    let text = "\
# Title

```rust
relatum: disable lint
```

~~~agent-tools extra words
# comment line
trammel: disable locks, notes
Relatum: disable   lint setup   # trailing comment
relatum: tools revise
relatum: disable lokcs
relatum: disable core
relatum: enable lint
not a rule
~~~

```agent-tools
relatum: disable revise
```
";
    let rules = parse_agents_md(text);
    assert_eq!(rules.disable.as_deref(), Some("lint setup"));
    assert_eq!(rules.tools.as_deref(), Some("revise"));
    assert_eq!(
        rules.warnings,
        strings(&[
            &format!("AGENTS.md agent-tools: unknown group \"lokcs\" for relatum; {GROUPS}"),
            &format!("AGENTS.md agent-tools: group \"core\" is always on and cannot be disabled (\"core\") for relatum; {GROUPS}"),
            &format!("AGENTS.md agent-tools: unknown key \"enable\" (use tools or disable) for relatum; {GROUPS}"),
        ])
    );
    // Lines for one key add up.
    let rules = parse_agents_md("```agent-tools\nrelatum: disable lint\nrelatum: disable setup\n```\n");
    assert_eq!(rules.disable.as_deref(), Some("lint,setup"));
    // No block, or no lines for this tool: nothing, and no warning.
    for text in ["# just docs\n", "```agent-tools\ntrammel: disable locks\n```\n", ""] {
        assert_eq!(parse_agents_md(text), Default::default(), "{text:?}");
    }
}

#[test]
fn no_block_keeps_the_default_surface() {
    for (tag, file) in [
        ("none", None),
        ("noblock", Some("# Agents\n\n```sh\nrelatum ls\n```\n")),
        ("others", Some("```agent-tools\ntrammel: disable locks\n```\n")),
    ] {
        let dir = project(tag);
        if let Some(text) = file {
            fs::write(dir.join("AGENTS.md"), text).unwrap();
        }
        let mut session = Session::start(&dir, &[]);
        let init = session.initialize();
        assert!(tail(&init).is_empty(), "{tag}: {:?}", tail(&init));
        let tools_cap = init.get("capabilities").and_then(|c| c.get("tools")).unwrap();
        assert_eq!(json::write_compact(tools_cap), r#"{"listChanged":true}"#);
        let (notes, names) = session.tools();
        assert!(notes.is_empty());
        assert_eq!(names, strings(&["init", "ls", "get", "new", "edit", "supersede", "check"]));
        drop(session);
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn the_block_shapes_the_surface_from_the_store_root() {
    let dir = project("root");
    fs::create_dir_all(dir.join(".swarmnotes")).unwrap();
    fs::create_dir_all(dir.join("src").join("deep")).unwrap();
    agents_md(&dir, "chisel: disable locks\nrelatum: disable lint, relatum_init\n");
    // Started in a subdirectory: the project root is where .swarmnotes is.
    let mut session = Session::start(&dir.join("src").join("deep"), &[]);
    let init = session.initialize();
    assert_eq!(
        tail(&init),
        strings(&["Tool groups on: core, revise. Off: lint, setup. To change them, add a line such as \"relatum: disable lint\" to an agent-tools block in AGENTS.md."])
    );
    assert_eq!(session.tools().1, strings(&["ls", "get", "new", "edit", "supersede"]));
    assert_eq!(
        session.call_text("relatum_check"),
        "tool \"relatum_check\" is disabled (group \"lint\") by AGENTS.md (agent-tools line \"relatum: disable\"); remove \"lint\" from that line to enable"
    );
    assert_eq!(
        session.call_text("relatum_init"),
        "tool \"relatum_init\" is disabled (group \"setup\") by AGENTS.md (agent-tools line \"relatum: disable\"); remove \"relatum_init\" from that line to enable"
    );
    drop(session);
    // SWARMNOTES_DIR points at the store: its parent is the root.
    let elsewhere = project("elsewhere");
    let store = dir.join(".swarmnotes");
    let mut session = Session::start(&elsewhere, &[("SWARMNOTES_DIR", store.to_str().unwrap())]);
    session.initialize();
    assert_eq!(session.tools().1, strings(&["ls", "get", "new", "edit", "supersede"]));
    drop(session);
    let _ = fs::remove_dir_all(dir);
    let _ = fs::remove_dir_all(elsewhere);
}

#[test]
fn env_overrides_the_file_per_key() {
    let dir = project("precedence");
    agents_md(&dir, "relatum: tools lint\nrelatum: disable revise\n");
    let cases: [(&[(&str, &str)], &[&str]); 4] = [
        // File only: tools lint (+core), then disable revise.
        (&[], &["ls", "get", "new", "check"]),
        // RELATUM_DISABLE replaces the file's disable line, not its tools line.
        (&[("RELATUM_DISABLE", "lint")], &["ls", "get", "new"]),
        // RELATUM_TOOLS replaces the file's tools line; the file's disable still applies.
        (&[("RELATUM_TOOLS", "all")], &["init", "ls", "get", "new", "check"]),
        // Both set: the file is not used.
        (&[("RELATUM_TOOLS", "revise"), ("RELATUM_DISABLE", "relatum_edit")], &["ls", "get", "new", "supersede"]),
    ];
    for (env, want) in cases {
        let mut session = Session::start(&dir, env);
        session.initialize();
        assert_eq!(session.tools().1, strings(want), "{env:?}");
    }
    let mut session = Session::start(&dir, &[("RELATUM_DISABLE", "lint")]);
    session.initialize();
    assert!(session.call_text("relatum_check").ends_with("by RELATUM_DISABLE; remove \"lint\" from it to enable"));
    drop(session);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn file_mistakes_are_warnings_not_failures() {
    let dir = project("mistakes");
    agents_md(&dir, "relatum: disable lokcs\nrelatum: disable core\nrelatum: disable setup\n");
    let mut session = Session::start(&dir, &[]);
    let init = session.initialize();
    assert_eq!(
        tail(&init),
        strings(&[
            "Tool groups on: core, revise, lint. Off: setup. To change them, add a line such as \"relatum: disable lint\" to an agent-tools block in AGENTS.md.",
            &format!("AGENTS.md agent-tools: unknown group \"lokcs\" for relatum; {GROUPS}"),
            &format!("AGENTS.md agent-tools: group \"core\" is always on and cannot be disabled (\"core\") for relatum; {GROUPS}"),
        ])
    );
    // The bad lines are ignored; the good one applies.
    assert_eq!(session.tools().1, strings(&["ls", "get", "new", "edit", "supersede", "check"]));
    drop(session);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn editing_the_block_mid_session_notifies_and_applies() {
    let dir = project("live");
    let mut session = Session::start(&dir, &[]);
    session.initialize();
    let all = strings(&["init", "ls", "get", "new", "edit", "supersede", "check"]);
    assert_eq!(session.tools(), (vec![], all.clone()));

    agents_md(&dir, "relatum: disable revise\n");
    let (notes, names) = session.tools();
    assert_eq!(notes, strings(&["notifications/tools/list_changed"]));
    assert_eq!(names, strings(&["init", "ls", "get", "new", "check"]));

    // A change that leaves the same tools on (a comment) sends nothing.
    agents_md(&dir, "# one agent works here\nrelatum: disable revise\n");
    assert_eq!(session.tools(), (vec![], strings(&["init", "ls", "get", "new", "check"])));

    // Any request notices, not just tools/list.
    agents_md(&dir, "relatum: disable revise, lint\n");
    let (notes, _) = session.request("ping", "{}");
    assert_eq!(notes, strings(&["notifications/tools/list_changed"]));
    assert_eq!(session.tools(), (vec![], strings(&["init", "ls", "get", "new"])));

    fs::remove_file(dir.join("AGENTS.md")).unwrap();
    assert_eq!(session.tools(), (strings(&["notifications/tools/list_changed"]), all));
    drop(session);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn doc_examples_switch_nothing() {
    // Each doc shows an agent-tools block nested in an outer fence, so it is
    // text, not config, even when pasted into an AGENTS.md.
    for (name, text) in [
        ("AGENTS.snippet.md", include_str!("../AGENTS.snippet.md")),
        ("skills/relatum/SKILL.md", include_str!("../skills/relatum/SKILL.md")),
        ("README.md", include_str!("../README.md")),
        ("SPEC.md", include_str!("../SPEC.md")),
    ] {
        assert_eq!(parse_agents_md(text), Default::default(), "{name}");
    }
    assert!(include_str!("../AGENTS.snippet.md").contains("relatum: disable lint, setup"));
    assert!(include_str!("../README.md").contains("relatum: disable lint, setup"));
    // The same block outside the outer fence would be live.
    let live = parse_agents_md("```agent-tools\nrelatum: disable lint, setup\n```\n");
    assert_eq!(live.disable.as_deref(), Some("lint, setup"));
    // Nested inside ~~~ or longer backtick fences: still text.
    for outer in ["~~~text\n```agent-tools\nrelatum: disable lint\n```\n~~~\n", "`````md\n````agent-tools\nrelatum: disable lint\n````\n`````\n"] {
        assert_eq!(parse_agents_md(outer), Default::default(), "{outer:?}");
    }
}

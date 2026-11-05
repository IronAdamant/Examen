//! Stdio MCP server. Newline-delimited JSON-RPC, no third-party crate.

use crate::error::NoteError;
use crate::json::{self, Value};
use crate::mcp_groups::Surface;
use crate::mcp_schema;
use crate::mcp_support::{
    InvokeErr, bool_or, check_optional_str, optional_int, optional_string, optional_tags, path_is_null,
    require_present_str, require_str, require_string_list, store_for_init, string_or, type_error,
    validate_ls, Want,
};
use crate::ops::{self, TagsIn};
use crate::prog;
use crate::pystr::{py_list, py_repr};
use crate::grammar::{self, DIR_NAME};
use crate::store::{self, canonical_dir};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

pub fn main_mcp(args: &[String]) -> i32 {
    if !args.is_empty() {
        let nl = if cfg!(windows) { "\r\n" } else { "\n" };
        let _ = std::io::stderr().write_all(
            format!(
                "{name}-mcp takes no arguments. Set SWARMNOTES_DIR to choose a store.{nl}",
                name = prog::NAME
            )
            .as_bytes(),
        );
        return 2;
    }
    prog::set_current(prog::NAME);
    serve()
}

pub fn serve() -> i32 {
    // A typo in RELATUM_TOOLS / RELATUM_DISABLE stops the server, so the host
    // shows it as failed instead of serving a surprise surface.
    let surface = match Surface::from_env() {
        Ok(surface) => surface,
        Err(line) => {
            eprint_line(&line);
            return 2;
        }
    };
    let stdin = std::io::stdin();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let trimmed = line.trim_end_matches(['\r', '\n']);
                if trimmed.is_empty() {
                    continue;
                }
                if let Some(response) = handle_line(trimmed, &surface) {
                    let mut stdout = std::io::stdout();
                    let _ = stdout.write_all(response.as_bytes());
                    let _ = stdout.write_all(b"\n");
                    let _ = stdout.flush();
                }
            }
        }
    }
    0
}

fn handle_line(line: &str, surface: &Surface) -> Option<String> {
    let value = match json::parse(line) {
        Ok(value) => value,
        Err(_) => {
            return Some(rpc_error(&Value::Null, -32700, "Parse error", None));
        }
    };
    let Value::Object(_) = &value else {
        return Some(rpc_error(&Value::Null, -32600, "Invalid Request", None));
    };
    let Some(method) = value.get("method").and_then(Value::as_str) else {
        let id = value.get("id").cloned().unwrap_or(Value::Null);
        return Some(rpc_error(&id, -32600, "Invalid Request", None));
    };
    let id = value.get("id").cloned();
    let Some(id) = id else {
        return None;
    };
    let params = value.get("params");
    let result = match method {
        "initialize" => Ok(initialize(params, surface)),
        "ping" => Ok(json::obj(vec![])),
        "tools/list" => Ok(json::obj(vec![("tools", listed_tools(surface))])),
        "tools/call" => call_tool(params, surface),
        "resources/list" => Ok(json::obj(vec![("resources", json::arr(vec![]))])),
        "prompts/list" => Ok(json::obj(vec![("prompts", json::arr(vec![]))])),
        other => Err(rpc_error(&id, -32601, "Method not found", Some(json::s(other)))),
    };
    match result {
        Ok(body) => Some(rpc_ok(&id, body)),
        Err(message) => Some(message),
    }
}

/// tools/list, minus switched-off tools.
fn listed_tools(surface: &Surface) -> Value {
    let Value::Array(tools) = mcp_schema::tools() else {
        return mcp_schema::tools();
    };
    let prefix = format!("{}_", prog::NAME);
    Value::Array(
        tools
            .into_iter()
            .filter(|tool| {
                let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
                surface.is_on(name.strip_prefix(&prefix).unwrap_or(name))
            })
            .collect(),
    )
}

fn initialize(params: Option<&Value>, surface: &Surface) -> Value {
    let mut instructions = mcp_schema::instructions();
    if let Some(line) = surface.summary() {
        instructions.push_str(&line);
        instructions.push('\n');
    }
    let version = params
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or("2024-11-05");
    json::obj(vec![
        (
            "capabilities",
            json::obj(vec![
                (
                    "prompts",
                    json::obj(vec![("listChanged", json::bool_val(false))]),
                ),
                (
                    "resources",
                    json::obj(vec![
                        ("listChanged", json::bool_val(false)),
                        ("subscribe", json::bool_val(false)),
                    ]),
                ),
                (
                    "tools",
                    json::obj(vec![("listChanged", json::bool_val(false))]),
                ),
            ]),
        ),
        ("instructions", json::s(&instructions)),
        ("protocolVersion", json::s(version)),
        (
            "serverInfo",
            json::obj(vec![
                ("name", json::s(prog::NAME)),
                ("version", json::s(prog::VERSION)),
            ]),
        ),
    ])
}

fn call_tool(params: Option<&Value>, surface: &Surface) -> Result<Value, String> {
    let name = params
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let arguments = params
        .and_then(|params| params.get("arguments"))
        .cloned()
        .unwrap_or_else(|| json::obj(vec![]));
    if name.is_empty() || !known_tool(name) {
        let text = format!("Unknown tool: {name}");
        log_fail(name, &text);
        return Ok(tool_error(&text));
    }
    let suffix = name.strip_prefix(&format!("{}_", prog::NAME)).unwrap_or(name);
    if let Some(text) = surface.refusal(suffix) {
        log_fail(name, &text);
        return Ok(tool_error(&text));
    }
    match invoke(name, &arguments) {
        Ok(value) => Ok(tool_ok(value)),
        Err(InvokeErr::Note(message)) => {
            let text = format!("Error executing tool {name}: {message}");
            log_fail(name, &text);
            Ok(tool_error(&text))
        }
        Err(InvokeErr::Validation { fields, text }) => {
            log_rejected(name, &fields);
            let text = format!("Error executing tool {name}: {text}");
            Ok(tool_error(&text))
        }
    }
}

fn known_tool(name: &str) -> bool {
    ["init", "ls", "get", "new", "edit", "supersede", "check"]
        .iter()
        .any(|suffix| name == prog::tool(suffix))
}

fn invoke(name: &str, args: &Value) -> Result<Value, InvokeErr> {
    if name == prog::tool("init") {
        let path = require_str(args, "path", true)?;
        let path = path.unwrap_or(".");
        if path_is_null(args, "path") {
            return Err(type_error(name, "path", Want::Str, &Value::Null));
        }
        let store = store_for_init(path).map_err(|err| InvokeErr::Note(err.message))?;
        let root = store::ensure_store(&store).map_err(|err| InvokeErr::Note(err.message))?;
        let shown = canonical_dir(root);
        return Ok(json::obj(vec![
            ("root", json::s(&shown.display().to_string())),
            ("report_format", json::s("_shared/report-format")),
        ]));
    }
    if name == prog::tool("ls") {
        validate_ls(name, args)?;
        let scope = string_or(args, "scope", "");
        let kind = optional_string(args, "kind").filter(|text| !text.is_empty());
        let tag = optional_string(args, "tag").filter(|text| !text.is_empty());
        let include = bool_or(args, "include_superseded", false);
        let details = bool_or(args, "details", false);
        let limit = optional_int(name, args, "limit")?.map(|n| usize::try_from(n).unwrap_or(0));
        let since = optional_string(args, "since")
            .filter(|text| !text.is_empty())
            .map(|text| ops::parse_since(&text))
            .transpose()
            .map_err(note)?;
        let root = tool_root()?;
        check_scope(&root, scope)?;
        let query = ops::LsQuery {
            scope: scope.to_string(),
            kind,
            tag,
            include_superseded: include,
            since,
            limit,
            details,
        };
        let listing = ops::list_notes_with(&root, &query).map_err(note)?;
        let notes = listing
            .rows
            .into_iter()
            .map(|row| {
                let mut pairs = vec![
                    ("id", json::s(&row.id)),
                    ("kind", json::s(&row.kind)),
                    ("summary", json::s(&row.summary)),
                    ("status", json::s(&row.status)),
                ];
                if let Some(next) = row.superseded_by {
                    pairs.push(("superseded_by", json::s(&next)));
                }
                if let Some(details) = row.details {
                    if let Some(author) = details.author {
                        pairs.push(("author", json::s(&author)));
                    }
                    if let Some(updated) = details.updated {
                        pairs.push(("updated", json::s(&updated)));
                    }
                    pairs.push(("modified", json::s(&details.modified)));
                }
                json::obj(pairs)
            })
            .collect();
        let mut result = vec![("notes", json::arr(notes))];
        if listing.more > 0 {
            result.push(("more", Value::Number(listing.more.to_string())));
        }
        return Ok(json::obj(result));
    }
    if name == prog::tool("get") {
        let ids = require_string_list(name, args, "ids")?;
        if ids.is_empty() {
            return Err(InvokeErr::Note(format!(
                "{} needs at least one note id",
                prog::tool("get")
            )));
        }
        let root = tool_root()?;
        let mut notes = Vec::new();
        let mut errors = Vec::new();
        for id in ids {
            match ops::resolve(&root, &id) {
                Ok(resolved) => notes.push(fetched(&id, &resolved)),
                Err(err) => errors.push(json::obj(vec![
                    ("id", json::s(&id)),
                    ("error", json::s(&agent_message(err))),
                ])),
            }
        }
        return Ok(json::obj(vec![
            ("notes", json::arr(notes)),
            ("errors", json::arr(errors)),
        ]));
    }
    if name == prog::tool("new") {
        let id = require_present_str(name, args, "id")?;
        let kind = require_present_str(name, args, "kind")?;
        let summary = require_present_str(name, args, "summary")?;
        let body = require_present_str(name, args, "body")?;
        check_optional_str(name, args, "author")?;
        let tags = optional_tags(name, args)?;
        let author = optional_string(args, "author");
        let root = tool_root()?;
        ops::add_note(
            &root,
            &id,
            &kind,
            &summary,
            &body,
            author.as_deref(),
            tags.map(TagsIn::List),
        )
        .map_err(note)?;
        return Ok(json::obj(vec![("id", json::s(&id))]));
    }
    if name == prog::tool("supersede") {
        let old = require_present_str(name, args, "old")?;
        let new = require_present_str(name, args, "new")?;
        let root = tool_root()?;
        let (old, new) = ops::supersede_note(&root, &old, &new).map_err(note)?;
        return Ok(json::obj(vec![
            ("old", json::s(&old)),
            ("new", json::s(&new)),
        ]));
    }
    if name == prog::tool("edit") {
        let id = require_present_str(name, args, "id")?;
        let author = require_present_str(name, args, "author")?;
        check_optional_str(name, args, "summary")?;
        check_optional_str(name, args, "body")?;
        let change = ops::NoteEdit {
            summary: optional_string(args, "summary"),
            body: optional_string(args, "body"),
            tags: optional_tags(name, args)?,
        };
        let root = tool_root()?;
        let updated = ops::edit_note(&root, &id, &author, change).map_err(note)?;
        return Ok(json::obj(vec![("id", json::s(&id)), ("updated", json::s(&updated))]));
    }
    if name == prog::tool("check") {
        let root = tool_root()?;
        let (errors, warnings) = ops::check(&root).map_err(note)?;
        return Ok(json::obj(vec![
            ("ok", json::bool_val(errors.is_empty())),
            (
                "errors",
                json::arr(errors.iter().map(|item| json::s(item)).collect()),
            ),
            (
                "warnings",
                json::arr(warnings.iter().map(|item| json::s(item)).collect()),
            ),
        ]));
    }
    Err(InvokeErr::Note(format!("Unknown tool: {name}")))
}

fn fetched(requested: &str, resolved: &ops::Resolved) -> Value {
    let note = &resolved.note;
    let mut pairs = vec![
        ("requested_id", json::s(requested)),
        ("id", json::s(&resolved.final_id)),
        ("kind", json::s(note.get_str("kind").unwrap_or(""))),
        ("summary", json::s(note.get_str("summary").unwrap_or(""))),
        (
            "status",
            json::s(note.get_str("status").unwrap_or("active")),
        ),
        ("body", json::s(note.body.trim_end_matches('\n'))),
        (
            "chain",
            json::arr(resolved.chain.iter().map(|id| json::s(id)).collect()),
        ),
    ];
    if let Some(author) = note.get_str("author") {
        if !author.is_empty() {
            pairs.push(("author", json::s(author)));
        }
    }
    if let Some(crate::meta::MetaVal::List(tags)) = note.get("tags") {
        if !tags.is_empty() {
            pairs.push((
                "tags",
                json::arr(tags.iter().map(|tag| json::s(tag)).collect()),
            ));
        }
    }
    json::obj(pairs)
}

fn note(err: NoteError) -> InvokeErr {
    InvokeErr::Note(agent_message(err))
}

/// An agent cannot run the CLI's `init`, so a missing store names the tool
/// to call and the directory the server is looking from.
fn tool_root() -> Result<PathBuf, InvokeErr> {
    match store::locate().map_err(note)? {
        Some(root) => Ok(root),
        None => {
            let here = std::env::current_dir()
                .map(|dir| canonical_dir(dir).display().to_string())
                .unwrap_or_else(|_| ".".to_string());
            Err(InvokeErr::Note(format!(
                "no {DIR_NAME}/ found in {here} or any parent. Call {init} to create one \
                 there, or set SWARMNOTES_DIR to the store path in this server's MCP config.",
                init = prog::tool("init"),
            )))
        }
    }
}

/// A scope exists once a note is written under it. Say so instead of only
/// "no such scope", so a child listing its own fresh scope knows what to do.
fn check_scope(root: &Path, scope: &str) -> Result<(), InvokeErr> {
    let scope = scope.trim_matches('/');
    if store::scope_dir(root, scope).is_dir() {
        return Ok(());
    }
    let next = if grammar::valid_id(scope) {
        format!(
            "Nothing is written there yet; {new} with an id like {scope}/<slug> creates it.",
            new = prog::tool("new")
        )
    } else {
        format!("{ls} with no scope lists every note.", ls = prog::tool("ls"))
    };
    Err(InvokeErr::Note(format!("no such scope: {scope}. {next}")))
}

/// Error text for a tool result. An invalid id also says what a valid one
/// looks like.
fn agent_message(err: NoteError) -> String {
    let (edit, supersede) = (prog::tool("edit"), prog::tool("supersede"));
    if let Some(rest) = err.message.strip_prefix("note already exists: ") {
        let id = rest.trim_end_matches(" (pick another id)");
        return format!(
            "note already exists: {id}. Pick another id. To change {id}: {edit} for a small fix by \
             its author, or write the replacement under a new id and call {supersede} with old={id}."
        );
    }
    if err.message.contains("; only its author edits it in place") {
        return format!(
            "{}. Write a finding with your view, or write a replacement under a new id and call \
             {supersede} with old set to this note.",
            err.message
        );
    }
    if err.message.starts_with("invalid note id:") {
        return format!(
            "{}. Ids are lowercase segments joined by /, each matching {}, e.g. net/retry-decision.",
            err.message,
            grammar::SEGMENT
        );
    }
    err.message
}

fn tool_ok(value: Value) -> Value {
    json::obj(vec![
        (
            "content",
            json::arr(vec![json::obj(vec![
                // Compact: this text is what most hosts put in the model's context.
                ("text", json::s(&json::write_compact(&value))),
                ("type", json::s("text")),
            ])]),
        ),
        ("isError", json::bool_val(false)),
        (
            "structuredContent",
            json::obj(vec![("result", value)]),
        ),
    ])
}

fn tool_error(text: &str) -> Value {
    json::obj(vec![
        (
            "content",
            json::arr(vec![json::obj(vec![
                ("text", json::s(text)),
                ("type", json::s("text")),
            ])]),
        ),
        ("isError", json::bool_val(true)),
    ])
}

fn rpc_ok(id: &Value, result: Value) -> String {
    json::write_compact(&json::obj(vec![
        ("jsonrpc", json::s("2.0")),
        ("id", id.clone()),
        ("result", result),
    ]))
}

fn rpc_error(id: &Value, code: i64, message: &str, data: Option<Value>) -> String {
    let mut pairs = vec![
        ("code", Value::Number(code.to_string())),
        ("message", json::s(message)),
    ];
    if let Some(data) = data {
        pairs.push(("data", data));
    }
    json::write_compact(&json::obj(vec![
        ("jsonrpc", json::s("2.0")),
        ("id", id.clone()),
        ("error", json::obj(pairs)),
    ]))
}

fn eprint_line(text: &str) {
    // Python's logging StreamHandler writes through a text-mode stderr, which
    // uses CRLF on Windows. Match that so the log bytes are the same.
    let nl = if cfg!(windows) { "\r\n" } else { "\n" };
    let _ = std::io::stderr().write_all(format!("{text}{nl}").as_bytes());
}

fn log_fail(name: &str, text: &str) {
    eprint_line(&format!(
        "Tool {} failed: {}",
        py_repr(name),
        py_repr(text)
    ));
}

fn log_rejected(name: &str, fields: &[String]) {
    eprint_line(&format!(
        "Tool {} rejected arguments: {}",
        py_repr(name),
        py_list(fields)
    ));
}


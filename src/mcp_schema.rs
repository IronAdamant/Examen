//! Tool list payload. Field order matches the MCP Python SDK's JSON schema.

use crate::json::{self, Value};
use crate::prog;

pub fn instructions() -> String {
    format!(
        "\
{name_title} stores pass-by-reference notes as markdown files under .swarmnotes/.
Do not rename that directory. A note ID is its path without .md.

When you spawn subagents, write each shared fact once and send a brief:

Task: <what to do>
Read first: <note IDs>
Your scope: <directory under .swarmnotes/>
Done when: <checkable end condition>

Use these tools. They read and write the same files as the {name} CLI.
{get} follows superseded_by. {ls} hides superseded notes unless
include_superseded is true. If the working directory is not the project,
the host must set SWARMNOTES_DIR to the .swarmnotes path.
{init} creates a store only in the current working directory or at
SWARMNOTES_DIR.
",
        name_title = title_name(),
        name = prog::NAME,
        get = prog::tool("get"),
        ls = prog::tool("ls"),
        init = prog::tool("init"),
    )
}

fn title_name() -> String {
    let mut chars = prog::NAME.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub fn tools() -> Value {
    let supersede = format!(
        "\
Mark old as replaced by new. new must already exist and resolve to a live note.\n\
\n    \
Later {get} calls on old follow the chain. {ls} hides old unless\n    \
include_superseded is true.\n    ",
        get = prog::tool("get"),
        ls = prog::tool("ls"),
    );
    json::arr(vec![
        tool("init", INIT, init_schema(), false),
        tool("ls", LS, ls_schema(), false),
        tool("get", GET, get_schema(), true),
        tool("new", NEW, new_schema(), true),
        tool("supersede", &supersede, supersede_schema(), true),
        tool("check", CHECK, check_schema(), false),
    ])
}

fn tool(suffix: &str, description: &str, input: Value, _required: bool) -> Value {
    let name = prog::tool(suffix);
    json::obj(vec![
        ("description", json::s(description)),
        ("inputSchema", input),
        ("name", json::s(&name)),
        ("outputSchema", output_schema(&name)),
    ])
}

fn output_schema(name: &str) -> Value {
    json::obj(vec![
        (
            "properties",
            json::obj(vec![(
                "result",
                json::obj(vec![
                    ("additionalProperties", json::bool_val(true)),
                    ("title", json::s("Result")),
                    ("type", json::s("object")),
                ]),
            )]),
        ),
        ("required", json::arr(vec![json::s("result")])),
        ("type", json::s("object")),
        ("title", json::s(&format!("{name}Output"))),
    ])
}

fn schema(name: &str, properties: Value, required: &[&str]) -> Value {
    let mut pairs = vec![("properties", properties)];
    if !required.is_empty() {
        pairs.push((
            "required",
            json::arr(required.iter().map(|item| json::s(item)).collect()),
        ));
    }
    pairs.push(("type", json::s("object")));
    pairs.push(("title", json::s(&format!("{name}Arguments"))));
    json::obj(pairs)
}

fn str_prop(title: &str, default: Option<&str>) -> Value {
    let mut pairs = Vec::new();
    if let Some(default) = default {
        pairs.push(("default", json::s(default)));
    }
    pairs.push(("title", json::s(title)));
    pairs.push(("type", json::s("string")));
    json::obj(pairs)
}

fn optional_str(title: &str) -> Value {
    json::obj(vec![
        (
            "anyOf",
            json::arr(vec![
                json::obj(vec![("type", json::s("string"))]),
                json::obj(vec![("type", json::s("null"))]),
            ]),
        ),
        ("default", Value::Null),
        ("title", json::s(title)),
    ])
}

fn bool_prop(title: &str) -> Value {
    json::obj(vec![
        ("default", json::bool_val(false)),
        ("title", json::s(title)),
        ("type", json::s("boolean")),
    ])
}

fn init_schema() -> Value {
    schema(
        &prog::tool("init"),
        json::obj(vec![("path", str_prop("Path", Some(".")))]),
        &[],
    )
}

fn ls_schema() -> Value {
    schema(
        &prog::tool("ls"),
        json::obj(vec![
            ("scope", str_prop("Scope", Some(""))),
            ("kind", optional_str("Kind")),
            ("include_superseded", bool_prop("Include Superseded")),
            ("tag", optional_str("Tag")),
        ]),
        &[],
    )
}

fn get_schema() -> Value {
    schema(
        &prog::tool("get"),
        json::obj(vec![(
            "ids",
            json::obj(vec![
                (
                    "items",
                    json::obj(vec![("type", json::s("string"))]),
                ),
                ("title", json::s("Ids")),
                ("type", json::s("array")),
            ]),
        )]),
        &["ids"],
    )
}

fn new_schema() -> Value {
    schema(
        &prog::tool("new"),
        json::obj(vec![
            ("id", str_prop("Id", None)),
            ("kind", str_prop("Kind", None)),
            ("summary", str_prop("Summary", None)),
            ("body", str_prop("Body", None)),
            ("author", optional_str("Author")),
            (
                "tags",
                json::obj(vec![
                    (
                        "anyOf",
                        json::arr(vec![
                            json::obj(vec![
                                (
                                    "items",
                                    json::obj(vec![("type", json::s("string"))]),
                                ),
                                ("type", json::s("array")),
                            ]),
                            json::obj(vec![("type", json::s("null"))]),
                        ]),
                    ),
                    ("default", Value::Null),
                    ("title", json::s("Tags")),
                ]),
            ),
        ]),
        &["id", "kind", "summary", "body"],
    )
}

fn supersede_schema() -> Value {
    schema(
        &prog::tool("supersede"),
        json::obj(vec![
            ("old", str_prop("Old", None)),
            ("new", str_prop("New", None)),
        ]),
        &["old", "new"],
    )
}

fn check_schema() -> Value {
    schema(&prog::tool("check"), json::obj(vec![]), &[])
}

const INIT: &str = "\
Create the default store and its _shared/report-format note.\n\
\n    \
Idempotent. path must be the current working directory, the directory in\n    \
SWARMNOTES_DIR, or the project directory whose .swarmnotes is\n    \
SWARMNOTES_DIR. Any other path is an error. Returns root (the store path)\n    \
and report_format (its note id).\n    ";

const LS: &str = "\
List notes as id, kind, summary, and status.\n\
\n    \
scope is a directory under the store; empty lists everything. kind filters\n    \
to one of decision, constraint, convention, context, format, finding,\n    \
attempt. Superseded notes are omitted unless include_superseded is true.\n    \
tag filters to one slug. The store is .swarmnotes/ or $SWARMNOTES_DIR.\n    ";

const GET: &str = "\
Fetch one or more notes by ID. Follows superseded_by, including multi-hop chains.\n\
\n    \
Returns notes (the live note, with chain) and errors (ids that failed).\n    \
Other ids are still returned when one id is missing.\n    ";

const NEW: &str = "\
Create a note. Fails if the id already exists; never overwrites.\n\
\n    \
kind is decision, constraint, convention, context, format, finding, or\n    \
attempt. summary is one line, at most 100 characters. body is the fact\n    \
(keep it under ~300 words). tags is a list of slugs.\n    ";

const CHECK: &str = "\
Lint every note in the store.\n\
\n    \
ok is false when there are errors. warnings (long bodies, missing author\n    \
or updated) do not fail the check. errors and warnings are the same\n    \
strings the CLI prints.\n    ";

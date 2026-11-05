//! Argument checks and the examen_init path rules.

use crate::error::NoteError;
use crate::grammar::DIR_NAME;
use crate::json::Value;
use crate::prog;
use crate::pystr::py_repr;
use crate::store::canonical_dir;
use std::path::{Path, PathBuf};

pub(crate) enum InvokeErr {
    Note(String),
    Validation { fields: Vec<String>, text: String },
}

fn object_pairs(args: &Value) -> &[(String, Value)] {
    match args {
        Value::Object(pairs) => pairs,
        _ => &[],
    }
}

pub(crate) fn lookup<'a>(args: &'a Value, key: &str) -> Option<&'a Value> {
    object_pairs(args)
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

pub(crate) fn path_is_null(args: &Value, key: &str) -> bool {
    matches!(lookup(args, key), Some(Value::Null))
}

pub(crate) fn string_or<'a>(args: &'a Value, key: &str, default: &'a str) -> &'a str {
    match lookup(args, key) {
        Some(Value::String(text)) => text,
        _ => default,
    }
}

pub(crate) fn optional_string(args: &Value, key: &str) -> Option<String> {
    match lookup(args, key) {
        Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
        Some(Value::String(_)) => Some(String::new()),
        _ => None,
    }
}

pub(crate) fn bool_or(args: &Value, key: &str, default: bool) -> bool {
    match lookup(args, key) {
        Some(Value::Bool(value)) => *value,
        _ => default,
    }
}

pub(crate) fn require_str<'a>(args: &'a Value, key: &str, defaulted: bool) -> Result<Option<&'a str>, InvokeErr> {
    match lookup(args, key) {
        Some(Value::String(text)) => Ok(Some(text)),
        None if defaulted => Ok(None),
        Some(value) => Err(type_error(&prog::tool("init"), key, Want::Str, value)),
        None => Err(type_error(&prog::tool("init"), key, Want::Str, &Value::Null)),
    }
}

pub(crate) fn require_present_str(tool: &str, args: &Value, key: &str) -> Result<String, InvokeErr> {
    match lookup(args, key) {
        Some(Value::String(text)) => Ok(text.clone()),
        None => Err(missing(tool, key, args)),
        Some(value) => Err(type_error(tool, key, Want::Str, value)),
    }
}

pub(crate) fn require_string_list(tool: &str, args: &Value, key: &str) -> Result<Vec<String>, InvokeErr> {
    match lookup(args, key) {
        None => Err(missing(tool, key, args)),
        Some(Value::Array(items)) => {
            let mut out = Vec::new();
            for item in items {
                match item {
                    Value::String(text) => out.push(text.clone()),
                    _ => {
                        let list = Value::Array(items.clone());
                        return Err(type_error(tool, key, Want::StrList, &list));
                    }
                }
            }
            Ok(out)
        }
        Some(value) => Err(type_error(tool, key, Want::StrList, value)),
    }
}

pub(crate) fn check_optional_str(tool: &str, args: &Value, key: &str) -> Result<(), InvokeErr> {
    match lookup(args, key) {
        None | Some(Value::Null) | Some(Value::String(_)) => Ok(()),
        Some(value) => Err(type_error(tool, key, Want::Str, value)),
    }
}

pub(crate) fn optional_tags(tool: &str, args: &Value) -> Result<Option<Vec<String>>, InvokeErr> {
    match lookup(args, "tags") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => {
            let mut out = Vec::new();
            for item in items {
                match item {
                    Value::String(text) => out.push(text.clone()),
                    _ => {
                        let list = Value::Array(items.clone());
                        return Err(type_error(tool, "tags", Want::StrList, &list));
                    }
                }
            }
            Ok(Some(out))
        }
        Some(value) => Err(type_error(tool, "tags", Want::StrList, value)),
    }
}

pub(crate) fn validate_ls(tool: &str, args: &Value) -> Result<(), InvokeErr> {
    check_optional_str(tool, args, "scope")?;
    check_optional_str(tool, args, "kind")?;
    check_optional_str(tool, args, "tag")?;
    check_optional_str(tool, args, "since")?;
    for key in ["include_superseded", "details"] {
        match lookup(args, key) {
            None | Some(Value::Bool(_)) => {}
            Some(value) => {
                return Err(type_error(tool, key, Want::Bool, value))
            }
        }
    }
    optional_int(tool, args, "limit").map(|_| ())
}

/// An optional integer argument. `None` when absent or null.
pub(crate) fn optional_int(tool: &str, args: &Value, key: &str) -> Result<Option<i64>, InvokeErr> {
    match lookup(args, key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(text)) => match text.parse::<i64>() {
            Ok(n) => Ok(Some(n)),
            Err(_) => Err(type_error(tool, key, Want::Int, &Value::Number(text.clone()))),
        },
        Some(value) => Err(type_error(tool, key, Want::Int, value)),
    }
}

/// What an argument should have been, for one-line validation errors.
#[derive(Clone, Copy)]
pub(crate) enum Want {
    Str,
    Bool,
    Int,
    StrList,
}

impl Want {
    fn phrase(self) -> &'static str {
        match self {
            Want::Str => "a string",
            Want::Bool => "true or false",
            Want::Int => "an integer",
            Want::StrList => "a list of strings",
        }
    }

    fn example(self) -> &'static str {
        match self {
            Want::Str => "\"text\"",
            Want::Bool => "true",
            Want::Int => "5",
            Want::StrList => "[\"net/retry\"]",
        }
    }
}

/// Every validation error starts with this. The parity harness relies on it.
pub(crate) const INVALID: &str = "invalid arguments: ";

/// Required arguments of each tool, in schema order.
fn required_of(tool: &str) -> &'static [&'static str] {
    if tool == prog::tool("new") {
        &["id", "kind", "summary", "body"]
    } else if tool == prog::tool("get") {
        &["ids"]
    } else if tool == prog::tool("supersede") {
        &["old", "new"]
    } else if tool == prog::tool("edit") {
        &["id", "author"]
    } else {
        &[]
    }
}

fn missing(tool: &str, key: &str, args: &Value) -> InvokeErr {
    // Report every missing required field of this tool, in schema order.
    let required = required_of(tool);
    let mut fields: Vec<String> = required
        .iter()
        .filter(|name| lookup(args, name).is_none())
        .map(|name| (*name).to_string())
        .collect();
    if fields.is_empty() {
        fields.push(key.to_string());
    }
    let word = if fields.len() == 1 { "argument" } else { "arguments" };
    let text = format!(
        "{INVALID}missing required {word} {}. {tool} needs {}.",
        fields.join(", "),
        and_list(required.iter().copied().chain((!required.contains(&key)).then_some(key)))
    );
    let mut sorted = fields;
    sorted.sort();
    InvokeErr::Validation {
        fields: sorted,
        text,
    }
}

fn and_list<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let items: Vec<&str> = items.collect();
    match items.len() {
        0 => String::new(),
        1 => items[0].to_string(),
        2 => format!("{} and {}", items[0], items[1]),
        n => format!("{}, and {}", items[..n - 1].join(", "), items[n - 1]),
    }
}

pub(crate) fn type_error(tool: &str, key: &str, want: Want, value: &Value) -> InvokeErr {
    InvokeErr::Validation {
        fields: vec![key.to_string()],
        text: format!(
            "{INVALID}{key} must be {}, got {}. Call {tool} again with {key} like {}.",
            want.phrase(),
            received(value),
            want.example()
        ),
    }
}

/// The value an agent sent, compact, when it is short; otherwise its kind.
fn received(value: &Value) -> String {
    let kind = match value {
        Value::Null => return "null".to_string(),
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "list",
        Value::Object(_) => "object",
    };
    let text = crate::json::write_compact(value);
    if text.chars().count() <= 40 {
        format!("{text} ({kind})")
    } else {
        format!("a long {kind}")
    }
}

pub(crate) fn store_for_init(path: &str) -> Result<PathBuf, NoteError> {
    if path.trim().is_empty() {
        return Err(NoteError::new(refusal(path)));
    }
    let requested = resolve_user(Path::new(path)).map_err(|err| {
        NoteError::new(format!("{} ({err})", refusal(path)))
    })?;
    let cwd = resolve_user(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .unwrap_or_else(|_| PathBuf::from("."));
    let env = std::env::var("SWARMNOTES_DIR").unwrap_or_default();
    let env = env.trim();
    let store = if env.is_empty() {
        None
    } else {
        Some(resolve_user(Path::new(env)).map_err(|err| {
            NoteError::new(format!("SWARMNOTES_DIR={env} is not usable: {err}"))
        })?)
    };
    if let Some(store) = &store {
        if paths_match(&requested, store) {
            return Ok(store.clone());
        }
    }
    if paths_match(&requested, &cwd) {
        return Ok(cwd.join(DIR_NAME));
    }
    if let Some(store) = &store {
        if paths_match(&requested.join(DIR_NAME), store) {
            return Ok(store.clone());
        }
    }
    Err(NoteError::new(refusal(path)))
}

fn refusal(path: &str) -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let cwd = resolve_user(&cwd).unwrap_or(cwd);
    let env = std::env::var("SWARMNOTES_DIR").unwrap_or_default();
    let env = env.trim();
    let allowed = if env.is_empty() {
        format!(
            "the current working directory ({}). Set SWARMNOTES_DIR to use another store",
            cwd.display()
        )
    } else {
        format!(
            "the current working directory ({}) or SWARMNOTES_DIR ({env})",
            cwd.display()
        )
    };
    format!(
        "{} only creates a store in {allowed}. Refusing {}.",
        prog::tool("init"),
        py_repr(path)
    )
}

fn paths_match(left: &Path, right: &Path) -> bool {
    let Ok(left) = resolve_user(left) else {
        return false;
    };
    let Ok(right) = resolve_user(right) else {
        return false;
    };
    #[cfg(windows)]
    {
        left.to_string_lossy().to_ascii_lowercase() == right.to_string_lossy().to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn resolve_user(path: &Path) -> Result<PathBuf, String> {
    let text = path.to_string_lossy();
    let expanded = if text == "~" {
        home()?
    } else if let Some(rest) = text.strip_prefix("~/") {
        home()?.join(rest)
    } else if let Some(rest) = text.strip_prefix("~\\") {
        home()?.join(rest)
    } else {
        path.to_path_buf()
    };
    let abs = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .map_err(|err| err.to_string())?
            .join(expanded)
    };
    if abs.exists() {
        return Ok(canonical_dir(abs));
    }
    let mut acc = PathBuf::new();
    let mut pending = Vec::new();
    for comp in abs.components() {
        if pending.is_empty() {
            let next = acc.join(comp);
            if next.exists() {
                acc = canonical_dir(next);
                continue;
            }
        }
        pending.push(comp);
    }
    for comp in pending {
        acc.push(comp);
    }
    Ok(normalize(acc))
}

fn normalize(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| "no home directory".to_string())
}

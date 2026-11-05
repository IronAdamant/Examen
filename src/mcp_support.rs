//! Argument checks and the examen_init path rules.

use crate::error::NoteError;
use crate::grammar::DIR_NAME;
use crate::json::Value;
use crate::prog;
use crate::pystr::{py_json_repr, py_repr, py_type_name};
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
        other => Err(InvokeErr::Note(format!("bad {key}: {}", other.map(|v| py_json_repr(v)).unwrap_or_else(|| "missing".into())))),
    }
}

pub(crate) fn require_present_str(tool: &str, args: &Value, key: &str) -> Result<String, InvokeErr> {
    match lookup(args, key) {
        Some(Value::String(text)) => Ok(text.clone()),
        None => Err(missing(tool, key, args)),
        Some(value) => Err(type_error(
            tool,
            key,
            "string_type",
            "Input should be a valid string",
            value,
        )),
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
                    other => {
                        return Err(type_error(
                            tool,
                            key,
                            "string_type",
                            "Input should be a valid string",
                            other,
                        ));
                    }
                }
            }
            Ok(out)
        }
        Some(value) => Err(type_error(
            tool,
            key,
            "list_type",
            "Input should be a valid list",
            value,
        )),
    }
}

pub(crate) fn check_optional_str(tool: &str, args: &Value, key: &str) -> Result<(), InvokeErr> {
    match lookup(args, key) {
        None | Some(Value::Null) | Some(Value::String(_)) => Ok(()),
        Some(value) => Err(type_error(
            tool,
            key,
            "string_type",
            "Input should be a valid string",
            value,
        )),
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
                    other => {
                        return Err(type_error(
                            tool,
                            "tags",
                            "string_type",
                            "Input should be a valid string",
                            other,
                        ));
                    }
                }
            }
            Ok(Some(out))
        }
        Some(value) => Err(type_error(
            tool,
            "tags",
            "list_type",
            "Input should be a valid list",
            value,
        )),
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
                return Err(type_error(
                    tool,
                    key,
                    "bool_type",
                    "Input should be a valid boolean",
                    value,
                ))
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
            Err(_) => Err(type_error(
                tool,
                key,
                "int_type",
                "Input should be a valid integer",
                &Value::Number(text.clone()),
            )),
        },
        Some(value) => Err(type_error(
            tool,
            key,
            "int_type",
            "Input should be a valid integer",
            value,
        )),
    }
}

fn missing(tool: &str, key: &str, args: &Value) -> InvokeErr {
    // Report every missing required field of this tool, in schema order.
    let required: &[&str] = if tool == prog::tool("new") {
        &["id", "kind", "summary", "body"]
    } else if tool == prog::tool("get") {
        &["ids"]
    } else if tool == prog::tool("supersede") {
        &["old", "new"]
    } else if tool == prog::tool("init") {
        &[]
    } else {
        &[]
    };
    let mut fields: Vec<String> = required
        .iter()
        .filter(|name| lookup(args, name).is_none())
        .map(|name| (*name).to_string())
        .collect();
    if fields.is_empty() {
        fields.push(key.to_string());
    }
    let text = validation_text(tool, &fields, args, &[]);
    let mut sorted = fields.clone();
    sorted.sort();
    InvokeErr::Validation {
        fields: sorted,
        text,
    }
}

pub(crate) fn type_error(tool: &str, key: &str, type_name: &str, sentence: &str, value: &Value) -> InvokeErr {
    let detail = format!(
        "{key}\n  {sentence} [type={type_name}, input_value={}, input_type={}]\n    For further information visit https://errors.pydantic.dev/2.14/v/{type_name}",
        py_json_repr(value),
        py_type_name(value)
    );
    InvokeErr::Validation {
        fields: vec![key.to_string()],
        text: format!("1 validation error for {tool}Arguments\n{detail}"),
    }
}

fn validation_text(tool: &str, missing: &[String], args: &Value, _extra: &[String]) -> String {
    let n = missing.len();
    let word = if n == 1 { "error" } else { "errors" };
    let mut out = format!("{n} validation {word} for {tool}Arguments\n");
    for (index, field) in missing.iter().enumerate() {
        out.push_str(field);
        out.push_str(&format!(
            "\n  Field required [type=missing, input_value={}, input_type={}]\n    For further information visit https://errors.pydantic.dev/2.14/v/missing",
            py_json_repr(args),
            py_type_name(args)
        ));
        if index + 1 != missing.len() {
            out.push('\n');
        }
    }
    out
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

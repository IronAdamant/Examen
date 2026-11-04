//! Python `repr` for the strings Examen puts in error messages.

pub fn py_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::new();
    out.push(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(quote);
            }
            c if (c as u32) < 0x20 || c == '\u{007f}' => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

pub fn py_list(items: &[String]) -> String {
    let body = items
        .iter()
        .map(|item| py_repr(item))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{body}]")
}

/// Python `repr` of a JSON object, used inside pydantic's `input_value=`.
pub fn py_json_repr(value: &crate::json::Value) -> String {
    use crate::json::Value;
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(n) => n.clone(),
        Value::String(s) => py_repr(s),
        Value::Array(items) => {
            let body = items
                .iter()
                .map(py_json_repr)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{body}]")
        }
        Value::Object(pairs) => {
            let body = pairs
                .iter()
                .map(|(k, v)| format!("{}: {}", py_repr(k), py_json_repr(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{body}}}")
        }
    }
}

pub fn py_type_name(value: &crate::json::Value) -> &'static str {
    use crate::json::Value;
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.contains('.') || n.contains('e') || n.contains('E') {
                "float"
            } else {
                "int"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

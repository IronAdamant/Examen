//! A small JSON type that keeps object key order.
//! Compact output matches pydantic's `model_dump_json`.
//! Pretty output (indent 2) matches `pydantic_core.to_json(..., indent=2)`.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn object(pairs: Vec<(String, Value)>) -> Self {
        Value::Object(pairs)
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct JsonError {
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

pub fn parse(input: &str) -> Result<Value, JsonError> {
    let mut p = Parser {
        s: input.as_bytes(),
        i: 0,
    };
    let value = p.parse_value()?;
    p.skip();
    if p.i != p.s.len() {
        return Err(p.err("trailing data"));
    }
    Ok(value)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn err(&self, message: &str) -> JsonError {
        JsonError {
            message: message.to_string(),
        }
    }

    fn skip(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn parse_value(&mut self) -> Result<Value, JsonError> {
        self.skip();
        if self.i >= self.s.len() {
            return Err(self.err("unexpected end"));
        }
        match self.s[self.i] {
            b'n' => self.consume_lit(b"null", Value::Null),
            b't' => self.consume_lit(b"true", Value::Bool(true)),
            b'f' => self.consume_lit(b"false", Value::Bool(false)),
            b'"' => Ok(Value::String(self.parse_string()?)),
            b'[' => self.parse_array(),
            b'{' => self.parse_object(),
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => Err(self.err("invalid value")),
        }
    }

    fn consume_lit(&mut self, lit: &[u8], value: Value) -> Result<Value, JsonError> {
        if self.s[self.i..].starts_with(lit) {
            self.i += lit.len();
            Ok(value)
        } else {
            Err(self.err("invalid literal"))
        }
    }

    fn parse_number(&mut self) -> Result<Value, JsonError> {
        let start = self.i;
        if self.s[self.i] == b'-' {
            self.i += 1;
        }
        if self.i >= self.s.len() || !self.s[self.i].is_ascii_digit() {
            return Err(self.err("invalid number"));
        }
        if self.s[self.i] == b'0' {
            self.i += 1;
        } else {
            while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                self.i += 1;
            }
        }
        if self.i < self.s.len() && self.s[self.i] == b'.' {
            self.i += 1;
            if self.i >= self.s.len() || !self.s[self.i].is_ascii_digit() {
                return Err(self.err("invalid number"));
            }
            while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                self.i += 1;
            }
        }
        if self.i < self.s.len() && matches!(self.s[self.i], b'e' | b'E') {
            self.i += 1;
            if self.i < self.s.len() && matches!(self.s[self.i], b'+' | b'-') {
                self.i += 1;
            }
            if self.i >= self.s.len() || !self.s[self.i].is_ascii_digit() {
                return Err(self.err("invalid number"));
            }
            while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                self.i += 1;
            }
        }
        let raw = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("0");
        Ok(Value::Number(raw.to_string()))
    }

    fn parse_string(&mut self) -> Result<String, JsonError> {
        self.i += 1;
        let mut out = String::new();
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c == b'"' {
                self.i += 1;
                return Ok(out);
            }
            if c == b'\\' {
                self.i += 1;
                if self.i >= self.s.len() {
                    return Err(self.err("bad escape"));
                }
                match self.s[self.i] {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let ch = self.parse_u_escape()?;
                        out.push(ch);
                        continue;
                    }
                    _ => return Err(self.err("bad escape")),
                }
                self.i += 1;
                continue;
            }
            if c < 0x20 {
                return Err(self.err("raw control in string"));
            }
            let rest = &self.s[self.i..];
            let ch = utf8_char(rest).ok_or_else(|| self.err("bad utf-8"))?;
            out.push(ch.0);
            self.i += ch.1;
        }
        Err(self.err("unterminated string"))
    }

    fn parse_u_escape(&mut self) -> Result<char, JsonError> {
        self.i += 1;
        let unit = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&unit) {
            if self.s[self.i..].starts_with(br"\u") {
                self.i += 2;
                let low = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(self.err("bad surrogate"));
                }
                let u = 0x10000 + (((unit as u32) - 0xD800) << 10) + ((low as u32) - 0xDC00);
                return char::from_u32(u).ok_or_else(|| self.err("bad surrogate"));
            }
            return Err(self.err("lonely surrogate"));
        }
        char::from_u32(unit as u32).ok_or_else(|| self.err("bad unicode"))
    }

    fn hex4(&mut self) -> Result<u16, JsonError> {
        if self.i + 4 > self.s.len() {
            return Err(self.err("short unicode escape"));
        }
        let text = std::str::from_utf8(&self.s[self.i..self.i + 4]).unwrap_or("");
        let n = u16::from_str_radix(text, 16).map_err(|_| self.err("bad unicode"))?;
        self.i += 4;
        Ok(n)
    }

    fn parse_array(&mut self) -> Result<Value, JsonError> {
        self.i += 1;
        let mut items = Vec::new();
        self.skip();
        if self.i < self.s.len() && self.s[self.i] == b']' {
            self.i += 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.parse_value()?);
            self.skip();
            if self.i >= self.s.len() {
                return Err(self.err("unterminated array"));
            }
            match self.s[self.i] {
                b']' => {
                    self.i += 1;
                    break;
                }
                b',' => {
                    self.i += 1;
                }
                _ => return Err(self.err("expected comma")),
            }
        }
        Ok(Value::Array(items))
    }

    fn parse_object(&mut self) -> Result<Value, JsonError> {
        self.i += 1;
        let mut pairs = Vec::new();
        self.skip();
        if self.i < self.s.len() && self.s[self.i] == b'}' {
            self.i += 1;
            return Ok(Value::Object(pairs));
        }
        loop {
            self.skip();
            if self.i >= self.s.len() || self.s[self.i] != b'"' {
                return Err(self.err("expected key"));
            }
            let key = self.parse_string()?;
            self.skip();
            if self.i >= self.s.len() || self.s[self.i] != b':' {
                return Err(self.err("expected colon"));
            }
            self.i += 1;
            let value = self.parse_value()?;
            pairs.push((key, value));
            self.skip();
            if self.i >= self.s.len() {
                return Err(self.err("unterminated object"));
            }
            match self.s[self.i] {
                b'}' => {
                    self.i += 1;
                    break;
                }
                b',' => self.i += 1,
                _ => return Err(self.err("expected comma")),
            }
        }
        Ok(Value::Object(pairs))
    }
}

fn utf8_char(bytes: &[u8]) -> Option<(char, usize)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let ch = text.chars().next()?;
    Some((ch, ch.len_utf8()))
}

pub fn write_compact(value: &Value) -> String {
    let mut out = String::new();
    write_compact_into(&mut out, value);
    out
}

pub fn write_pretty(value: &Value) -> String {
    let mut out = String::new();
    write_pretty_into(&mut out, value, 0);
    out
}

fn write_compact_into(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(n),
        Value::String(s) => push_string(out, s),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_compact_into(out, item);
            }
            out.push(']');
        }
        Value::Object(pairs) => {
            out.push('{');
            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_string(out, k);
                out.push(':');
                write_compact_into(out, v);
            }
            out.push('}');
        }
    }
}

fn write_pretty_into(out: &mut String, value: &Value, indent: usize) {
    match value {
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Object(pairs) if pairs.is_empty() => out.push_str("{}"),
        Value::Array(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                push_indent(out, indent + 2);
                write_pretty_into(out, item, indent + 2);
                if i + 1 != items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(out, indent);
            out.push(']');
        }
        Value::Object(pairs) => {
            out.push_str("{\n");
            for (i, (k, v)) in pairs.iter().enumerate() {
                push_indent(out, indent + 2);
                push_string(out, k);
                out.push_str(": ");
                write_pretty_into(out, v, indent + 2);
                if i + 1 != pairs.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(out, indent);
            out.push('}');
        }
        other => write_compact_into(out, other),
    }
}

fn push_indent(out: &mut String, n: usize) {
    out.extend(std::iter::repeat(' ').take(n));
}

fn push_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn s(text: &str) -> Value {
    Value::String(text.to_string())
}

pub fn bool_val(v: bool) -> Value {
    Value::Bool(v)
}

pub fn arr(items: Vec<Value>) -> Value {
    Value::Array(items)
}

pub fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Object(
        pairs
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

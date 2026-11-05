//! Note frontmatter: the flat `key: value` subset Examen writes and lints.

use crate::error::{NoteError, NoteResult};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaVal {
    Str(String),
    List(Vec<String>),
}

impl MetaVal {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            MetaVal::Str(s) => Some(s),
            MetaVal::List(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub fields: Vec<(String, MetaVal)>,
    pub body: String,
}

impl Note {
    pub fn get(&self, key: &str) -> Option<&MetaVal> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(MetaVal::as_str)
    }

    pub fn set(&mut self, key: &str, value: MetaVal) {
        if let Some((_, slot)) = self.fields.iter_mut().find(|(k, _)| k == key) {
            *slot = value;
        } else {
            self.fields.push((key.to_string(), value));
        }
    }
}

pub fn read_note_text(path: &Path) -> NoteResult<String> {
    let data = fs::read(path).map_err(|err| NoteError::new(err.to_string()))?;
    let data = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&data);
    let text = std::str::from_utf8(data).map_err(|_| NoteError::new("not valid utf-8"))?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

pub enum Block {
    Missing,
    Unclosed,
    Closed(String),
}

pub fn split_frontmatter(text: &str) -> Block {
    if !text.starts_with("---\n") {
        return Block::Missing;
    }
    match text[4..].find("\n---\n") {
        Some(rel) => Block::Closed(text[4..4 + rel].to_string()),
        None => Block::Unclosed,
    }
}

pub fn parse(text: &str) -> Note {
    let block = split_frontmatter(text);
    let Block::Closed(block) = block else {
        return Note {
            fields: Vec::new(),
            body: text.to_string(),
        };
    };
    let mut fields = Vec::new();
    for line in block.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = raw_value.trim();
        let meta_val = if value.starts_with('[') && value.ends_with(']') {
            let inner = &value[1..value.len() - 1];
            let items = inner
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect();
            MetaVal::List(items)
        } else {
            MetaVal::Str(value.to_string())
        };
        if let Some((_, slot)) = fields.iter_mut().find(|(k, _)| k == key) {
            *slot = meta_val;
        } else {
            fields.push((key.to_string(), meta_val));
        }
    }
    let end = text.find("\n---\n").unwrap_or(0);
    let body = text[end + 5..].trim_start_matches('\n').to_string();
    Note { fields, body }
}

pub fn frontmatter_issues(text: &str) -> Vec<String> {
    let block = match split_frontmatter(text) {
        Block::Missing => return vec!["missing frontmatter".to_string()],
        Block::Unclosed => return vec!["unclosed frontmatter".to_string()],
        Block::Closed(block) => block,
    };
    let mut issues = Vec::new();
    let mut seen = Vec::new();
    for line in block.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let Some((key, _)) = line.split_once(':') else {
            issues.push(format!(
                "frontmatter line is not key: value ({})",
                line.trim()
            ));
            continue;
        };
        let key = key.trim();
        if seen.iter().any(|seen_key: &String| seen_key == key) {
            issues.push(format!("duplicate frontmatter key: {key}"));
        }
        seen.push(key.to_string());
        if !crate::grammar::KNOWN_KEYS.contains(&key) {
            issues.push(format!("unknown frontmatter key: {key}"));
        }
    }
    issues
}

pub fn render(note: &Note) -> String {
    let mut lines = vec!["---".to_string()];
    for (key, value) in &note.fields {
        let rendered = match value {
            MetaVal::List(items) => format!("[{}]", items.join(", ")),
            MetaVal::Str(text) => text.clone(),
        };
        lines.push(format!("{key}: {rendered}"));
    }
    lines.push("---".to_string());
    let body = note.body.trim_end_matches('\n');
    format!("{}\n\n{body}\n", lines.join("\n"))
}

/// Replace a note so a concurrent reader sees the old bytes or the new bytes,
/// never a truncated file: write a temp file beside it, then rename over it.
pub fn write_note_file(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = temp_beside(path);
    fs::write(&tmp, file_bytes(text))?;
    let result = rename_over(&tmp, path);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Create a note only if `path` does not exist, publishing the whole file at
/// once. A hard link from a complete temp file is an atomic exclusive create.
/// Where hard links are unsupported, fall back to create_new and write.
pub fn create_note_file(path: &Path, text: &str) -> std::io::Result<()> {
    let bytes = file_bytes(text);
    let tmp = temp_beside(path);
    fs::write(&tmp, &bytes)?;
    let linked = fs::hard_link(&tmp, path);
    let _ = fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Err(err),
        Err(_) => {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            file.write_all(&bytes)
        }
    }
}

/// Temp name in the note's directory. It does not end in `.md`, so a store
/// walk never lists it, even if a crash leaves it behind.
fn temp_beside(path: &Path) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.subsec_nanos())
        .unwrap_or(0);
    path.with_file_name(format!(
        ".{name}.{}-{}-{nanos}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Before Rust 1.84, std renames on Windows with MoveFileExW, which refuses
/// to replace a file that any process has open, even a reader that allows
/// delete sharing. Readers hold a note for microseconds, so retry in 1 ms
/// steps for up to about two seconds before giving up.
fn rename_over(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut tries = 0;
    loop {
        match fs::rename(from, to) {
            Err(err)
                if cfg!(windows)
                    && err.kind() == std::io::ErrorKind::PermissionDenied
                    && tries < 2000 =>
            {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            other => return other,
        }
    }
}

pub fn file_bytes(text: &str) -> Vec<u8> {
    #[cfg(windows)]
    {
        text.replace('\n', "\r\n").into_bytes()
    }
    #[cfg(not(windows))]
    {
        text.as_bytes().to_vec()
    }
}

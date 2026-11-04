//! Id, tag, and date grammars from SPEC.md, checked without the regex crate.

pub const SEGMENT: &str = "_?[a-z0-9][a-z0-9-]*";
pub const KINDS: &[&str] = &[
    "decision",
    "constraint",
    "convention",
    "context",
    "format",
    "finding",
    "attempt",
];
pub const STATUSES: &[&str] = &["active", "draft", "superseded"];
pub const KNOWN_KEYS: &[&str] = &[
    "kind",
    "summary",
    "author",
    "updated",
    "tags",
    "status",
    "superseded_by",
];
pub const SUMMARY_MAX: usize = 100;
pub const WORD_BUDGET: usize = 300;
pub const MAX_SUPERSEDE_HOPS: usize = 32;
pub const DIR_NAME: &str = ".swarmnotes";

fn alnum(c: u8) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

pub fn valid_segment(seg: &str) -> bool {
    let b = seg.as_bytes();
    if b.is_empty() {
        return false;
    }
    let mut i = 0;
    if b[0] == b'_' {
        i = 1;
    }
    if i >= b.len() || !alnum(b[i]) {
        return false;
    }
    i += 1;
    while i < b.len() {
        if !(alnum(b[i]) || b[i] == b'-') {
            return false;
        }
        i += 1;
    }
    true
}

pub fn valid_id(id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    id.split('/').all(valid_segment)
}

pub fn valid_tag(tag: &str) -> bool {
    let b = tag.as_bytes();
    if b.is_empty() || !alnum(b[0]) {
        return false;
    }
    b[1..].iter().all(|c| alnum(*c) || *c == b'-')
}

/// Python `str.isspace`, used by `str.split()`.
pub fn is_py_space(c: char) -> bool {
    let u = c as u32;
    (0x9..=0xd).contains(&u)
        || (0x1c..=0x20).contains(&u)
        || u == 0x85
        || u == 0xa0
        || u == 0x1680
        || (0x2000..=0x200a).contains(&u)
        || (0x2028..=0x2029).contains(&u)
        || u == 0x202f
        || u == 0x205f
        || u == 0x3000
}

/// Python `text.split()` with no arguments.
pub fn py_words(text: &str) -> usize {
    let mut n = 0;
    let mut in_word = false;
    for c in text.chars() {
        if is_py_space(c) {
            in_word = false;
        } else if !in_word {
            in_word = true;
            n += 1;
        }
    }
    n
}

pub fn joined(items: &[&str]) -> String {
    items.join(", ")
}

//! Note commands shared by the CLI and the MCP tools.

use crate::clock::{self, valid_date};
use crate::error::{NoteError, NoteResult};
use crate::grammar::{self, KINDS, STATUSES, SUMMARY_MAX, WORD_BUDGET};
use crate::meta::{self, MetaVal, Note};
use crate::pystr::py_repr;
use crate::store;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub kind: String,
    pub summary: String,
    pub status: String,
    pub superseded_by: Option<String>,
    /// Filled only when `LsQuery::details` is set.
    pub details: Option<Details>,
}

/// Who wrote a note and when, for `ls` with details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub author: Option<String>,
    pub updated: Option<String>,
    /// File modification time, `YYYY-MM-DDTHH:MM:SSZ`.
    pub modified: String,
}

/// `ls` arguments beyond scope/kind/tag/all. The defaults list exactly
/// what `list_notes` lists.
#[derive(Debug, Clone, Default)]
pub struct LsQuery {
    pub scope: String,
    pub kind: Option<String>,
    pub tag: Option<String>,
    pub include_superseded: bool,
    /// Only notes whose file changed at or after this many seconds since
    /// the Unix epoch (UTC).
    pub since: Option<i64>,
    /// At most this many rows; the rest are counted in `Listing::more`.
    pub limit: Option<usize>,
    pub details: bool,
}

pub struct Listing {
    pub rows: Vec<Row>,
    /// Matching notes left out by `limit`.
    pub more: usize,
}

/// `--since` / `since`: a UTC date or date-time.
pub fn parse_since(text: &str) -> NoteResult<i64> {
    clock::parse_utc(text).ok_or_else(|| {
        NoteError::new(format!(
            "since must be YYYY-MM-DD or YYYY-MM-DDTHH:MM:SSZ (UTC), not {}",
            py_repr(text)
        ))
    })
}

pub fn parse_tags(tags: Option<TagsIn>) -> NoteResult<Vec<String>> {
    let Some(tags) = tags else {
        return Ok(Vec::new());
    };
    let items = match tags {
        TagsIn::Csv(text) => text
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect(),
        TagsIn::List(parts) => {
            let mut items = Vec::new();
            for part in parts {
                if part.trim().is_empty() {
                    return Err(NoteError::new("invalid tags"));
                }
                items.push(part.trim().to_string());
            }
            items
        }
    };
    let bad: Vec<_> = items
        .iter()
        .filter(|item| !grammar::valid_tag(item))
        .cloned()
        .collect();
    if !bad.is_empty() {
        return Err(NoteError::new(format!("invalid tags: {}", bad.join(", "))));
    }
    Ok(items)
}

pub enum TagsIn {
    Csv(String),
    List(Vec<String>),
}

pub fn add_note(
    root: &Path,
    note_id: &str,
    kind: &str,
    summary: &str,
    body: &str,
    author: Option<&str>,
    tags: Option<TagsIn>,
) -> NoteResult<String> {
    if !KINDS.contains(&kind) {
        return Err(NoteError::new(format!(
            "kind must be one of: {}",
            grammar::joined(KINDS)
        )));
    }
    let summary = summary.trim();
    if summary.is_empty() {
        return Err(NoteError::new("missing summary"));
    }
    if summary.chars().count() > SUMMARY_MAX {
        return Err(NoteError::new(format!(
            "summary is {} chars; max {SUMMARY_MAX}",
            summary.chars().count()
        )));
    }
    if body.trim().is_empty() {
        return Err(NoteError::new(
            "empty body (pass --body or pipe text on stdin)",
        ));
    }
    let mut fields = vec![
        ("kind".into(), MetaVal::Str(kind.to_string())),
        ("summary".into(), MetaVal::Str(summary.to_string())),
    ];
    if let Some(author) = author {
        if author.trim().is_empty() {
            return Err(NoteError::new("empty author"));
        }
        fields.push(("author".into(), MetaVal::Str(author.trim().to_string())));
    }
    fields.push(("updated".into(), MetaVal::Str(clock::today())));
    let parsed = parse_tags(tags)?;
    if !parsed.is_empty() {
        fields.push(("tags".into(), MetaVal::List(parsed)));
    }
    let note = Note {
        fields,
        body: body.to_string(),
    };
    store::create(root, note_id, &note)?;
    Ok(note_id.to_string())
}

pub fn list_notes(
    root: &Path,
    scope: &str,
    kind: Option<&str>,
    tag: Option<&str>,
    include_superseded: bool,
) -> NoteResult<Vec<Row>> {
    let query = LsQuery {
        scope: scope.to_string(),
        kind: kind.map(str::to_string),
        tag: tag.map(str::to_string),
        include_superseded,
        ..LsQuery::default()
    };
    Ok(list_notes_with(root, &query)?.rows)
}

pub fn list_notes_with(root: &Path, query: &LsQuery) -> NoteResult<Listing> {
    let kind = query.kind.as_deref();
    let tag = query.tag.as_deref();
    if let Some(kind) = kind {
        if !KINDS.contains(&kind) {
            return Err(NoteError::new(format!(
                "kind must be one of: {}",
                grammar::joined(KINDS)
            )));
        }
    }
    if let Some(tag) = tag {
        if !grammar::valid_tag(tag) {
            return Err(NoteError::new(format!("invalid tag: {tag}")));
        }
    }
    if query.limit == Some(0) {
        return Err(NoteError::new("limit must be at least 1"));
    }
    let timed = query.since.is_some() || query.details;
    let mut found = store::walk_notes(root, query.scope.trim_matches('/'), timed)?;
    if let Some(since) = query.since {
        // Filter on the walk's timestamps, before any note is opened.
        found.retain(|note| note.modified.map(clock::epoch_secs).unwrap_or(0) >= since);
    }
    let paths: Vec<&Path> = found.iter().map(|note| note.path.as_path()).collect();
    let texts = meta::read_note_texts(&paths);
    let mut rows = Vec::new();
    for (entry, text) in found.iter().zip(texts) {
        let note = meta::parse(&text?);
        let status = note.get_str("status").unwrap_or("active").to_string();
        if status == "superseded" && !query.include_superseded {
            continue;
        }
        if let Some(kind) = kind {
            if note.get_str("kind") != Some(kind) {
                continue;
            }
        }
        if let Some(tag) = tag {
            match note.get("tags") {
                Some(MetaVal::List(tags)) if tags.iter().any(|item| item == tag) => {}
                _ => continue,
            }
        }
        let superseded_by = if status == "superseded" {
            Some(
                note.get_str("superseded_by")
                    .unwrap_or("?")
                    .to_string(),
            )
        } else {
            None
        };
        let details = query.details.then(|| Details {
            author: note.get_str("author").filter(|a| !a.is_empty()).map(str::to_string),
            updated: note.get_str("updated").filter(|u| !u.is_empty()).map(str::to_string),
            modified: clock::format_utc(entry.modified.map(clock::epoch_secs).unwrap_or(0)),
        });
        rows.push(Row {
            id: entry.id.clone(),
            kind: note.get_str("kind").unwrap_or("?").to_string(),
            summary: note.get_str("summary").unwrap_or("").to_string(),
            status,
            superseded_by,
            details,
        });
    }
    let mut more = 0;
    if let Some(limit) = query.limit {
        more = rows.len().saturating_sub(limit);
        rows.truncate(limit);
    }
    Ok(Listing { rows, more })
}

pub struct Resolved {
    pub final_id: String,
    pub note: Note,
    pub chain: Vec<String>,
}

pub fn resolve(root: &Path, note_id: &str) -> NoteResult<Resolved> {
    let mut chain = vec![note_id.to_string()];
    let mut note = store::load(root, note_id)?;
    while note.get_str("status") == Some("superseded") {
        let nxt = match note.get("superseded_by") {
            Some(MetaVal::Str(text)) if !text.is_empty() => text.clone(),
            Some(MetaVal::List(items)) if !items.is_empty() => {
                return Err(NoteError::new(format!(
                    "invalid superseded_by: {}",
                    crate::pystr::py_list(items)
                )));
            }
            _ => {
                if chain.len() == 1 {
                    return Err(NoteError::new("superseded but no superseded_by"));
                }
                return Err(NoteError::new(format!(
                    "supersede chain ends at {} with no superseded_by",
                    chain.last().map(String::as_str).unwrap_or(note_id)
                )));
            }
        };
        if !grammar::valid_id(&nxt) {
            return Err(NoteError::new(format!(
                "invalid superseded_by: {}",
                py_repr(&nxt)
            )));
        }
        if chain.iter().any(|seen| seen == &nxt) {
            let mut shown = chain.clone();
            shown.push(nxt);
            return Err(NoteError::new(format!(
                "supersede cycle: {}",
                shown.join(" -> ")
            )));
        }
        if chain.len() >= grammar::MAX_SUPERSEDE_HOPS {
            return Err(NoteError::new(format!(
                "supersede chain longer than {}",
                grammar::MAX_SUPERSEDE_HOPS
            )));
        }
        chain.push(nxt.clone());
        note = store::load(root, &nxt)?;
    }
    if note.get("superseded_by").is_some_and(meta_truthy) {
        return Err(NoteError::new(
            "superseded_by set but status is not superseded",
        ));
    }
    let final_id = chain.last().cloned().unwrap_or_else(|| note_id.to_string());
    Ok(Resolved {
        final_id,
        note,
        chain,
    })
}

fn meta_truthy(value: &MetaVal) -> bool {
    match value {
        MetaVal::Str(text) => !text.is_empty(),
        MetaVal::List(items) => !items.is_empty(),
    }
}

/// Changes `edit_note` may make. `None` leaves a field as it is.
#[derive(Debug, Default)]
pub struct NoteEdit {
    pub summary: Option<String>,
    pub body: Option<String>,
    /// An empty list removes the tags.
    pub tags: Option<Vec<String>>,
}

/// A small correction in place (SPEC section 5): new summary, body, or tags
/// and today's `updated`. Only the note's author may edit it, and only
/// while it is live. Replacing a note is `supersede`. Returns the date
/// written to `updated`.
pub fn edit_note(root: &Path, note_id: &str, author: &str, change: NoteEdit) -> NoteResult<String> {
    let author = author.trim();
    if author.is_empty() {
        return Err(NoteError::new("empty author"));
    }
    if change.summary.is_none() && change.body.is_none() && change.tags.is_none() {
        return Err(NoteError::new("nothing to change: pass summary, body, or tags"));
    }
    let path = store::note_path(root, note_id)?;
    let mut note = store::load(root, note_id)?;
    if note.get_str("status") == Some("superseded") {
        let next = note.get_str("superseded_by").unwrap_or("?");
        return Err(NoteError::new(format!(
            "{note_id} is superseded by {next}; edit the live note {next} instead"
        )));
    }
    match note.get_str("author").map(str::trim) {
        Some(owner) if !owner.is_empty() && owner != author => {
            return Err(NoteError::new(format!(
                "{note_id} was written by {owner}, not {author}; only its author edits it in place"
            )));
        }
        Some(owner) if !owner.is_empty() => {}
        _ => note.set("author", MetaVal::Str(author.to_string())),
    }
    if let Some(summary) = change.summary {
        let summary = summary.trim();
        if summary.is_empty() {
            return Err(NoteError::new("missing summary"));
        }
        if summary.chars().count() > SUMMARY_MAX {
            return Err(NoteError::new(format!(
                "summary is {} chars; max {SUMMARY_MAX}",
                summary.chars().count()
            )));
        }
        note.set("summary", MetaVal::Str(summary.to_string()));
    }
    if let Some(body) = change.body {
        if body.trim().is_empty() {
            return Err(NoteError::new("empty body"));
        }
        note.body = body;
    }
    if let Some(tags) = change.tags {
        let tags = parse_tags(Some(TagsIn::List(tags)))?;
        if tags.is_empty() {
            note.fields.retain(|(key, _)| key != "tags");
        } else {
            note.set("tags", MetaVal::List(tags));
        }
    }
    let today = clock::today();
    note.set("updated", MetaVal::Str(today.clone()));
    meta::write_note_file(&path, &meta::render(&note)).map_err(|err| NoteError::new(err.to_string()))?;
    Ok(today)
}

pub fn supersede_note(root: &Path, old: &str, new: &str) -> NoteResult<(String, String)> {
    if old == new {
        return Err(NoteError::new("a note cannot supersede itself"));
    }
    let path = store::note_path(root, old)?;
    let mut note = store::load(root, old)?;
    let resolved = resolve(root, new)?;
    if resolved.chain.iter().any(|id| id == old) {
        let mut shown = vec![old.to_string()];
        shown.extend(resolved.chain);
        return Err(NoteError::new(format!(
            "supersede cycle: {}",
            shown.join(" -> ")
        )));
    }
    note.set("status", MetaVal::Str("superseded".into()));
    note.set("superseded_by", MetaVal::Str(new.to_string()));
    note.set("updated", MetaVal::Str(clock::today()));
    meta::write_note_file(&path, &meta::render(&note)).map_err(|err| NoteError::new(err.to_string()))?;
    Ok((old.to_string(), new.to_string()))
}

pub fn check(root: &Path) -> NoteResult<(Vec<String>, Vec<String>)> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let found = store::iter_notes(root, "")?;
    let paths: Vec<&Path> = found.iter().map(|(_, path)| path.as_path()).collect();
    let texts = meta::read_note_texts(&paths);
    for ((note_id, _), text) in found.iter().zip(texts) {
        let where_ = format!("{note_id}:");
        if !grammar::valid_id(note_id) {
            errors.push(format!(
                "{where_} invalid id (segments must match {})",
                grammar::SEGMENT
            ));
        }
        let text = match text {
            Ok(text) => text,
            Err(err) => {
                errors.push(format!("{where_} {err}"));
                continue;
            }
        };
        let structural = meta::frontmatter_issues(&text);
        if !structural.is_empty() {
            let stop = structural.iter().any(|issue| issue.ends_with("frontmatter"));
            for issue in structural {
                errors.push(format!("{where_} {issue}"));
            }
            if stop {
                continue;
            }
        }
        let note = meta::parse(&text);
        if note.get_str("kind").map(|k| KINDS.contains(&k)) != Some(true) {
            errors.push(format!(
                "{where_} kind must be one of: {}",
                grammar::joined(KINDS)
            ));
        }
        match note.get("summary") {
            Some(MetaVal::Str(summary)) if !summary.trim().is_empty() => {
                let len = summary.chars().count();
                if len > SUMMARY_MAX {
                    errors.push(format!(
                        "{where_} summary is {len} chars; max {SUMMARY_MAX}"
                    ));
                }
            }
            _ => errors.push(format!("{where_} missing summary")),
        }
        let status_val = note.get("status");
        let status = status_val.and_then(MetaVal::as_str).unwrap_or("active");
        let status_known = match status_val {
            None => true,
            Some(MetaVal::Str(text)) => STATUSES.contains(&text.as_str()),
            Some(MetaVal::List(_)) => false,
        };
        if !status_known {
            errors.push(format!(
                "{where_} status must be one of: {}",
                grammar::joined(STATUSES)
            ));
        } else if status == "superseded" || note.get("superseded_by").is_some() {
            if let Err(err) = resolve(root, &note_id) {
                errors.push(format!("{where_} {err}"));
            }
        }
        if let Some(tags) = note.get("tags") {
            for issue in tag_issues(tags) {
                errors.push(format!("{where_} {issue}"));
            }
        }
        match note.get("author") {
            None => warnings.push(format!("{where_} missing author")),
            Some(MetaVal::Str(author)) if !author.trim().is_empty() => {}
            _ => errors.push(format!("{where_} empty author")),
        }
        match note.get("updated") {
            None => warnings.push(format!("{where_} missing updated")),
            Some(MetaVal::Str(updated)) if valid_date(updated) => {}
            _ => errors.push(format!("{where_} updated must be YYYY-MM-DD")),
        }
        let words = grammar::py_words(&note.body);
        if words == 0 {
            errors.push(format!("{where_} empty body"));
        } else if words > WORD_BUDGET {
            warnings.push(format!(
                "{where_} body is {words} words; consider splitting (budget {WORD_BUDGET})"
            ));
        }
    }
    Ok((errors, warnings))
}

fn tag_issues(tags: &MetaVal) -> Vec<String> {
    let MetaVal::List(items) = tags else {
        return vec!["tags must be an inline list like [a, b]".to_string()];
    };
    let bad: Vec<_> = items
        .iter()
        .filter(|tag| !grammar::valid_tag(tag))
        .cloned()
        .collect();
    if bad.is_empty() {
        Vec::new()
    } else {
        vec![format!("invalid tags: {}", bad.join(", "))]
    }
}

pub fn format_ls(rows: &[Row]) -> String {
    let width = rows.iter().map(|row| row.id.chars().count()).max().unwrap_or(0);
    let mut out = String::new();
    for row in rows {
        let mark = if row.status == "superseded" {
            format!(
                " [-> {}]",
                row.superseded_by.as_deref().unwrap_or("?")
            )
        } else if row.status == "draft" {
            " [draft]".to_string()
        } else {
            String::new()
        };
        let id_pad = " ".repeat(width.saturating_sub(row.id.chars().count()));
        let kind_pad = " ".repeat(10usize.saturating_sub(row.kind.chars().count()));
        out.push_str(&row.id);
        out.push_str(&id_pad);
        out.push_str("  ");
        out.push_str(&row.kind);
        out.push_str(&kind_pad);
        out.push_str("  ");
        out.push_str(&row.summary);
        out.push_str(&mark);
        if let Some(details) = &row.details {
            if let Some(author) = &details.author {
                out.push_str(&format!("  by {author}"));
            }
            out.push_str(&format!("  at {}", details.modified));
        }
        out.push('\n');
    }
    out
}

/// The line `ls --limit` prints after the rows when it left notes out.
pub fn format_more(more: usize) -> String {
    if more == 0 {
        return String::new();
    }
    format!("... {more} more (raise --limit, narrow the scope, or use --since)\n")
}

pub fn format_get(resolved: &Resolved) -> String {
    let kind = resolved.note.get_str("kind").unwrap_or("?");
    let summary = resolved.note.get_str("summary").unwrap_or("");
    let mut out = format!(
        "## {} ({kind}) \u{2014} {summary}\n",
        resolved.final_id
    );
    if resolved.chain.len() > 1 {
        out.push_str(&format!("(superseded: {})\n", resolved.chain.join(" -> ")));
    }
    out.push_str(resolved.note.body.trim_end_matches('\n'));
    out.push('\n');
    out
}

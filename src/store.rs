//! `.swarmnotes/` lookup, exclusive create, and directory walks.

use crate::error::{NoteError, NoteResult};
use crate::grammar::{self, DIR_NAME};
use crate::meta::{self, MetaVal, Note};
use std::fs;
use std::path::{Path, PathBuf};

pub fn find_root() -> NoteResult<PathBuf> {
    match locate()? {
        Some(root) => Ok(root),
        None => {
            let prog = crate::prog::current_prog();
            Err(NoteError::new(format!(
                "no {DIR_NAME}/ found (run `{prog} init` or set SWARMNOTES_DIR)"
            )))
        }
    }
}

/// `$SWARMNOTES_DIR`, else the nearest `.swarmnotes/` walking up from the
/// working directory. `None` when there is no store.
pub fn locate() -> NoteResult<Option<PathBuf>> {
    if let Some(env) = std::env::var_os("SWARMNOTES_DIR") {
        if !env.is_empty() {
            let raw = env.to_string_lossy().to_string();
            let root = PathBuf::from(&env);
            if !root.is_dir() {
                return Err(NoteError::new(format!(
                    "SWARMNOTES_DIR={raw} is not a directory"
                )));
            }
            return Ok(Some(root));
        }
    }
    let here = std::env::current_dir().map_err(|err| NoteError::new(err.to_string()))?;
    let here = canonical_dir(here);
    let mut cursor = Some(here.as_path());
    while let Some(dir) = cursor {
        let candidate = dir.join(DIR_NAME);
        if candidate.is_dir() {
            return Ok(Some(candidate));
        }
        cursor = dir.parent();
    }
    Ok(None)
}

/// The directory a scope names under the store.
pub fn scope_dir(root: &Path, scope: &str) -> PathBuf {
    py_join(root, scope.trim_matches('/'))
}

pub fn note_path(root: &Path, note_id: &str) -> NoteResult<PathBuf> {
    if !grammar::valid_id(note_id) {
        return Err(NoteError::new(format!(
            "invalid note id: {}",
            crate::pystr::py_repr(note_id)
        )));
    }
    Ok(root.join(format!("{note_id}.md")))
}

pub fn note_id_of(root: &Path, path: &Path) -> String {
    let rel = match path.strip_prefix(root) {
        Ok(rel) => rel.to_path_buf(),
        Err(_) => path.to_path_buf(),
    };
    let mut text = rel.to_string_lossy().replace('\\', "/");
    if let Some(stripped) = text.strip_suffix(".md") {
        text = stripped.to_string();
    } else if cfg!(windows) {
        if text.len() >= 3 && text[text.len() - 3..].eq_ignore_ascii_case(".md") {
            text.truncate(text.len() - 3);
        }
    }
    text
}

pub fn load(root: &Path, note_id: &str) -> NoteResult<Note> {
    let path = note_path(root, note_id)?;
    if !path.is_file() {
        return Err(NoteError::new(format!("no such note: {note_id}")));
    }
    Ok(meta::parse(&meta::read_note_text(&path)?))
}

pub fn iter_notes(root: &Path, scope: &str) -> NoteResult<Vec<(String, PathBuf)>> {
    let scope = scope.trim_matches('/');
    let base = py_join(root, scope);
    if !base.is_dir() {
        return Err(NoteError::new(format!("no such scope: {scope}")));
    }
    let mut files = Vec::new();
    walk_md(&base, &mut files)?;
    files.sort_by(|a, b| a.components().cmp(b.components()));
    Ok(files
        .into_iter()
        .map(|path| (note_id_of(root, &path), path))
        .collect())
}

fn walk_md(dir: &Path, out: &mut Vec<PathBuf>) -> NoteResult<()> {
    let entries = fs::read_dir(dir).map_err(|err| NoteError::new(err.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|err| NoteError::new(err.to_string()))?;
        let name = entry.file_name();
        let path = entry.path();
        let ft = entry
            .file_type()
            .map_err(|err| NoteError::new(err.to_string()))?;
        if ft.is_symlink() {
            if path.is_file() && is_md_name(&name) {
                out.push(path);
            }
            continue;
        }
        if ft.is_dir() {
            walk_md(&path, out)?;
        } else if ft.is_file() && is_md_name(&name) {
            out.push(path);
        }
    }
    Ok(())
}

fn is_md_name(name: &std::ffi::OsStr) -> bool {
    let text = name.to_string_lossy();
    if cfg!(windows) {
        text.len() >= 3 && text.as_bytes()[text.len() - 3..].eq_ignore_ascii_case(b".md")
    } else {
        text.ends_with(".md")
    }
}

fn py_join(root: &Path, scope: &str) -> PathBuf {
    if scope.is_empty() {
        return root.to_path_buf();
    }
    let mut out = root.to_path_buf();
    for part in scope.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        out.push(part);
    }
    out
}

pub fn create(root: &Path, note_id: &str, note: &Note) -> NoteResult<PathBuf> {
    let path = note_path(root, note_id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| NoteError::new(err.to_string()))?;
    }
    match meta::create_note_file(&path, &meta::render(note)) {
        Ok(()) => Ok(path),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Err(NoteError::new(
            format!("note already exists: {note_id} (pick another id)"),
        )),
        Err(err) => Err(NoteError::new(err.to_string())),
    }
}

pub fn ensure_store(root: &Path) -> NoteResult<PathBuf> {
    fs::create_dir_all(root.join("_shared")).map_err(|err| NoteError::new(err.to_string()))?;
    let note = Note {
        fields: vec![
            ("kind".into(), MetaVal::Str("format".into())),
            (
                "summary".into(),
                MetaVal::Str("Shape of the final report every agent returns.".into()),
            ),
            // Kept as "examen" so init matches Examen 0.3.0 byte for byte.
            ("author".into(), MetaVal::Str("examen".into())),
            ("updated".into(), MetaVal::Str(crate::clock::today())),
        ],
        body: DEFAULT_REPORT_FORMAT.to_string(),
    };
    match create(root, "_shared/report-format", &note) {
        Ok(_) => Ok(root.to_path_buf()),
        Err(err) if err.message.contains("already exists") => Ok(root.to_path_buf()),
        Err(err) => Err(err),
    }
}

pub fn canonical_dir(path: PathBuf) -> PathBuf {
    match fs::canonicalize(&path) {
        Ok(found) => strip_verbatim(found),
        Err(_) => path,
    }
}

fn strip_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

pub fn init_display(path: &str) -> String {
    let path = if path.is_empty() { "." } else { path };
    if path == "." {
        return DIR_NAME.to_string();
    }
    let sep = std::path::MAIN_SEPARATOR;
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return format!("{sep}{DIR_NAME}");
    }
    format!("{trimmed}{sep}{DIR_NAME}")
}

pub const DEFAULT_REPORT_FORMAT: &str = "\
End your work with a report in this shape, and nothing longer:\n\
\n\
Status: done | blocked | partial\n\
Changed: files touched, one line each\n\
Notes written: IDs of finding/attempt notes you created\n\
Open questions: anything the parent must decide (or \"none\")\n\
\n\
Put details in notes, not in the report. The parent fetches them only if needed.\n";

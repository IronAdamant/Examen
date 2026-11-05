use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

fn run_in(args: &[&str], stdin: &str, tty: bool) -> (i32, String, String) {
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    relatum::cli::run(&owned, "relatum", stdin, tty)
}

fn run(args: &[&str]) -> (i32, String, String) {
    run_in(args, "", true)
}

struct Scratch {
    _guard: std::sync::MutexGuard<'static, ()>,
    dir: PathBuf,
    old: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let guard = LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "relatum-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let old = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        std::env::remove_var("SWARMNOTES_DIR");
        Self {
            _guard: guard,
            dir,
            old,
        }
    }

    fn init(&self) {
        let (code, _, err) = run(&["init"]);
        assert_eq!(code, 0, "{err}");
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::env::remove_var("SWARMNOTES_DIR");
        let _ = std::env::set_current_dir(&self.old);
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn note_text(id: &str) -> String {
    let path = Path::new(".swarmnotes").join(format!("{id}.md"));
    relatum::meta::read_note_text(&path).unwrap()
}

#[test]
fn init_creates_report_format_and_is_idempotent() {
    let _scratch = Scratch::new();
    _scratch.init();
    let (code, out, _) = run(&["ls"]);
    assert_eq!(code, 0);
    assert!(out.contains("_shared/report-format"));
    assert_eq!(run(&["init"]).0, 0);
    let notes: Vec<_> = walk_md(Path::new(".swarmnotes"));
    assert_eq!(notes.len(), 1);
    let (code, out, err) = run(&["check"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("ok"));
}

#[test]
fn init_custom_path() {
    let _scratch = Scratch::new();
    let (code, _, err) = run(&["init", "nested"]);
    assert_eq!(code, 0, "{err}");
    assert!(Path::new("nested/.swarmnotes/_shared/report-format.md").is_file());
}

#[test]
fn new_get_tags_and_refuse_overwrite() {
    let _scratch = Scratch::new();
    _scratch.init();
    assert_eq!(run(&["new", "net/retry", "--kind", "decision", "--summary", "backoff", "--body", "use jitter"]).0, 0);
    let (code, out, _) = run(&["get", "net/retry"]);
    assert_eq!(code, 0);
    assert!(out.contains("## net/retry (decision) \u{2014} backoff"));
    assert!(out.contains("use jitter"));
    let (code, _, err) = run(&["new", "a", "--kind", "decision", "--summary", "s", "--body", "b"]);
    assert_eq!(code, 0, "{err}");
    let (code, _, err) = run(&["new", "a", "--kind", "decision", "--summary", "s", "--body", "other"]);
    assert_eq!(code, 1);
    assert!(err.contains("already exists"));
    assert!(note_text("a").contains("\nb\n"));
    assert_eq!(run(&["new", "t", "--kind", "finding", "--summary", "s", "--body", "b", "--tags", "net, http"]).0, 0);
    let root = relatum::store::find_root().unwrap();
    let note = relatum::store::load(&root, "t").unwrap();
    match note.get("tags") {
        Some(relatum::meta::MetaVal::List(tags)) => assert_eq!(tags, &vec!["net".to_string(), "http".to_string()]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn new_validates_and_reads_stdin() {
    let _scratch = Scratch::new();
    _scratch.init();
    assert!(run(&["new", "Bad/ID", "--kind", "decision", "--summary", "s", "--body", "b"]).2.contains("invalid note id"));
    assert!(run(&["new", "x", "--kind", "nope", "--summary", "s", "--body", "b"]).2.contains("kind must be"));
    let long = "y".repeat(101);
    assert!(run(&["new", "x", "--kind", "decision", "--summary", &long, "--body", "b"]).2.contains("max 100"));
    assert!(run(&["new", "x", "--kind", "decision", "--summary", "   ", "--body", "b"]).2.contains("missing summary"));
    assert!(run(&["new", "x", "--kind", "decision", "--summary", "s", "--body", "b", "--tags", "Nope"]).2.contains("invalid tags"));
    let (code, _, err) = run(&["new", "e", "--kind", "decision", "--summary", "s", "--body", "  \n"]);
    assert_eq!(code, 1);
    assert!(err.contains("empty body"));
    assert!(!Path::new(".swarmnotes/e.md").exists());
    let (code, _, err) = run_in(&["new", "piped", "--kind", "context", "--summary", "from stdin"], "piped body\n", false);
    assert_eq!(code, 0, "{err}");
    assert!(note_text("piped").contains("piped body"));
    let (code, _, err) = run(&["new", "s", "--kind", "decision", "--summary", "  hello  ", "--body", "b", "--author", "parent"]);
    assert_eq!(code, 0, "{err}");
    let note = relatum::store::load(&relatum::store::find_root().unwrap(), "s").unwrap();
    assert_eq!(note.get_str("summary"), Some("hello"));
    assert_eq!(note.get_str("author"), Some("parent"));
    let updated = note.get_str("updated").unwrap();
    assert!(updated.len() == 10 && updated.as_bytes()[4] == b'-');
}

#[test]
fn ls_filters_and_draft_mark() {
    let _scratch = Scratch::new();
    _scratch.init();
    assert_eq!(run(&["new", "net/a", "--kind", "decision", "--summary", "s", "--body", "b", "--tags", "net"]).0, 0);
    assert_eq!(run(&["new", "net/client/b", "--kind", "finding", "--summary", "s", "--body", "b", "--tags", "http"]).0, 0);
    assert_eq!(run(&["new", "ui/c", "--kind", "decision", "--summary", "s", "--body", "b"]).0, 0);
    let (_, out, _) = run(&["ls", "net"]);
    assert!(out.contains("net/a"));
    assert!(out.contains("net/client/b"));
    assert!(!out.contains("ui/c"));
    let (_, out, _) = run(&["ls", "--kind", "finding"]);
    assert_eq!(out.split_whitespace().next(), Some("net/client/b"));
    let (_, out, _) = run(&["ls", "--tag", "net"]);
    assert!(out.contains("net/a") && !out.contains("net/client/b"));
    assert!(run(&["ls", "--kind", "nope"]).2.contains("kind must be"));
    assert!(run(&["ls", "--tag", "Bad"]).2.contains("invalid tag"));
    let root = relatum::store::find_root().unwrap();
    let mut note = relatum::store::load(&root, "net/a").unwrap();
    note.set("status", relatum::meta::MetaVal::Str("draft".into()));
    relatum::meta::write_note_file(&relatum::store::note_path(&root, "net/a").unwrap(), &relatum::meta::render(&note)).unwrap();
    // draft was set on net/a, but the python test writes a separate note `d`.
    assert_eq!(run(&["new", "d", "--kind", "context", "--summary", "draft note", "--body", "body"]).0, 0);
    let mut note = relatum::store::load(&root, "d").unwrap();
    note.set("status", relatum::meta::MetaVal::Str("draft".into()));
    relatum::meta::write_note_file(&relatum::store::note_path(&root, "d").unwrap(), &relatum::meta::render(&note)).unwrap();
    assert!(run(&["ls"]).1.contains("[draft]"));
}

#[test]
fn supersede_hides_follows_and_rejects_cycles() {
    let _scratch = Scratch::new();
    _scratch.init();
    assert_eq!(run(&["new", "old", "--kind", "decision", "--summary", "s", "--body", "old body"]).0, 0);
    assert_eq!(run(&["new", "new", "--kind", "decision", "--summary", "s", "--body", "new body"]).0, 0);
    assert_eq!(run(&["supersede", "old", "new"]).0, 0);
    let (_, out, _) = run(&["ls"]);
    assert!(!out.contains("old "));
    assert!(run(&["ls", "--all"]).1.contains("[-> new]"));
    let (_, out, _) = run(&["get", "old"]);
    assert!(out.contains("new body") && !out.contains("old body"));
    assert!(out.contains("superseded: old -> new"));

    assert_eq!(run(&["new", "a", "--kind", "decision", "--summary", "s", "--body", "body-a"]).0, 0);
    assert_eq!(run(&["new", "b", "--kind", "decision", "--summary", "s", "--body", "body-b"]).0, 0);
    assert_eq!(run(&["new", "c", "--kind", "decision", "--summary", "s", "--body", "body-c"]).0, 0);
    assert_eq!(run(&["supersede", "b", "c"]).0, 0);
    assert_eq!(run(&["supersede", "a", "b"]).0, 0);
    let (code, out, err) = run(&["get", "a"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("body-c"));
    assert!(out.contains("superseded: a -> b -> c"));
    assert!(!out.contains("body-a") && !out.contains("body-b"));

    let (code, _, err) = run(&["supersede", "old", "missing"]);
    assert_eq!(code, 1);
    assert!(err.contains("no such note"));
    let (code, _, err) = run(&["supersede", "c", "c"]);
    assert_eq!(code, 1);
    assert!(err.contains("cannot supersede itself"));
    let (code, _, err) = run(&["supersede", "c", "a"]);
    assert_eq!(code, 1);
    assert!(err.contains("cycle"));
    let note = relatum::store::load(&relatum::store::find_root().unwrap(), "c").unwrap();
    assert_ne!(note.get_str("status"), Some("superseded"));
}

#[test]
fn get_reports_a_missing_id_and_separates_notes() {
    let _scratch = Scratch::new();
    _scratch.init();
    assert_eq!(run(&["new", "a", "--kind", "decision", "--summary", "s", "--body", "a body"]).0, 0);
    let (code, out, err) = run(&["get", "missing", "a"]);
    assert_eq!(code, 1);
    assert!(out.contains("a body"));
    assert!(err.contains("no such note: missing"));
    assert_eq!(run(&["new", "b", "--kind", "decision", "--summary", "s", "--body", "beta"]).0, 0);
    let (code, out, err) = run(&["get", "a", "b"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.find("a body").unwrap() < out.find("beta").unwrap());
    assert!(out.contains("\n\n## b"));
}

#[test]
fn check_flags_the_python_suite() {
    {
        let scratch = Scratch::new();
        scratch.init();
        let root = Path::new(".swarmnotes");
        fs::write(root.join("nofm.md"), "just text\n").unwrap();
        fs::write(root.join("cyc1.md"), "---\nkind: decision\nsummary: s\nstatus: superseded\nsuperseded_by: cyc2\n---\n\nb\n").unwrap();
        fs::write(root.join("cyc2.md"), "---\nkind: decision\nsummary: s\nstatus: superseded\nsuperseded_by: cyc1\n---\n\nb\n").unwrap();
        assert_eq!(run(&["new", "long", "--kind", "decision", "--summary", "s", "--body", &"word ".repeat(400)]).0, 0);
        let (code, out, _) = run(&["check"]);
        assert_eq!(code, 1);
        assert!(out.contains("nofm: missing frontmatter"));
        assert!(out.contains("supersede cycle"));
        assert!(out.contains("long: body is 400 words"));
    }
    let scratch = Scratch::new();
    scratch.init();
    assert_eq!(run(&["new", "edge", "--kind", "decision", "--summary", "s", "--body", &"word ".repeat(300), "--author", "p"]).0, 0);
    assert_eq!(run(&["new", "long", "--kind", "decision", "--summary", "s", "--body", &"word ".repeat(301), "--author", "p"]).0, 0);
    let (code, out, _) = run(&["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("long: body is 301 words"));
    assert!(!out.contains("edge:"));
    assert!(!out.contains("ok"));
}

#[test]
fn check_structural_and_chain_errors() {
    let _scratch = Scratch::new();
    _scratch.init();
    let root = Path::new(".swarmnotes");
    fs::write(root.join("empty.md"), "---\nkind: decision\nsummary: s\nauthor: p\nupdated: 2026-10-02\n---\n\n").unwrap();
    fs::write(root.join("bad.md"), "---\nkind: decision\nsummary: s\nauthor: p\nstatus: stale\nupdated: 2026-13-40\n---\n\nbody\n").unwrap();
    fs::write(root.join("open.md"), "---\nkind: decision\nsummary: s\n").unwrap();
    fs::write(root.join("bin.md"), [0xff, 0xfe]).unwrap();
    let (code, out, _) = run(&["check"]);
    assert_eq!(code, 1);
    assert!(out.contains("empty: empty body"));
    assert!(out.contains("bad: status must be one of"));
    assert!(out.contains("bad: updated must be YYYY-MM-DD"));
    assert!(out.contains("open: unclosed frontmatter"));
    assert!(out.contains("bin: not valid utf-8"));

    fs::write(root.join("dup.md"), "---\nkind: decision\nkind: finding\nsummary: s\nauthor: p\nupdated: 2026-10-02\nowner: me\ntags: nope\n---\n\nbody\n").unwrap();
    let (_, out, _) = run(&["check"]);
    assert!(out.contains("dup: duplicate frontmatter key: kind"));
    assert!(out.contains("dup: unknown frontmatter key: owner"));
    assert!(out.contains("dup: tags must be an inline list"));

    fs::write(root.join("gone.md"), "---\nkind: decision\nsummary: s\nauthor: p\nupdated: 2026-10-02\nstatus: superseded\nsuperseded_by: missing\n---\n\nbody\n").unwrap();
    fs::write(root.join("nosup.md"), "---\nkind: decision\nsummary: s\nauthor: p\nupdated: 2026-10-02\nstatus: superseded\n---\n\nbody\n").unwrap();
    fs::write(root.join("stray.md"), "---\nkind: decision\nsummary: s\nauthor: p\nupdated: 2026-10-02\nsuperseded_by: _shared/report-format\n---\n\nbody\n").unwrap();
    let (_, out, _) = run(&["check"]);
    assert!(out.contains("gone: no such note: missing"));
    assert!(out.contains("nosup: superseded but no superseded_by"));
    assert!(out.contains("stray: superseded_by set but status is not superseded"));
}

#[test]
fn supersede_hop_limit() {
    let _scratch = Scratch::new();
    _scratch.init();
    let root = relatum::store::find_root().unwrap();
    let last = relatum::grammar::MAX_SUPERSEDE_HOPS;
    for i in 0..=last {
        let mut fields = vec![
            ("kind".into(), relatum::meta::MetaVal::Str("decision".into())),
            ("summary".into(), relatum::meta::MetaVal::Str("s".into())),
            ("author".into(), relatum::meta::MetaVal::Str("p".into())),
            ("updated".into(), relatum::meta::MetaVal::Str("2026-10-02".into())),
        ];
        if i < last {
            fields.push(("status".into(), relatum::meta::MetaVal::Str("superseded".into())));
            fields.push(("superseded_by".into(), relatum::meta::MetaVal::Str(format!("h{}", i + 1))));
        }
        relatum::store::create(&root, &format!("h{i}"), &relatum::meta::Note { fields, body: "body".into() }).unwrap();
    }
    let (code, out, _) = run(&["check"]);
    assert_eq!(code, 1);
    assert!(out.contains("longer than"));
}

#[test]
fn swarmnotes_dir_and_subdirectory_lookup() {
    let scratch = Scratch::new();
    scratch.init();
    let other = scratch.dir.join("elsewhere");
    fs::create_dir(&other).unwrap();
    std::env::set_var("SWARMNOTES_DIR", &other);
    assert_eq!(run(&["new", "x", "--kind", "decision", "--summary", "s", "--body", "b"]).0, 0);
    assert!(other.join("x.md").is_file());
    assert!(!Path::new(".swarmnotes/x.md").exists());
    std::env::set_var("SWARMNOTES_DIR", scratch.dir.join("nope"));
    let (code, _, err) = run(&["ls"]);
    assert_eq!(code, 1);
    assert!(err.contains("not a directory"));
    std::env::remove_var("SWARMNOTES_DIR");
    let sub = scratch.dir.join("deep/er");
    fs::create_dir_all(&sub).unwrap();
    std::env::set_current_dir(&sub).unwrap();
    assert!(run(&["ls"]).1.contains("_shared/report-format"));
    let (code, _, err) = run(&["ls", "nope"]);
    assert_eq!(code, 1);
    assert!(err.contains("no such scope"));
}

#[test]
fn prog_name_alias_and_example_store() {
    assert_eq!(relatum::prog::prog_name("swarmnotes"), "swarmnotes");
    assert_eq!(relatum::prog::prog_name("/usr/local/bin/relatum"), "relatum");
    assert_eq!(relatum::prog::prog_name("relatum-mcp"), "relatum");
    assert_eq!(relatum::prog::prog_name("examen.py"), "relatum");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/retry-swarm/.swarmnotes");
    let (errors, warnings) = relatum::ops::check(&root).unwrap();
    assert_eq!((errors, warnings), (Vec::<String>::new(), Vec::<String>::new()));
}

#[test]
fn dates_reject_impossible_calendar_days() {
    assert!(relatum::clock::valid_date("2024-02-29"));
    assert!(!relatum::clock::valid_date("2026-02-29"));
    assert!(!relatum::clock::valid_date("2026-13-40"));
    assert!(!relatum::clock::valid_date("0000-01-01"));
    assert!(relatum::clock::valid_date("0001-01-01"));
    let today = relatum::clock::today();
    assert_eq!(today.len(), 10);
}

#[test]
fn skill_matches_agents_snippet_and_readme_names_the_rename_once() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let skill = fs::read_to_string(root.join("skills/relatum/SKILL.md")).unwrap();
    let snippet = fs::read_to_string(root.join("AGENTS.snippet.md")).unwrap();
    let note = relatum::meta::parse(&skill);
    assert_eq!(note.body, snippet);
    for line in ["Task:", "Read first:", "Your scope:", "Done when:", "relatum_get"] {
        assert!(snippet.contains(line), "{line}");
    }
    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    assert_eq!(readme.matches("Formerly Examen").count(), 1);
    assert!(readme.contains("no Cargo crates") || readme.contains("no Cargo crates."));
    assert!(readme.contains("relatum-mcp"));
    assert!(readme.contains("`.swarmnotes/`"));
    for banned in [
        "pip install examen",
        "pip install 'examen",
        "pip install \"examen\"",
        "https://pypi.org",
    ] {
        assert!(!readme.contains(banned), "{banned}");
    }
    assert!(readme.contains("| cli startup"));
    assert!(readme.contains("4044 KB"));
}

#[test]
fn package_name_is_the_only_rename_point_and_has_no_crates() {
    let cargo = include_str!("../Cargo.toml");
    assert!(cargo.contains("name = \"relatum\""));
    assert!(cargo.contains(&format!("version = \"{}\"", relatum::VERSION)));
    assert!(cargo.contains("publish = false"));
    assert!(cargo.contains("name = \"relatum-mcp\""));
    assert!(cargo.contains("name = \"swarmnotes\""));
    let deps = cargo.split("[dependencies]").nth(1).unwrap().split('[').next().unwrap();
    assert!(!deps.lines().any(|line| line.contains('=') && !line.trim().starts_with('#')));
    let dev = cargo.split("[dev-dependencies]").nth(1).unwrap().split('[').next().unwrap();
    assert!(!dev.lines().any(|line| line.contains('=') && !line.trim().starts_with('#')));
    assert_eq!(relatum::NAME, "relatum");
    assert_eq!(relatum::VERSION, env!("CARGO_PKG_VERSION"));
    let server = include_str!("../server.json");
    let npm = include_str!("../npm/relatum/package.json");
    let pinned = format!("\"version\": \"{}\"", relatum::VERSION);
    assert!(server.contains(&pinned) && npm.contains(&pinned), "server.json and npm/relatum must carry the crate version");
    for path in ["src"] {
        for file in rust_files(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)) {
            let text = fs::read_to_string(&file).unwrap();
            let lowered = text.to_ascii_lowercase();
            assert!(!lowered.contains("todo!"), "{}", file.display());
            assert!(!text.contains("unimplemented!"), "{}", file.display());
            for line in text.lines() {
                let code = line.split("//").next().unwrap();
                assert!(!code.contains("TODO"), "{}", file.display());
            }
        }
    }
}

fn walk_md(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_md(&path));
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            out.push(path);
        }
    }
    out
}

fn rust_files(dir: PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.exists() {
        return out;
    }
    for entry in fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_files(path));
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    out
}

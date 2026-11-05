//! Readers racing writers must see a whole note or no note, never a torn one.

use relatum::meta::{self, MetaVal};
use relatum::{ops, store};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "relatum-race-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let root = dir.join(".swarmnotes");
    store::ensure_store(&root).unwrap();
    root
}

fn add(root: &Path, id: &str, body: &str) {
    ops::add_note(root, id, "decision", id, body, Some("t"), None).unwrap();
}

fn leftovers(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if !path.to_string_lossy().ends_with(".md") {
                found.push(path.display().to_string());
            }
        }
    }
    found
}

#[test]
fn supersede_never_shows_readers_a_torn_note() {
    let root = scratch("supersede");
    add(&root, "t/old", "old body");
    add(&root, "t/n1", "one");
    add(&root, "t/n2", "two");
    ops::supersede_note(&root, "t/old", "t/n2").unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let reads = Arc::new(AtomicUsize::new(0));
    let path = store::note_path(&root, "t/old").unwrap();

    let writer = {
        let (root, stop) = (root.clone(), stop.clone());
        thread::spawn(move || {
            let mut i = 0u32;
            while !stop.load(Ordering::Relaxed) {
                let target = if i % 2 == 0 { "t/n1" } else { "t/n2" };
                ops::supersede_note(&root, "t/old", target).unwrap();
                i += 1;
            }
            i
        })
    };
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let (path, stop, reads) = (path.clone(), stop.clone(), reads.clone());
            thread::spawn(move || {
                let mut torn = Vec::new();
                while !stop.load(Ordering::Relaxed) {
                    let text = meta::read_note_text(&path).unwrap();
                    let note = meta::parse(&text);
                    let whole = note.get_str("kind") == Some("decision")
                        && note.body == "old body\n"
                        && matches!(note.get("superseded_by"), Some(MetaVal::Str(_)));
                    if !whole {
                        torn.push(text);
                    }
                    reads.fetch_add(1, Ordering::Relaxed);
                }
                torn
            })
        })
        .collect();

    thread::sleep(Duration::from_millis(1500));
    stop.store(true, Ordering::Relaxed);
    let writes = writer.join().unwrap();
    let torn: Vec<String> = readers.into_iter().flat_map(|r| r.join().unwrap()).collect();
    assert!(writes > 20, "only {writes} writes; a busy note starved the writer");
    assert!(reads.load(Ordering::Relaxed) > 100, "too few reads");
    assert!(torn.is_empty(), "{} torn reads, first: {:?}", torn.len(), torn[0]);
    assert!(leftovers(&root).is_empty(), "{:?}", leftovers(&root));
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn new_note_appears_whole_and_exactly_once() {
    let root = scratch("create");
    let stop = Arc::new(AtomicBool::new(false));
    let body = "word ".repeat(2000);
    let reader = {
        let (root, stop, body) = (root.clone(), stop.clone(), body.clone());
        thread::spawn(move || {
            let mut torn = 0usize;
            let mut seen = 0usize;
            while !stop.load(Ordering::Relaxed) {
                for i in 0..50 {
                    let path = store::note_path(&root, &format!("c/n{i}")).unwrap();
                    if let Ok(text) = meta::read_note_text(&path) {
                        seen += 1;
                        if meta::parse(&text).body.trim_end() != body.trim_end() {
                            torn += 1;
                        }
                    }
                }
            }
            (seen, torn)
        })
    };
    let writers: Vec<_> = (0..4)
        .map(|_| {
            let (root, body) = (root.clone(), body.clone());
            thread::spawn(move || {
                (0..50)
                    .filter(|i| {
                        ops::add_note(&root, &format!("c/n{i}"), "finding", "s", &body, None, None)
                            .is_ok()
                    })
                    .count()
            })
        })
        .collect();
    let created: usize = writers.into_iter().map(|w| w.join().unwrap()).sum();
    stop.store(true, Ordering::Relaxed);
    let (seen, torn) = reader.join().unwrap();
    assert_eq!(created, 50, "each id must be created by exactly one writer");
    assert_eq!(torn, 0, "{torn} torn reads of {seen}");
    assert!(leftovers(&root).is_empty(), "{:?}", leftovers(&root));
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

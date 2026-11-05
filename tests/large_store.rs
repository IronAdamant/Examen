//! Stores big enough to be read on several threads must list and lint
//! exactly as a one-at-a-time read would: same order, same first error.

use relatum::{ops, store};
use std::fs;
use std::path::PathBuf;

const NOTES: usize = 300;

fn store_with_notes(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "relatum-large-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let root = dir.join(".swarmnotes");
    store::ensure_store(&root).unwrap();
    for i in 0..NOTES {
        let id = format!("b{}/n{i:03}", i % 7);
        ops::add_note(&root, &id, "finding", &format!("note {i}"), "body", Some("t"), None).unwrap();
    }
    root
}

/// The order a sorted single-threaded walk produces.
fn expected_ids() -> Vec<String> {
    let mut ids: Vec<String> = (0..NOTES).map(|i| format!("b{}/n{i:03}", i % 7)).collect();
    ids.push("_shared/report-format".to_string());
    ids.sort_by(|a, b| a.split('/').cmp(b.split('/')));
    ids
}

#[test]
fn listing_a_large_store_keeps_walk_order_and_content() {
    let root = store_with_notes("ls");
    let rows = ops::list_notes(&root, "", None, None, false).unwrap();
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    assert_eq!(ids, expected_ids());
    for row in rows.iter().filter(|row| row.id.starts_with('b')) {
        let n: usize = row.id.rsplit('n').next().unwrap().parse().unwrap();
        assert_eq!(row.summary, format!("note {n}"));
    }
    let (errors, warnings) = ops::check(&root).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(warnings.is_empty(), "{warnings:?}");
    let _ = fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn a_bad_note_in_a_large_store_fails_the_same_way() {
    let root = store_with_notes("bad");
    fs::write(root.join("b3").join("n999.md"), b"\xff\xfe not utf-8").unwrap();
    fs::write(root.join("b5").join("n998.md"), b"\xff\xfe not utf-8").unwrap();
    let err = ops::list_notes(&root, "", None, None, false).unwrap_err();
    assert_eq!(err.message, "not valid utf-8");
    let (errors, _) = ops::check(&root).unwrap();
    assert_eq!(
        errors,
        vec!["b3/n999: not valid utf-8".to_string(), "b5/n998: not valid utf-8".to_string()]
    );
    let _ = fs::remove_dir_all(root.parent().unwrap());
}

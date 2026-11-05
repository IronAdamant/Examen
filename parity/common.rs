//! Shared helpers for the parity harness and the benchmark. std only.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Repository root (this crate's manifest directory).
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The Python that runs the frozen Examen 0.3.0 oracle. `PARITY_PYTHON`
/// wins; otherwise the first of `python3` and `python` that is Python 3.
pub fn python() -> String {
    if let Some(chosen) = std::env::var_os("PARITY_PYTHON") {
        if !chosen.is_empty() {
            return chosen.to_string_lossy().to_string();
        }
    }
    for candidate in ["python3", "python"] {
        let ok = Command::new(candidate)
            .args(["-c", "import sys; sys.exit(sys.version_info[0] != 3)"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if ok {
            return candidate.to_string();
        }
    }
    fail("no Python 3 found. Install one or set PARITY_PYTHON.")
}

/// Print to stderr and exit 1, like Python's `raise SystemExit("...")`.
pub fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

pub fn legacy_ref() -> String {
    for reference in ["python-legacy", "origin/python-legacy"] {
        let found = Command::new("git")
            .args(["rev-parse", "--verify", reference])
            .current_dir(root())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if found {
            return reference.to_string();
        }
    }
    fail("python-legacy is not available. Fetch it with: git fetch origin python-legacy")
}

/// Write examen.py and examen_mcp.py from the python-legacy branch into `dest`.
pub fn extract_legacy(dest: &Path) {
    let reference = legacy_ref();
    fs::create_dir_all(dest).unwrap_or_else(|err| fail(&format!("{}: {err}", dest.display())));
    for name in ["examen.py", "examen_mcp.py"] {
        let out = Command::new("git")
            .args(["show", &format!("{reference}:{name}")])
            .current_dir(root())
            .stderr(Stdio::inherit())
            .output()
            .unwrap_or_else(|err| fail(&format!("git show: {err}")));
        if !out.status.success() {
            fail(&format!("git show {reference}:{name} failed"));
        }
        fs::write(dest.join(name), out.stdout)
            .unwrap_or_else(|err| fail(&format!("{name}: {err}")));
    }
}

/// A new empty directory under the system temp dir, like `tempfile.mkdtemp`.
pub fn mkdtemp(prefix: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_";
    loop {
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos() as u64)
            .unwrap_or(0)
            ^ (u64::from(std::process::id()) << 32)
            ^ SEQ.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mut name = String::from(prefix);
        for _ in 0..8 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            name.push(ALPHABET[(seed % ALPHABET.len() as u64) as usize] as char);
        }
        let path = std::env::temp_dir().join(name);
        match fs::create_dir(&path) {
            Ok(()) => return path,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => fail(&format!("mkdtemp {}: {err}", path.display())),
        }
    }
}

pub fn rmtree(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

/// `Path.write_text(text, encoding="utf-8")`: text mode, so `\n` becomes the
/// platform line ending.
pub fn write_text(path: &Path, text: &str) {
    let data = if cfg!(windows) {
        text.replace('\n', "\r\n")
    } else {
        text.to_string()
    };
    fs::write(path, data).unwrap_or_else(|err| fail(&format!("{}: {err}", path.display())));
}

/// Python `str()` of a path. Rust's `display` matches it on every platform.
pub fn path_str(path: &Path) -> String {
    path.display().to_string()
}

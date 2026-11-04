//! Binary name. The crate name in Cargo.toml is the rename point.
//! `swarmnotes` remains the legacy CLI alias from Examen 0.2.

use std::cell::RefCell;

pub const NAME: &str = env!("CARGO_PKG_NAME");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

thread_local! {
    static PROG: RefCell<String> = RefCell::new(NAME.to_string());
}

pub fn set_current(prog: &str) {
    PROG.with(|slot| *slot.borrow_mut() = prog.to_string());
}

pub fn current_prog() -> String {
    PROG.with(|slot| slot.borrow().clone())
}

pub fn stem(argv0: &str) -> String {
    let base = argv0
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(argv0);
    let stem = base.split_once('.').map(|(head, _)| head).unwrap_or(base);
    stem.to_ascii_lowercase()
}

/// `swarmnotes` keeps its own help and error prefix. Every other argv0 is the crate name.
pub fn prog_name(argv0: &str) -> String {
    if stem(argv0) == "swarmnotes" {
        "swarmnotes".to_string()
    } else {
        NAME.to_string()
    }
}

pub fn tool(suffix: &str) -> String {
    format!("{NAME}_{suffix}")
}

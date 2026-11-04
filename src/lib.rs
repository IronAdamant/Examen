//! Relatum: pass-by-reference notes for agent swarms.
//!
//! The on-disk store is still `.swarmnotes/`. Notes written by Examen and
//! SwarmNotes load unchanged.

pub mod cli;
pub mod cli_help;
pub mod cli_parse;
pub mod clock;
pub mod error;
pub mod grammar;
pub mod json;
pub mod mcp;
pub mod mcp_schema;
pub mod mcp_support;
pub mod meta;
pub mod ops;
pub mod prog;
pub mod pystr;
pub mod store;

pub use error::NoteError;
pub use prog::{NAME, VERSION};

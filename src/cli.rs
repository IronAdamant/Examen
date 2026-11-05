//! The `relatum` / `swarmnotes` command line.

use crate::cli_help::{help_text, usage_only};
use crate::cli_parse::{self, Command, Parsed};
use crate::error::NoteError;
use crate::ops::{self, TagsIn};
use crate::prog;
use crate::store;
use std::path::Path;

pub fn run(args: &[String], prog_name: &str, stdin: &str, stdin_is_tty: bool) -> (i32, String, String) {
    prog::set_current(prog_name);
    let mut out = String::new();
    let mut err = String::new();
    let code = match cli_parse::parse(args) {
        Parsed::Help(cmd) => {
            out.push_str(&help_text(prog_name, cmd.as_deref()));
            0
        }
        Parsed::Fail { child, message } => {
            err.push_str(&usage_only(prog_name, child.as_deref()));
            let who = match &child {
                Some(cmd) => format!("{prog_name} {cmd}"),
                None => prog_name.to_string(),
            };
            err.push_str(&format!("{who}: error: {message}\n"));
            2
        }
        Parsed::Run(cmd) => match dispatch(cmd, stdin, stdin_is_tty, &mut out, &mut err, prog_name) {
            Ok(code) => code,
            Err(error) => {
                err.push_str(&format!("{prog_name}: {error}\n"));
                1
            }
        },
    };
    (code, out, err)
}

fn dispatch(
    cmd: Command,
    stdin: &str,
    stdin_is_tty: bool,
    out: &mut String,
    err: &mut String,
    prog_name: &str,
) -> Result<i32, NoteError> {
    match cmd {
        Command::Init { path } => {
            let root = Path::new(if path.is_empty() { "." } else { path.as_str() }).join(crate::grammar::DIR_NAME);
            store::ensure_store(&root)?;
            out.push_str(&store::init_display(&path));
            out.push('\n');
            Ok(0)
        }
        Command::New {
            id,
            kind,
            summary,
            author,
            tags,
            body,
        } => {
            let body = match body {
                Some(body) => body,
                None if stdin_is_tty => String::new(),
                None => stdin.to_string(),
            };
            let root = store::find_root()?;
            let id = ops::add_note(
                &root,
                &id,
                &kind,
                &summary,
                &body,
                author.as_deref(),
                tags.map(TagsIn::Csv),
            )?;
            out.push_str(&id);
            out.push('\n');
            Ok(0)
        }
        Command::Ls {
            scope,
            kind,
            tag,
            all,
            limit,
            since,
            details,
        } => {
            let since = since.as_deref().map(ops::parse_since).transpose()?;
            let root = store::find_root()?;
            let query = ops::LsQuery {
                scope,
                kind,
                tag,
                include_superseded: all,
                since,
                limit,
                details,
            };
            let listing = ops::list_notes_with(&root, &query)?;
            out.push_str(&ops::format_ls(&listing.rows));
            out.push_str(&ops::format_more(listing.more));
            Ok(0)
        }
        Command::Get { ids } => {
            let root = store::find_root()?;
            let mut failed = false;
            for (index, id) in ids.iter().enumerate() {
                match ops::resolve(&root, id) {
                    Ok(resolved) => {
                        if index > 0 {
                            out.push('\n');
                        }
                        out.push_str(&ops::format_get(&resolved));
                    }
                    Err(error) => {
                        err.push_str(&format!("{prog_name}: {error}\n"));
                        failed = true;
                    }
                }
            }
            Ok(if failed { 1 } else { 0 })
        }
        Command::Supersede { old, new } => {
            let root = store::find_root()?;
            let (old, new) = ops::supersede_note(&root, &old, &new)?;
            out.push_str(&format!("{old} -> {new}\n"));
            Ok(0)
        }
        Command::Check => {
            let root = store::find_root()?;
            let (errors, warnings) = ops::check(&root)?;
            for warning in &warnings {
                out.push_str(&format!("warning: {warning}\n"));
            }
            for error in &errors {
                out.push_str(&format!("error: {error}\n"));
            }
            if errors.is_empty() && warnings.is_empty() {
                out.push_str("ok\n");
            }
            Ok(if errors.is_empty() { 0 } else { 1 })
        }
        Command::Mcp => Ok(crate::mcp::serve()),
    }
}

pub fn main_from_env(args: &[String], prog_name: &str) -> i32 {
    let stdin_is_tty = std::io::IsTerminal::is_terminal(&std::io::stdin());
    let mut stdin = String::new();
    let needs_stdin = matches!(
        cli_parse::parse(args),
        Parsed::Run(Command::New { body: None, .. })
    );
    if needs_stdin && !stdin_is_tty {
        let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin);
    }
    let (code, out, err) = run(args, prog_name, &stdin, !needs_stdin || stdin_is_tty);
    let nl = if cfg!(windows) { "\r\n" } else { "\n" };
    use std::io::Write;
    let _ = std::io::stdout().write_all(out.replace('\n', nl).as_bytes());
    let _ = std::io::stderr().write_all(err.replace('\n', nl).as_bytes());
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    code
}

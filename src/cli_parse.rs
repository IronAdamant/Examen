//! Argument parsing that follows CPython 3.12 argparse for this CLI's grammar.

use crate::pystr::py_repr;

pub const COMMANDS: &[&str] = &["init", "new", "ls", "get", "supersede", "check"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Init {
        path: String,
    },
    New {
        id: String,
        kind: String,
        summary: String,
        author: Option<String>,
        tags: Option<String>,
        body: Option<String>,
    },
    Ls {
        scope: String,
        kind: Option<String>,
        tag: Option<String>,
        all: bool,
    },
    Get {
        ids: Vec<String>,
    },
    Supersede {
        old: String,
        new: String,
    },
    Check,
}

#[derive(Debug)]
pub enum Parsed {
    Help(Option<String>),
    Run(Command),
    /// `child` is the subcommand whose usage to print. None uses the top-level usage.
    Fail {
        child: Option<String>,
        message: String,
    },
}

struct Flag {
    name: &'static str,
    takes: bool,
    required: bool,
}

pub fn parse(args: &[String]) -> Parsed {
    let mut index = 0;
    let mut ended = false;
    let mut command: Option<String> = None;
    let mut early_unknown = Vec::new();
    let mut child_args = Vec::new();
    while index < args.len() {
        let token = &args[index];
        if !ended && token == "--" {
            ended = true;
            index += 1;
            continue;
        }
        if !ended && is_help(token) && command.is_none() {
            return Parsed::Help(None);
        }
        if command.is_none() {
            if !ended && is_option(token) {
                early_unknown.push(token.clone());
            } else {
                command = Some(token.clone());
            }
        } else {
            child_args.push(token.clone());
        }
        index += 1;
    }
    let Some(command) = command else {
        return Parsed::Fail {
            child: None,
            message: "the following arguments are required: cmd".to_string(),
        };
    };
    if !COMMANDS.contains(&command.as_str()) {
        return Parsed::Fail {
            child: None,
            message: format!(
                "argument cmd: invalid choice: {} (choose from {})",
                py_repr(&command),
                COMMANDS
                    .iter()
                    .map(|name| py_repr(name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
    }
    if !early_unknown.is_empty() {
        return Parsed::Fail {
            child: None,
            message: format!("unrecognized arguments: {}", early_unknown.join(" ")),
        };
    }
    match parse_child(&command, &child_args) {
        Ok(cmd) => Parsed::Run(cmd),
        Err(ChildFail::Help) => Parsed::Help(Some(command)),
        Err(ChildFail::Message(message)) => Parsed::Fail {
            child: Some(command),
            message,
        },
        Err(ChildFail::Unrecognized(extra)) => Parsed::Fail {
            child: None,
            message: format!("unrecognized arguments: {}", extra.join(" ")),
        },
    }
}

enum ChildFail {
    Help,
    Message(String),
    Unrecognized(Vec<String>),
}

fn parse_child(cmd: &str, args: &[String]) -> Result<Command, ChildFail> {
    let flags = flags_for(cmd);
    let mut values: Vec<(&str, String)> = Vec::new();
    let mut positionals = Vec::new();
    let mut unknown = Vec::new();
    let mut index = 0;
    let mut ended = false;
    while index < args.len() {
        let token = &args[index];
        if !ended && token == "--" {
            ended = true;
            index += 1;
            continue;
        }
        if !ended && is_help(token) {
            return Err(ChildFail::Help);
        }
        if !ended && is_option(token) {
            let (name, inline) = split_inline(token);
            let Some(flag) = flags.iter().find(|flag| flag.name == name) else {
                unknown.push(token.clone());
                index += 1;
                continue;
            };
            if !flag.takes {
                if let Some(explicit) = inline {
                    return Err(ChildFail::Message(format!(
                        "argument {name}: ignored explicit argument {}",
                        py_repr(&explicit)
                    )));
                }
                set_value(&mut values, flag.name, String::new());
                index += 1;
                continue;
            }
            let value = if let Some(explicit) = inline {
                index += 1;
                explicit
            } else {
                index += 1;
                if index >= args.len() {
                    return Err(ChildFail::Message(format!(
                        "argument {name}: expected one argument"
                    )));
                }
                let value = args[index].clone();
                index += 1;
                value
            };
            set_value(&mut values, flag.name, value);
            continue;
        }
        positionals.push(token.clone());
        index += 1;
    }

    let mut missing = Vec::new();
    match cmd {
        "init" => {}
        "new" => {
            if positionals.is_empty() {
                missing.push("id".to_string());
            }
        }
        "ls" => {}
        "get" => {
            if positionals.is_empty() {
                missing.push("ids".to_string());
            }
        }
        "supersede" => {
            if positionals.is_empty() {
                missing.push("old".to_string());
                missing.push("new".to_string());
            } else if positionals.len() == 1 {
                missing.push("new".to_string());
            }
        }
        "check" => {}
        _ => {}
    }
    for flag in flags {
        if flag.required && values.iter().all(|(name, _)| *name != flag.name) {
            missing.push(flag.name.to_string());
        }
    }
    if !missing.is_empty() {
        return Err(ChildFail::Message(format!(
            "the following arguments are required: {}",
            missing.join(", ")
        )));
    }

    let max_pos = match cmd {
        "init" | "ls" => 1,
        "new" => 1,
        "get" => usize::MAX,
        "supersede" => 2,
        _ => 0,
    };
    let mut extra = unknown;
    if positionals.len() > max_pos {
        extra.extend(positionals.drain(max_pos..));
    }
    if !extra.is_empty() {
        return Err(ChildFail::Unrecognized(extra));
    }

    let opt = |name: &str| -> Option<String> {
        values
            .iter()
            .rev()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.clone())
    };
    Ok(match cmd {
        "init" => Command::Init {
            path: positionals.first().cloned().unwrap_or_else(|| ".".to_string()),
        },
        "new" => Command::New {
            id: positionals.remove(0),
            kind: opt("--kind").unwrap_or_default(),
            summary: opt("--summary").unwrap_or_default(),
            author: opt("--author"),
            tags: opt("--tags"),
            body: opt("--body"),
        },
        "ls" => Command::Ls {
            scope: positionals.first().cloned().unwrap_or_default(),
            kind: opt("--kind"),
            tag: opt("--tag"),
            all: opt("--all").is_some(),
        },
        "get" => Command::Get { ids: positionals },
        "supersede" => Command::Supersede {
            old: positionals.remove(0),
            new: positionals.remove(0),
        },
        _ => Command::Check,
    })
}

fn flags_for(cmd: &str) -> Vec<Flag> {
    match cmd {
        "new" => vec![
            Flag { name: "--kind", takes: true, required: true },
            Flag { name: "--summary", takes: true, required: true },
            Flag { name: "--author", takes: true, required: false },
            Flag { name: "--tags", takes: true, required: false },
            Flag { name: "--body", takes: true, required: false },
        ],
        "ls" => vec![
            Flag { name: "--kind", takes: true, required: false },
            Flag { name: "--tag", takes: true, required: false },
            Flag { name: "--all", takes: false, required: false },
        ],
        _ => Vec::new(),
    }
}

fn set_value(values: &mut Vec<(&str, String)>, name: &'static str, value: String) {
    if let Some((_, slot)) = values.iter_mut().find(|(key, _)| *key == name) {
        *slot = value;
    } else {
        values.push((name, value));
    }
}

fn is_help(token: &str) -> bool {
    token == "-h" || token == "--help"
}

fn is_option(token: &str) -> bool {
    token.starts_with('-') && token != "-"
}

fn split_inline(token: &str) -> (&str, Option<String>) {
    if let Some((name, value)) = token.split_once('=') {
        if name.starts_with("--") && name != "--" {
            return (name, Some(value.to_string()));
        }
    }
    (token, None)
}

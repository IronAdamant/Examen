//! MCP tool groups (the suite-wide tool-groups convention). Agents choose
//! groups per project in an `agent-tools` block in the project's AGENTS.md;
//! an operator can override with `<NAME>_TOOLS` / `<NAME>_DISABLE`. Only the
//! MCP surface changes; the CLI is not affected.

use crate::prog;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Each tool's group. `core` is always on.
pub const GROUPS: &[(&str, &[&str])] = &[
    ("core", &["ls", "get", "new"]),
    ("revise", &["edit", "supersede"]),
    ("lint", &["check"]),
    ("setup", &["init"]),
];

const CORE: &str = "core";
const BLOCK: &str = "agent-tools";

/// Where a switch came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Env,
    File,
}

/// Why a tool is off, for the error a call to it returns.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Off {
    /// Not in the `tools` list.
    NotAllowed(Source),
    /// In the `disable` list, by group or by its own name (`label`).
    Disabled(String, Source),
}

/// The tools a server exposes. `Surface::default()` is everything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Surface {
    /// Tool suffix -> why it is off. Empty means all on.
    off: Vec<(&'static str, Off)>,
}

pub fn tools_var() -> String {
    format!("{}_TOOLS", prog::NAME.to_uppercase())
}

pub fn disable_var() -> String {
    format!("{}_DISABLE", prog::NAME.to_uppercase())
}

fn group_of(suffix: &str) -> &'static str {
    GROUPS
        .iter()
        .find(|(_, tools)| tools.contains(&suffix))
        .map(|(group, _)| *group)
        .unwrap_or(CORE)
}

fn all_suffixes() -> impl Iterator<Item = &'static str> {
    GROUPS.iter().flat_map(|(_, tools)| tools.iter().copied())
}

fn tools_in(group: &str) -> Vec<&'static str> {
    GROUPS
        .iter()
        .filter(|(name, _)| *name == group)
        .flat_map(|(_, tools)| tools.iter().copied())
        .collect()
}

/// A name from a list: a group, a tool (`relatum_edit` or `edit`), or
/// `all` where allowed.
enum Name {
    All,
    Group(&'static str),
    Tool(&'static str),
}

fn resolve(name: &str, allow_all: bool) -> Option<Name> {
    if allow_all && name == "all" {
        return Some(Name::All);
    }
    if let Some((group, _)) = GROUPS.iter().find(|(group, _)| *group == name) {
        return Some(Name::Group(group));
    }
    let prefix = format!("{}_", prog::NAME);
    let suffix = name.strip_prefix(&prefix).unwrap_or(name);
    all_suffixes().find(|tool| *tool == suffix).map(Name::Tool)
}

/// Names separated by commas and/or whitespace; case is ignored.
fn names(raw: &str) -> Vec<String> {
    raw.split(|c: char| c == ',' || c.is_whitespace())
        .map(str::to_lowercase)
        .filter(|part| !part.is_empty())
        .collect()
}

fn group_names() -> String {
    GROUPS.iter().map(|(group, _)| *group).collect::<Vec<_>>().join(", ")
}

fn catalogue() -> String {
    let tools: Vec<String> = all_suffixes().map(prog::tool).collect();
    format!("groups: {}; tools: {}", group_names(), tools.join(", "))
}

/// A bad name in a list.
enum Bad {
    Unknown(String),
    Core(String),
}

/// A `tools` or `disable` list and where it came from.
struct Switch<'a> {
    list: &'a str,
    source: Source,
}

fn check_tools(list: &str) -> Result<Vec<&'static str>, Bad> {
    let mut on = Vec::new();
    for name in names(list) {
        match resolve(&name, true) {
            Some(Name::All) => on.extend(all_suffixes()),
            Some(Name::Group(group)) => on.extend(tools_in(group)),
            Some(Name::Tool(tool)) => on.push(tool),
            None => return Err(Bad::Unknown(name)),
        }
    }
    Ok(on)
}

/// (tools hidden, label to remove) for each name in a disable list.
fn check_disable(list: &str) -> Result<Vec<(Vec<&'static str>, String)>, Bad> {
    let mut out = Vec::new();
    for name in names(list) {
        let (hidden, label) = match resolve(&name, false) {
            Some(Name::Group(group)) => (tools_in(group), group.to_string()),
            Some(Name::Tool(tool)) => (vec![tool], prog::tool(tool)),
            Some(Name::All) | None => return Err(Bad::Unknown(name)),
        };
        if hidden.iter().any(|tool| group_of(tool) == CORE) {
            return Err(Bad::Core(name));
        }
        out.push((hidden, label));
    }
    Ok(out)
}

impl Surface {
    /// The surface for these switches. Lists must already be valid.
    fn build(tools: Option<Switch>, disable: Option<Switch>) -> Surface {
        let mut surface = Surface::default();
        if let Some(Switch { list, source }) = tools {
            let on = check_tools(list).unwrap_or_else(|_| all_suffixes().collect());
            for tool in all_suffixes() {
                if group_of(tool) != CORE && !on.contains(&tool) {
                    surface.off.push((tool, Off::NotAllowed(source)));
                }
            }
        }
        if let Some(Switch { list, source }) = disable {
            for (hidden, label) in check_disable(list).unwrap_or_default() {
                for tool in hidden {
                    surface.off.retain(|(off, _)| *off != tool);
                    surface.off.push((tool, Off::Disabled(label.clone(), source)));
                }
            }
        }
        surface
    }

    /// Env-only surface, as in 1.2.0. `Err` is the line to print before
    /// exiting with code 2.
    pub fn parse(tools: Option<&str>, disable: Option<&str>) -> Result<Surface, String> {
        validate_env(tools, disable)?;
        Ok(Surface::build(
            tools.map(|list| Switch { list, source: Source::Env }),
            disable.map(|list| Switch { list, source: Source::Env }),
        ))
    }

    /// Whether the tool with this suffix is exposed.
    pub fn is_on(&self, suffix: &str) -> bool {
        !self.off.iter().any(|(tool, _)| *tool == suffix)
    }

    fn enabled(&self) -> Vec<&'static str> {
        all_suffixes().filter(|tool| self.is_on(tool)).collect()
    }

    /// The one-line error for a call to a switched-off tool.
    pub fn refusal(&self, suffix: &str) -> Option<String> {
        let (_, why) = self.off.iter().find(|(tool, _)| *tool == suffix)?;
        let name = prog::tool(suffix);
        let group = group_of(suffix);
        let file_line = |key: &str| format!("AGENTS.md ({BLOCK} line \"{}: {key}\")", prog::NAME);
        Some(match why {
            Off::NotAllowed(Source::Env) => format!(
                "tool \"{name}\" is disabled (group \"{group}\") by {}; add \"{group}\" to it to enable",
                tools_var()
            ),
            Off::Disabled(label, Source::Env) => format!(
                "tool \"{name}\" is disabled (group \"{group}\") by {}; remove \"{label}\" from it to enable",
                disable_var()
            ),
            Off::NotAllowed(Source::File) => format!(
                "tool \"{name}\" is disabled (group \"{group}\") by {}; add \"{group}\" to that line to enable",
                file_line("tools")
            ),
            Off::Disabled(label, Source::File) => format!(
                "tool \"{name}\" is disabled (group \"{group}\") by {}; remove \"{label}\" from that line to enable",
                file_line("disable")
            ),
        })
    }

    /// Groups on and off, for the initialize instructions.
    fn on_off(&self) -> (String, String) {
        let (mut on, mut off) = (Vec::new(), Vec::new());
        for (group, tools) in GROUPS {
            let live: Vec<&str> = tools.iter().copied().filter(|tool| self.is_on(tool)).collect();
            if live.len() == tools.len() {
                on.push(group.to_string());
            } else if live.is_empty() {
                off.push(group.to_string());
            } else {
                let names: Vec<String> = live.iter().map(|tool| prog::tool(tool)).collect();
                on.push(format!("{group} (only {})", names.join(", ")));
            }
        }
        let off = if off.is_empty() { "none".to_string() } else { off.join(", ") };
        (on.join(", "), off)
    }
}

fn validate_env(tools: Option<&str>, disable: Option<&str>) -> Result<(), String> {
    let fail = |var: String, bad: Bad| match bad {
        Bad::Unknown(name) => format!("{var}: unknown group \"{name}\"; {}", catalogue()),
        Bad::Core(name) => format!(
            "{var}: group \"{CORE}\" is always on and cannot be disabled (\"{name}\"); {}",
            catalogue()
        ),
    };
    if let Some(list) = tools {
        check_tools(list).map_err(|bad| fail(tools_var(), bad))?;
    }
    if let Some(list) = disable {
        check_disable(list).map_err(|bad| fail(disable_var(), bad))?;
    }
    Ok(())
}

/// This tool's valid lines from an AGENTS.md `agent-tools` block, plus one
/// warning per line that was ignored.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FileRules {
    pub tools: Option<String>,
    pub disable: Option<String>,
    pub warnings: Vec<String>,
}

/// Read this tool's lines from AGENTS.md text. Errors never fail: the line
/// is ignored and a warning is kept for the initialize instructions.
pub fn parse_agents_md(text: &str) -> FileRules {
    let mut rules = FileRules::default();
    let Some(block) = agent_tools_block(text) else {
        return rules;
    };
    let tool = prog::NAME;
    let warn = |what: String| format!("AGENTS.md {BLOCK}: {what} for {tool}; groups: {}", group_names());
    for line in block {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((who, rest)) = line.split_once(':') else {
            continue;
        };
        if who.trim().to_lowercase() != tool {
            continue;
        }
        let rest = rest.trim();
        let (key, list) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let list = list.trim();
        let slot = match key.to_lowercase().as_str() {
            "tools" => match check_tools(list) {
                Ok(_) => &mut rules.tools,
                Err(bad) => {
                    rules.warnings.push(warn(bad_text(bad)));
                    continue;
                }
            },
            "disable" => match check_disable(list) {
                Ok(_) => &mut rules.disable,
                Err(bad) => {
                    rules.warnings.push(warn(bad_text(bad)));
                    continue;
                }
            },
            other => {
                rules.warnings.push(warn(format!("unknown key \"{other}\" (use tools or disable)")));
                continue;
            }
        };
        // Several lines for one key add up.
        let joined = match slot.take() {
            Some(before) => format!("{before},{list}"),
            None => list.to_string(),
        };
        *slot = Some(joined);
    }
    rules
}

fn bad_text(bad: Bad) -> String {
    match bad {
        Bad::Unknown(name) => format!("unknown group \"{name}\""),
        Bad::Core(name) => format!("group \"{CORE}\" is always on and cannot be disabled (\"{name}\")"),
    }
}

/// The first fenced block whose info string is `agent-tools`, as lines.
/// Fences are three or more backticks or tildes; the block ends at a line
/// of at least as many of the same character.
fn agent_tools_block(text: &str) -> Option<Vec<&str>> {
    let lines: Vec<&str> = text.lines().map(|line| line.trim_end_matches('\r')).collect();
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim_start();
        index += 1;
        let Some(first) = trimmed.chars().next().filter(|c| *c == '`' || *c == '~') else {
            continue;
        };
        let run = trimmed.chars().take_while(|c| *c == first).count();
        if run < 3 {
            continue;
        }
        let info = trimmed[run..].split_whitespace().next().unwrap_or("");
        let mut body = Vec::new();
        while index < lines.len() {
            let inner = lines[index].trim();
            index += 1;
            if inner.len() >= run && inner.chars().all(|c| c == first) {
                break;
            }
            body.push(lines[index - 1]);
        }
        if info == BLOCK {
            return Some(body);
        }
    }
    None
}

/// AGENTS.md's identity, to notice an edit with one stat.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    path: PathBuf,
    modified: Option<SystemTime>,
    len: u64,
}

/// The server's groups: env switches (fixed at startup) over the AGENTS.md
/// block (re-read when the file changes) over the default.
#[derive(Debug)]
pub struct Groups {
    env_tools: Option<String>,
    env_disable: Option<String>,
    stamp: Option<Stamp>,
    /// The root once a store has been found; a store does not move during a
    /// session, so later requests cost one stat of AGENTS.md.
    store_root: Option<PathBuf>,
    file: FileRules,
    surface: Surface,
}

impl Groups {
    /// Read the env switches and AGENTS.md. `Err` is the env error line to
    /// print before exiting with code 2; AGENTS.md errors are warnings.
    pub fn from_env() -> Result<Groups, String> {
        // An empty value counts as unset, as many hosts write unset entries.
        let get = |var: &str| std::env::var(var).ok().filter(|value| !value.trim().is_empty());
        Groups::new(get(&tools_var()), get(&disable_var()))
    }

    pub fn new(env_tools: Option<String>, env_disable: Option<String>) -> Result<Groups, String> {
        validate_env(env_tools.as_deref(), env_disable.as_deref())?;
        let mut groups = Groups {
            env_tools,
            env_disable,
            stamp: None,
            store_root: None,
            file: FileRules::default(),
            surface: Surface::default(),
        };
        let stamp = groups.current_stamp();
        groups.reload(stamp);
        Ok(groups)
    }

    /// AGENTS.md's stamp at the project root, finding the root again only
    /// until a store has been found.
    fn current_stamp(&mut self) -> Option<Stamp> {
        if let Some(root) = &self.store_root {
            return stamp_at(root);
        }
        let (root, has_store) = project_root()?;
        let stamp = stamp_at(&root);
        if has_store {
            self.store_root = Some(root);
        }
        stamp
    }

    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    /// Stat AGENTS.md; if it moved or changed, re-read it. True when the
    /// set of exposed tools changed.
    pub fn refresh(&mut self) -> bool {
        let stamp = self.current_stamp();
        if stamp == self.stamp {
            return false;
        }
        let before = self.surface.enabled();
        self.reload(stamp);
        self.surface.enabled() != before
    }

    fn reload(&mut self, stamp: Option<Stamp>) {
        self.file = match &stamp {
            Some(stamp) => std::fs::read(&stamp.path)
                .map(|data| parse_agents_md(&String::from_utf8_lossy(&data)))
                .unwrap_or_default(),
            None => FileRules::default(),
        };
        self.stamp = stamp;
        // Per key: env beats the file.
        let pick = |env: &Option<String>, file: &Option<String>| match (env, file) {
            (Some(list), _) => Some((list.clone(), Source::Env)),
            (None, Some(list)) => Some((list.clone(), Source::File)),
            (None, None) => None,
        };
        let tools = pick(&self.env_tools, &self.file.tools);
        let disable = pick(&self.env_disable, &self.file.disable);
        self.surface = Surface::build(
            tools.as_ref().map(|(list, source)| Switch { list, source: *source }),
            disable.as_ref().map(|(list, source)| Switch { list, source: *source }),
        );
    }

    /// The lines that end the initialize instructions: groups on and off,
    /// how to change them, and any AGENTS.md warnings.
    /// Empty when nothing is configured (no env switch and no line for this
    /// tool in an AGENTS.md block), so the default instructions stay as they were.
    pub fn instructions_tail(&self) -> String {
        let configured = self.env_tools.is_some()
            || self.env_disable.is_some()
            || self.file != FileRules::default();
        if !configured {
            return String::new();
        }
        let (on, off) = self.surface.on_off();
        let mut out = format!(
            "Tool groups on: {on}. Off: {off}. To change them, add a line such as \
             \"{name}: disable lint\" to an {BLOCK} block in AGENTS.md",
            name = prog::NAME
        );
        if self.env_tools.is_some() || self.env_disable.is_some() {
            out.push_str(&format!(
                "; {} / {} are set and take precedence",
                tools_var(),
                disable_var()
            ));
        }
        out.push_str(".\n");
        for warning in &self.file.warnings {
            out.push_str(warning);
            out.push('\n');
        }
        out
    }
}

/// The project root: the directory holding the store, or the working
/// directory when there is none. The flag says whether a store was found.
fn project_root() -> Option<(PathBuf, bool)> {
    match crate::store::locate() {
        Ok(Some(store)) => {
            let store = crate::store::canonical_dir(store);
            store.parent().map(|parent| (parent.to_path_buf(), true))
        }
        _ => std::env::current_dir().ok().map(|dir| (dir, false)),
    }
}

fn stamp_at(root: &Path) -> Option<Stamp> {
    let path = root.join("AGENTS.md");
    let meta = std::fs::metadata(&path).ok()?;
    meta.is_file().then(|| Stamp {
        modified: meta.modified().ok(),
        len: meta.len(),
        path,
    })
}

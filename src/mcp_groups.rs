//! MCP tool groups: `<NAME>_TOOLS` and `<NAME>_DISABLE` switch parts of the
//! MCP surface on and off (the suite-wide tool-groups convention). The CLI
//! is not affected. With neither variable set, nothing changes.

use crate::prog;

/// Each tool's group. `core` is always on.
pub const GROUPS: &[(&str, &[&str])] = &[
    ("core", &["ls", "get", "new"]),
    ("revise", &["edit", "supersede"]),
    ("lint", &["check"]),
    ("setup", &["init"]),
];

const CORE: &str = "core";

/// Why a tool is off, for the error a call to it returns.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Off {
    /// Not named in `_TOOLS`; adding `group` turns it on.
    NotAllowed,
    /// Named in `_DISABLE`, by group or by its own name.
    Disabled(String),
}

/// The tools a server exposes. `Surface::default()` is everything, as with
/// no variables set.
#[derive(Debug, Clone, Default)]
pub struct Surface {
    /// Tool suffix -> why it is off. Empty means all on.
    off: Vec<(&'static str, Off)>,
    /// True when either variable was set.
    configured: bool,
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

/// A name from a variable: a group, a tool (`relatum_edit` or `edit`), or
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

fn names(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|part| part.trim().to_lowercase())
        .filter(|part| !part.is_empty())
        .collect()
}

fn catalogue() -> String {
    let groups: Vec<&str> = GROUPS.iter().map(|(group, _)| *group).collect();
    let tools: Vec<String> = all_suffixes().map(prog::tool).collect();
    format!("groups: {}; tools: {}", groups.join(", "), tools.join(", "))
}

impl Surface {
    /// Read the two variables. `Err` is the one line to print before
    /// exiting with code 2.
    pub fn from_env() -> Result<Surface, String> {
        // An empty value counts as unset, as many hosts write unset entries.
        let get = |var: &str| std::env::var(var).ok().filter(|value| !value.trim().is_empty());
        Surface::parse(get(&tools_var()).as_deref(), get(&disable_var()).as_deref())
    }

    pub fn parse(tools: Option<&str>, disable: Option<&str>) -> Result<Surface, String> {
        let mut surface = Surface {
            off: Vec::new(),
            configured: tools.is_some() || disable.is_some(),
        };
        if let Some(raw) = tools {
            let var = tools_var();
            let mut on: Vec<&str> = Vec::new();
            for name in names(raw) {
                match resolve(&name, true) {
                    Some(Name::All) => on.extend(all_suffixes()),
                    Some(Name::Group(group)) => on.extend(tools_in(group)),
                    Some(Name::Tool(tool)) => on.push(tool),
                    None => return Err(format!("{var}: unknown group \"{name}\"; {}", catalogue())),
                }
            }
            for tool in all_suffixes() {
                if group_of(tool) != CORE && !on.contains(&tool) {
                    surface.off.push((tool, Off::NotAllowed));
                }
            }
        }
        if let Some(raw) = disable {
            let var = disable_var();
            for name in names(raw) {
                let (hidden, label): (Vec<&'static str>, String) = match resolve(&name, false) {
                    Some(Name::Group(group)) => (tools_in(group), group.to_string()),
                    Some(Name::Tool(tool)) => (vec![tool], prog::tool(tool)),
                    Some(Name::All) | None => {
                        return Err(format!("{var}: unknown group \"{name}\"; {}", catalogue()))
                    }
                };
                if hidden.iter().any(|tool| group_of(tool) == CORE) {
                    return Err(format!(
                        "{var}: group \"{CORE}\" is always on and cannot be disabled (\"{name}\"); {}",
                        catalogue()
                    ));
                }
                for tool in hidden {
                    surface.off.retain(|(off, _)| *off != tool);
                    surface.off.push((tool, Off::Disabled(label.clone())));
                }
            }
        }
        Ok(surface)
    }

    /// Whether the tool with this suffix is exposed.
    pub fn is_on(&self, suffix: &str) -> bool {
        !self.off.iter().any(|(tool, _)| *tool == suffix)
    }

    /// The one-line error for a call to a switched-off tool.
    pub fn refusal(&self, suffix: &str) -> Option<String> {
        let (_, why) = self.off.iter().find(|(tool, _)| *tool == suffix)?;
        let name = prog::tool(suffix);
        let group = group_of(suffix);
        Some(match why {
            Off::NotAllowed => format!(
                "tool \"{name}\" is disabled (group \"{group}\") by {}; add \"{group}\" to it to enable",
                tools_var()
            ),
            Off::Disabled(label) => format!(
                "tool \"{name}\" is disabled (group \"{group}\") by {}; remove \"{label}\" from it to enable",
                disable_var()
            ),
        })
    }

    /// One short line for the initialize instructions. `None` when neither
    /// variable is set, so the default instructions stay as they were.
    pub fn summary(&self) -> Option<String> {
        if !self.configured {
            return None;
        }
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
        Some(format!(
            "Tool groups on: {}. Off: {off}. Set by {} / {}.",
            on.join(", "),
            tools_var(),
            disable_var()
        ))
    }
}

fn tools_in(group: &str) -> Vec<&'static str> {
    GROUPS
        .iter()
        .filter(|(name, _)| *name == group)
        .flat_map(|(_, tools)| tools.iter().copied())
        .collect()
}

//! Help and usage text that matches CPython 3.12 argparse at a pipe width of 80
//! (formatter width 78). Section text does not depend on the program name.

const WIDTH: usize = 78;

pub fn help_text(prog: &str, cmd: Option<&str>) -> String {
    let mut out = usage_lines(prog, cmd).join("\n");
    out.push_str("\n\n");
    out.push_str(tail(cmd));
    out
}

pub fn usage_only(prog: &str, cmd: Option<&str>) -> String {
    let mut out = usage_lines(prog, cmd).join("\n");
    out.push('\n');
    out
}

fn tail(cmd: Option<&str>) -> &'static str {
    match cmd {
        None => include_str!("help/main.txt"),
        Some("init") => include_str!("help/init.txt"),
        Some("new") => include_str!("help/new.txt"),
        Some("ls") => include_str!("help/ls.txt"),
        Some("get") => include_str!("help/get.txt"),
        Some("supersede") => include_str!("help/supersede.txt"),
        Some("check") => include_str!("help/check.txt"),
        Some("mcp") => include_str!("help/mcp.txt"),
        _ => "",
    }
}

fn usage_lines(prog: &str, cmd: Option<&str>) -> Vec<String> {
    let (opt, pos) = parts(cmd);
    let prog_full = match cmd {
        Some(cmd) => format!("{prog} {cmd}"),
        None => prog.to_string(),
    };
    let mut flat_parts = vec![prog_full.clone()];
    flat_parts.extend(opt.iter().map(|part| (*part).to_string()));
    flat_parts.extend(pos.iter().map(|part| (*part).to_string()));
    let flat = flat_parts.join(" ");
    let prefix = "usage: ";
    if prefix.len() + flat.len() <= WIDTH {
        return vec![format!("{prefix}{flat}")];
    }
    let indent = " ".repeat(prefix.len() + prog_full.len() + 1);
    let mut head = vec![prog_full];
    head.extend(opt.iter().map(|part| (*part).to_string()));
    let mut lines = wrap_parts(&head, &indent, true);
    if !pos.is_empty() {
        let pos: Vec<String> = pos.iter().map(|part| (*part).to_string()).collect();
        lines.extend(wrap_parts(&pos, &indent, false));
    }
    if !lines.is_empty() {
        lines[0] = format!("{prefix}{}", lines[0]);
    }
    lines
}

fn parts(cmd: Option<&str>) -> (&'static [&'static str], &'static [&'static str]) {
    match cmd {
        None => (
            &["[-h]"],
            &["{init,new,ls,get,supersede,check}", "..."],
        ),
        Some("init") => (&["[-h]"], &["[path]"]),
        Some("new") => (
            &[
                "[-h]",
                "--kind",
                "KIND",
                "--summary",
                "SUMMARY",
                "[--author AUTHOR]",
                "[--tags TAGS]",
                "[--body BODY]",
            ],
            &["id"],
        ),
        Some("ls") => (
            &["[-h]", "[--kind KIND]", "[--tag TAG]", "[--all]"],
            &["[scope]"],
        ),
        Some("get") => (&["[-h]"], &["ids", "[ids ...]"]),
        Some("supersede") => (&["[-h]"], &["old", "new"]),
        Some("check") | Some("mcp") => (&["[-h]"], &[]),
        _ => (&["[-h]"], &[]),
    }
}

fn wrap_parts(parts: &[String], indent: &str, strip_first: bool) -> Vec<String> {
    let indent_len = indent.len();
    let mut lines = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut line_len: isize = if strip_first {
        "usage: ".len() as isize - 1
    } else {
        indent_len as isize - 1
    };
    for part in parts {
        let plen = part.len() as isize;
        if line_len + 1 + plen > WIDTH as isize && !cur.is_empty() {
            lines.push(format!("{indent}{}", cur.join(" ")));
            cur.clear();
            line_len = indent_len as isize - 1;
        }
        cur.push(part);
        line_len += plen + 1;
    }
    if !cur.is_empty() {
        lines.push(format!("{indent}{}", cur.join(" ")));
    }
    if strip_first && !lines.is_empty() {
        lines[0] = lines[0][indent_len..].to_string();
    }
    lines
}

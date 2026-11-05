use std::io::Write;
use std::process::{Command, Stdio};

use relatum::cli_parse::{self, Command as Cmd, Parsed};

const INIT: &str = concat!(
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"#,
    r#""protocolVersion":"2024-11-05","capabilities":{},"#,
    r#""clientInfo":{"name":"t","version":"0"}}}"#,
);

#[test]
fn mcp_is_a_subcommand_and_top_level_help_stays_the_six_commands() {
    match cli_parse::parse(&["mcp".to_string()]) {
        Parsed::Run(Cmd::Mcp) => {}
        other => panic!("expected mcp, got {other:?}"),
    }

    let (code, out, err) = relatum::cli::run(&["mcp".into(), "-h".into()], "relatum", "", true);
    assert_eq!(code, 0, "{err}");
    assert_eq!(err, "");
    assert!(out.contains("usage: relatum mcp [-h]\n"), "{out}");
    assert!(out.contains("start the stdio MCP server\n"), "{out}");

    let (code, out, err) = relatum::cli::run(&["mcp".into(), "extra".into()], "relatum", "", true);
    assert_eq!(out, "");
    assert_eq!(code, 2, "{err}");
    assert_eq!(
        err,
        "usage: relatum [-h] {init,new,ls,get,supersede,check} ...\nrelatum: error: unrecognized arguments: extra\n"
    );

    let (code, out, err) = relatum::cli::run(&["-h".into()], "relatum", "", true);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("{init,new,ls,get,supersede,check}"));
    assert!(!out.contains("mcp"));
}

#[test]
fn mcp_subcommand_serves_stdio_initialize() {
    assert_initialize(env!("CARGO_BIN_EXE_relatum"), &["mcp"]);
    assert_initialize(env!("CARGO_BIN_EXE_relatum-mcp"), &[]);
}

fn assert_initialize(bin: &str, args: &[&str]) {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {bin}: {err}"));
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        writeln!(stdin, "{INIT}").expect("write initialize");
    }
    let output = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8(output.stdout).expect("stdout utf-8");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{bin} {args:?} status {:?}\nstdout={stdout}\nstderr={stderr}",
        output.status.code()
    );
    assert!(stdout.contains("\"name\":\"relatum\""), "{stdout}");
    let version = format!("\"version\":\"{}\"", env!("CARGO_PKG_VERSION"));
    assert!(stdout.contains(&version), "{stdout}");
}

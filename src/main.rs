fn main() {
    let mut args = std::env::args();
    let argv0 = args.next().unwrap_or_default();
    let rest: Vec<String> = args.collect();
    let code = if relatum::prog::stem(&argv0).ends_with("-mcp") {
        relatum::mcp::main_mcp(&rest)
    } else {
        let prog = relatum::prog::prog_name(&argv0);
        relatum::cli::main_from_env(&rest, &prog)
    };
    std::process::exit(code);
}

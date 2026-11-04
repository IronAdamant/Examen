fn expect(prog: &str, args: &[&str], file: &str) {
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    let (code, out, err) = relatum::cli::run(&owned, prog, "", true);
    assert_eq!(code, 0, "{err}");
    assert_eq!(err, "");
    let expected = match file {
        "examen_h" => include_str!("fixtures/help/examen_h.txt"),
        "examen_init_h" => include_str!("fixtures/help/examen_init_h.txt"),
        "examen_new_h" => include_str!("fixtures/help/examen_new_h.txt"),
        "examen_ls_h" => include_str!("fixtures/help/examen_ls_h.txt"),
        "examen_get_h" => include_str!("fixtures/help/examen_get_h.txt"),
        "examen_supersede_h" => include_str!("fixtures/help/examen_supersede_h.txt"),
        "examen_check_h" => include_str!("fixtures/help/examen_check_h.txt"),
        "relatum_h" => include_str!("fixtures/help/relatum_h.txt"),
        "relatum_init_h" => include_str!("fixtures/help/relatum_init_h.txt"),
        "relatum_new_h" => include_str!("fixtures/help/relatum_new_h.txt"),
        "relatum_ls_h" => include_str!("fixtures/help/relatum_ls_h.txt"),
        "relatum_get_h" => include_str!("fixtures/help/relatum_get_h.txt"),
        "relatum_supersede_h" => include_str!("fixtures/help/relatum_supersede_h.txt"),
        "relatum_check_h" => include_str!("fixtures/help/relatum_check_h.txt"),
        "swarmnotes_h" => include_str!("fixtures/help/swarmnotes_h.txt"),
        "swarmnotes_init_h" => include_str!("fixtures/help/swarmnotes_init_h.txt"),
        "swarmnotes_new_h" => include_str!("fixtures/help/swarmnotes_new_h.txt"),
        "swarmnotes_ls_h" => include_str!("fixtures/help/swarmnotes_ls_h.txt"),
        "swarmnotes_get_h" => include_str!("fixtures/help/swarmnotes_get_h.txt"),
        "swarmnotes_supersede_h" => include_str!("fixtures/help/swarmnotes_supersede_h.txt"),
        "swarmnotes_check_h" => include_str!("fixtures/help/swarmnotes_check_h.txt"),
        other => panic!("unknown fixture {other}"),
    };
    assert_eq!(out, expected, "{file}");
}

#[test]
fn help_matches_cpython_argparse() {
    for prog in ["examen", "relatum", "swarmnotes"] {
        expect(prog, &["-h"], &format!("{prog}_h"));
        expect(prog, &["--help"], &format!("{prog}_h"));
        for cmd in ["init", "new", "ls", "get", "supersede", "check"] {
            expect(prog, &[cmd, "-h"], &format!("{prog}_{cmd}_h"));
        }
    }
}

#[test]
fn argparse_errors_match_the_observed_cpython_text() {
    let cases = [
        (
            &[][..],
            2,
            "usage: relatum [-h] {init,new,ls,get,supersede,check} ...\nrelatum: error: the following arguments are required: cmd\n",
        ),
        (
            &["nope"][..],
            2,
            "usage: relatum [-h] {init,new,ls,get,supersede,check} ...\nrelatum: error: argument cmd: invalid choice: 'nope' (choose from 'init', 'new', 'ls', 'get', 'supersede', 'check')\n",
        ),
        (
            &["new"][..],
            2,
            "usage: relatum new [-h] --kind KIND --summary SUMMARY [--author AUTHOR]\n                   [--tags TAGS] [--body BODY]\n                   id\nrelatum new: error: the following arguments are required: id, --kind, --summary\n",
        ),
        (
            &["ls", "--kind"][..],
            2,
            "usage: relatum ls [-h] [--kind KIND] [--tag TAG] [--all] [scope]\nrelatum ls: error: argument --kind: expected one argument\n",
        ),
        (
            &["--bogus"][..],
            2,
            "usage: relatum [-h] {init,new,ls,get,supersede,check} ...\nrelatum: error: the following arguments are required: cmd\n",
        ),
        (
            &["ls", "--bogus"][..],
            2,
            "usage: relatum [-h] {init,new,ls,get,supersede,check} ...\nrelatum: error: unrecognized arguments: --bogus\n",
        ),
        (
            &["ls", "--all=true"][..],
            2,
            "usage: relatum ls [-h] [--kind KIND] [--tag TAG] [--all] [scope]\nrelatum ls: error: argument --all: ignored explicit argument 'true'\n",
        ),
        (
            &["get"][..],
            2,
            "usage: relatum get [-h] ids [ids ...]\nrelatum get: error: the following arguments are required: ids\n",
        ),
        (
            &["supersede", "only"][..],
            2,
            "usage: relatum supersede [-h] old new\nrelatum supersede: error: the following arguments are required: new\n",
        ),
        (
            &["check", "extra"][..],
            2,
            "usage: relatum [-h] {init,new,ls,get,supersede,check} ...\nrelatum: error: unrecognized arguments: extra\n",
        ),
    ];
    for (args, code, err) in cases {
        let owned: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
        let (got_code, out, got_err) = relatum::cli::run(&owned, "relatum", "", true);
        assert_eq!(out, "");
        assert_eq!(got_code, code, "{args:?}\n{got_err}");
        assert_eq!(got_err, err, "{args:?}");
    }
}

import ast
import contextlib
import io
import os
import sys
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO))

import examen  # noqa: E402


def run(*argv):
    """Run the CLI in-process; return (exit_code, stdout, stderr)."""
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = examen.main(list(argv))
    return code, out.getvalue(), err.getvalue()


class CliTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.old_cwd = os.getcwd()
        os.chdir(self.tmp.name)
        os.environ.pop("SWARMNOTES_DIR", None)
        self.assertEqual(run("init")[0], 0)

    def tearDown(self):
        os.chdir(self.old_cwd)
        os.environ.pop("SWARMNOTES_DIR", None)
        self.tmp.cleanup()

    def new(self, note_id, kind="decision", summary="s", body="b", *extra):
        return run("new", note_id, "--kind", kind, "--summary", summary, "--body", body, *extra)

    def test_init_creates_report_format(self):
        code, out, _ = run("ls")
        self.assertEqual(code, 0)
        self.assertIn("_shared/report-format", out)

    def test_init_is_idempotent(self):
        self.assertEqual(run("init")[0], 0)
        notes = list(Path(".swarmnotes").rglob("*.md"))
        self.assertEqual(len(notes), 1)

    def test_init_custom_path(self):
        code, _, err = run("init", "nested")
        self.assertEqual(code, 0, err)
        path = Path("nested") / ".swarmnotes" / "_shared" / "report-format.md"
        self.assertTrue(path.is_file())

    def test_new_then_get(self):
        self.assertEqual(self.new("net/retry", summary="backoff", body="use jitter")[0], 0)
        code, out, _ = run("get", "net/retry")
        self.assertEqual(code, 0)
        self.assertIn("## net/retry (decision) — backoff", out)
        self.assertIn("use jitter", out)

    def test_new_refuses_overwrite(self):
        self.new("a")
        code, _, err = self.new("a", body="other")
        self.assertEqual(code, 1)
        self.assertIn("already exists", err)
        self.assertIn("\nb\n", (Path(".swarmnotes") / "a.md").read_text(encoding="utf-8"))

    def test_new_validates(self):
        self.assertIn("invalid note id", self.new("Bad/ID")[2])
        self.assertIn("kind must be", self.new("x", kind="nope")[2])
        self.assertIn("max 100", self.new("x", summary="y" * 101)[2])
        self.assertIn("missing summary", self.new("x", summary="   ")[2])
        self.assertIn("invalid tags", self.new("x", "decision", "s", "b", "--tags", "Nope")[2])

    def test_new_rejects_empty_body(self):
        code, _, err = self.new("e", body="  \n")
        self.assertEqual(code, 1)
        self.assertIn("empty body", err)
        self.assertFalse((Path(".swarmnotes") / "e.md").exists())

    def test_new_reads_stdin(self):
        old = sys.stdin
        sys.stdin = io.StringIO("piped body\n")
        try:
            code, _, err = run("new", "piped", "--kind", "context", "--summary", "from stdin")
        finally:
            sys.stdin = old
        self.assertEqual(code, 0, err)
        self.assertIn("piped body", (Path(".swarmnotes") / "piped.md").read_text(encoding="utf-8"))

    def test_new_strips_summary_and_sets_updated(self):
        code, _, err = self.new("s", "decision", "  hello  ", "b", "--author", "parent")
        self.assertEqual(code, 0, err)
        meta, _ = examen.load(Path(".swarmnotes"), "s")
        self.assertEqual(meta["summary"], "hello")
        self.assertEqual(meta["author"], "parent")
        self.assertRegex(meta["updated"], r"^\d{4}-\d{2}-\d{2}$")

    def test_tags_round_trip(self):
        self.new("t", "finding", "s", "b", "--tags", "net, http")
        meta, _ = examen.load(Path(".swarmnotes"), "t")
        self.assertEqual(meta["tags"], ["net", "http"])

    def test_ls_scope_kind_and_tag(self):
        self.new("net/a", "decision", "s", "b", "--tags", "net")
        self.new("net/client/b", "finding", "s", "b", "--tags", "http")
        self.new("ui/c", "decision")
        _, out, _ = run("ls", "net")
        self.assertIn("net/a", out)
        self.assertIn("net/client/b", out)
        self.assertNotIn("ui/c", out)
        _, out, _ = run("ls", "--kind", "finding")
        self.assertEqual(out.split()[0], "net/client/b")
        _, out, _ = run("ls", "--tag", "net")
        self.assertIn("net/a", out)
        self.assertNotIn("net/client/b", out)

    def test_ls_rejects_bad_filters(self):
        code, _, err = run("ls", "--kind", "nope")
        self.assertEqual(code, 1)
        self.assertIn("kind must be", err)
        code, _, err = run("ls", "--tag", "Bad")
        self.assertEqual(code, 1)
        self.assertIn("invalid tag", err)

    def test_ls_marks_draft(self):
        self.new("d", "context", "draft note", "body")
        root = Path(".swarmnotes")
        meta, body = examen.load(root, "d")
        meta["status"] = "draft"
        examen.note_path(root, "d").write_text(examen.render(meta, body), encoding="utf-8")
        _, out, _ = run("ls")
        self.assertIn("[draft]", out)

    def test_supersede_hides_and_follows(self):
        self.new("old", body="old body")
        self.new("new", body="new body")
        self.assertEqual(run("supersede", "old", "new")[0], 0)
        _, out, _ = run("ls")
        self.assertNotIn("old ", out)
        _, out, _ = run("ls", "--all")
        self.assertIn("[-> new]", out)
        _, out, _ = run("get", "old")
        self.assertIn("new body", out)
        self.assertNotIn("old body", out)
        self.assertIn("superseded: old -> new", out)

    def test_supersede_multihop(self):
        self.new("a", body="body-a")
        self.new("b", body="body-b")
        self.new("c", body="body-c")
        self.assertEqual(run("supersede", "b", "c")[0], 0)
        self.assertEqual(run("supersede", "a", "b")[0], 0)
        code, out, err = run("get", "a")
        self.assertEqual(code, 0, err)
        self.assertIn("body-c", out)
        self.assertIn("superseded: a -> b -> c", out)
        self.assertNotIn("body-a", out)
        self.assertNotIn("body-b", out)

    def test_supersede_requires_target(self):
        self.new("old")
        code, _, err = run("supersede", "old", "missing")
        self.assertEqual(code, 1)
        self.assertIn("no such note", err)

    def test_supersede_rejects_self_and_cycle(self):
        self.new("a", body="body-a")
        self.new("b", body="body-b")
        code, _, err = run("supersede", "a", "a")
        self.assertEqual(code, 1)
        self.assertIn("cannot supersede itself", err)
        self.assertEqual(run("supersede", "a", "b")[0], 0)
        code, _, err = run("supersede", "b", "a")
        self.assertEqual(code, 1)
        self.assertIn("cycle", err)
        meta, _ = examen.load(Path(".swarmnotes"), "b")
        self.assertNotEqual(meta.get("status"), "superseded")

    def test_get_missing_reports_but_prints_rest(self):
        self.new("a", body="a body")
        code, out, err = run("get", "missing", "a")
        self.assertEqual(code, 1)
        self.assertIn("a body", out)
        self.assertIn("no such note: missing", err)

    def test_get_separates_multiple_notes(self):
        self.new("a", body="alpha")
        self.new("b", body="beta")
        code, out, err = run("get", "a", "b")
        self.assertEqual(code, 0, err)
        self.assertLess(out.index("alpha"), out.index("beta"))
        self.assertIn("\n\n## b", out)

    def test_check_ok_on_fresh_init(self):
        code, out, err = run("check")
        self.assertEqual((code, err), (0, ""))
        self.assertIn("ok", out)

    def test_check_flags_problems(self):
        root = Path(".swarmnotes")
        (root / "nofm.md").write_text("just text\n", encoding="utf-8")
        (root / "cyc1.md").write_text(
            "---\nkind: decision\nsummary: s\nstatus: superseded\nsuperseded_by: cyc2\n---\n\nb\n",
            encoding="utf-8",
        )
        (root / "cyc2.md").write_text(
            "---\nkind: decision\nsummary: s\nstatus: superseded\nsuperseded_by: cyc1\n---\n\nb\n",
            encoding="utf-8",
        )
        self.new("long", body="word " * 400)
        code, out, _ = run("check")
        self.assertEqual(code, 1)
        self.assertIn("nofm: missing frontmatter", out)
        self.assertIn("supersede cycle", out)
        self.assertIn("long: body is 400 words", out)

    def test_check_word_budget_is_warning(self):
        self.new("edge", "decision", "s", "word " * 300, "--author", "p")
        self.new("long", "decision", "s", "word " * 301, "--author", "p")
        code, out, _ = run("check")
        self.assertEqual(code, 0, out)
        self.assertIn("long: body is 301 words", out)
        self.assertNotIn("edge:", out)
        self.assertNotIn("ok", out)

    def test_check_empty_body_status_date_and_encoding(self):
        root = Path(".swarmnotes")
        (root / "empty.md").write_text(
            "---\nkind: decision\nsummary: s\nauthor: p\nupdated: 2026-10-02\n---\n\n",
            encoding="utf-8",
        )
        (root / "bad.md").write_text(
            "---\nkind: decision\nsummary: s\nauthor: p\nstatus: stale\nupdated: 2026-13-40\n---\n\nbody\n",
            encoding="utf-8",
        )
        (root / "open.md").write_text("---\nkind: decision\nsummary: s\n", encoding="utf-8")
        (root / "bin.md").write_bytes(b"\xff\xfe not utf-8")
        code, out, _ = run("check")
        self.assertEqual(code, 1)
        self.assertIn("empty: empty body", out)
        self.assertIn("bad: status must be one of", out)
        self.assertIn("bad: updated must be YYYY-MM-DD", out)
        self.assertIn("open: unclosed frontmatter", out)
        self.assertIn("bin: not valid utf-8", out)

    def test_check_unknown_duplicate_keys_and_tags(self):
        (Path(".swarmnotes") / "dup.md").write_text(
            "---\nkind: decision\nkind: finding\nsummary: s\nauthor: p\n"
            "updated: 2026-10-02\nowner: me\ntags: nope\n---\n\nbody\n",
            encoding="utf-8",
        )
        code, out, _ = run("check")
        self.assertEqual(code, 1)
        self.assertIn("dup: duplicate frontmatter key: kind", out)
        self.assertIn("dup: unknown frontmatter key: owner", out)
        self.assertIn("dup: tags must be an inline list", out)

    def test_check_dangling_supersede(self):
        root = Path(".swarmnotes")
        (root / "gone.md").write_text(
            "---\nkind: decision\nsummary: s\nauthor: p\nupdated: 2026-10-02\n"
            "status: superseded\nsuperseded_by: missing\n---\n\nbody\n",
            encoding="utf-8",
        )
        (root / "nosup.md").write_text(
            "---\nkind: decision\nsummary: s\nauthor: p\nupdated: 2026-10-02\n"
            "status: superseded\n---\n\nbody\n",
            encoding="utf-8",
        )
        (root / "stray.md").write_text(
            "---\nkind: decision\nsummary: s\nauthor: p\nupdated: 2026-10-02\n"
            "superseded_by: _shared/report-format\n---\n\nbody\n",
            encoding="utf-8",
        )
        code, out, _ = run("check")
        self.assertEqual(code, 1)
        self.assertIn("gone: no such note: missing", out)
        self.assertIn("nosup: superseded but no superseded_by", out)
        self.assertIn("stray: superseded_by set but status is not superseded", out)

    def test_supersede_hop_limit(self):
        root = Path(".swarmnotes")
        last = examen.MAX_SUPERSEDE_HOPS
        for i in range(last + 1):
            meta = {"kind": "decision", "summary": "s", "author": "p", "updated": "2026-10-02"}
            if i < last:
                meta["status"] = "superseded"
                meta["superseded_by"] = f"h{i + 1}"
            examen.create(root, f"h{i}", meta, "body")
        code, out, _ = run("check")
        self.assertEqual(code, 1)
        self.assertIn("longer than", out)

    def test_swarmnotes_dir_env(self):
        other = Path(self.tmp.name) / "elsewhere"
        other.mkdir()
        os.environ["SWARMNOTES_DIR"] = str(other)
        self.new("x")
        self.assertTrue((other / "x.md").is_file())
        self.assertFalse((Path(".swarmnotes") / "x.md").exists())

    def test_swarmnotes_dir_missing(self):
        os.environ["SWARMNOTES_DIR"] = str(Path(self.tmp.name) / "nope")
        code, _, err = run("ls")
        self.assertEqual(code, 1)
        self.assertIn("not a directory", err)

    def test_root_found_from_subdirectory(self):
        self.new("x")
        sub = Path(self.tmp.name) / "deep" / "er"
        sub.mkdir(parents=True)
        os.chdir(sub)
        self.assertIn("x", run("ls")[1])

    def test_missing_scope(self):
        code, _, err = run("ls", "nope")
        self.assertEqual(code, 1)
        self.assertIn("no such scope", err)

    def test_prog_name_alias(self):
        self.assertEqual(examen.prog_name("swarmnotes"), "swarmnotes")
        self.assertEqual(examen.prog_name("/usr/local/bin/examen"), "examen")
        self.assertEqual(examen.prog_name("examen.py"), "examen")


class RepoTest(unittest.TestCase):
    def test_example_notes_pass_check(self):
        errors, warnings = examen.check(REPO / "examples" / "retry-swarm" / ".swarmnotes")
        self.assertEqual((errors, warnings), ([], []))

    def test_example_cli_follow_and_hide(self):
        old = os.getcwd()
        os.chdir(REPO / "examples" / "retry-swarm")
        os.environ.pop("SWARMNOTES_DIR", None)
        try:
            code, out, err = run("check")
            self.assertEqual(code, 0, out + err)
            self.assertIn("ok", out)
            code, out, err = run("get", "net/backoff-linear")
            self.assertEqual(code, 0, err)
            self.assertIn("full jitter", out)
            self.assertIn("superseded: net/backoff-linear -> net/retry-decision", out)
            self.assertNotIn("1s, 2s, 3s", out)
            _, listed, _ = run("ls", "net")
            self.assertNotIn("backoff-linear", listed)
            _, listed, _ = run("ls", "net", "--all")
            self.assertIn("backoff-linear", listed)
        finally:
            os.chdir(old)

    def test_skill_matches_agents_snippet(self):
        skill = (REPO / "skills" / "examen" / "SKILL.md").read_text(encoding="utf-8")
        snippet = (REPO / "AGENTS.snippet.md").read_text(encoding="utf-8")
        _, body = examen.parse(skill)
        self.assertEqual(body, snippet)
        for line in ("Task:", "Read first:", "Your scope:", "Done when:", "examen_get"):
            self.assertIn(line, snippet)
        brief = (REPO / "templates" / "brief.md").read_text(encoding="utf-8")
        for line in ("Task:", "Read first:", "Your scope:", "Done when:"):
            self.assertIn(line, brief)

    def test_readme_names_examen_once(self):
        readme = (REPO / "README.md").read_text(encoding="utf-8")
        former = "Swarm" + "Notes"
        self.assertTrue(readme.startswith("# Examen\n"))
        self.assertIn("`.swarmnotes/`", readme)
        self.assertIn("no third-party dependencies", readme)
        self.assertIn("examen[mcp]", readme)
        self.assertIn("examen-mcp", readme)
        self.assertEqual(readme.count(former), 1)
        hits = []
        for path in REPO.rglob("*"):
            if not path.is_file() or ".git" in path.parts or "__pycache__" in path.parts:
                continue
            if path.suffix not in {".md", ".py", ".toml"} and path.name != "LICENSE":
                continue
            if former in path.read_text(encoding="utf-8"):
                hits.append(path.relative_to(REPO).as_posix())
        self.assertEqual(hits, ["README.md"])

    def test_pyproject_core_stays_zero_dep(self):
        text = (REPO / "pyproject.toml").read_text(encoding="utf-8")
        self.assertIn('name = "examen"', text)
        self.assertIn('version = "0.3.0"', text)
        self.assertIn("dependencies = []", text)
        self.assertIn('examen = "examen:main"', text)
        self.assertIn('swarmnotes = "examen:main"', text)
        self.assertIn('examen-mcp = "examen_mcp:main"', text)
        self.assertIn("[project.optional-dependencies]", text)
        self.assertIn('mcp = ["mcp>=2.0,<3"]', text)
        self.assertEqual(examen.__version__, "0.3.0")
        self.assertTrue((REPO / "LICENSE").read_text(encoding="utf-8").startswith("MIT License\n"))
        changelog = (REPO / "CHANGELOG.md").read_text(encoding="utf-8")
        self.assertIn("## 0.3.0", changelog)
        self.assertIn("## 0.2.0", changelog)
        contributing = (REPO / "CONTRIBUTING.md").read_text(encoding="utf-8")
        self.assertIn("unittest", contributing)
        self.assertIn("SPEC.md", contributing)
        self.assertIn("standard library", contributing)

    def test_module_imports_stdlib_only(self):
        tree = ast.parse((REPO / "examen.py").read_text(encoding="utf-8"))
        mods = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                mods.update(alias.name.split(".")[0] for alias in node.names)
            elif isinstance(node, ast.ImportFrom) and node.module and node.level == 0:
                mods.add(node.module.split(".")[0])
        allowed = {"argparse", "os", "re", "sys", "datetime", "pathlib"}
        self.assertTrue(mods <= allowed, mods)


if __name__ == "__main__":
    unittest.main()

"""Tool-handler tests, plus an in-memory and stdio check when mcp is installed."""
import ast
import asyncio
import contextlib
import importlib.util
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO))

import examen  # noqa: E402
import examen_mcp  # noqa: E402

HAS_MCP = importlib.util.find_spec("mcp") is not None


class HandlerTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.old_cwd = os.getcwd()
        os.chdir(self.tmp.name)
        os.environ.pop("SWARMNOTES_DIR", None)
        examen_mcp.examen_init(".")

    def tearDown(self):
        os.chdir(self.old_cwd)
        os.environ.pop("SWARMNOTES_DIR", None)
        self.tmp.cleanup()

    def test_init_ls_and_check_share_the_cli_store(self):
        listed = examen_mcp.examen_ls()
        ids = [row["id"] for row in listed["notes"]]
        self.assertIn("_shared/report-format", ids)
        checked = examen_mcp.examen_check()
        self.assertEqual(checked["ok"], True)
        self.assertEqual(checked["errors"], [])
        code, out, err = _cli("get", "_shared/report-format")
        self.assertEqual(code, 0, err)
        self.assertIn("Status: done", out)

    def test_new_get_and_supersede_round_trip(self):
        created = examen_mcp.examen_new(
            "net/retry",
            "decision",
            "backoff",
            "use jitter",
            author="parent",
            tags=["net", "retry"],
        )
        self.assertEqual(created, {"id": "net/retry"})
        examen_mcp.examen_new("net/linear", "decision", "old", "1s then 2s", author="parent")
        examen_mcp.examen_supersede("net/linear", "net/retry")
        hidden = [row["id"] for row in examen_mcp.examen_ls("net")["notes"]]
        self.assertEqual(hidden, ["net/retry"])
        shown = examen_mcp.examen_ls("net", include_superseded=True)["notes"]
        linear = next(row for row in shown if row["id"] == "net/linear")
        self.assertEqual(linear["superseded_by"], "net/retry")
        fetched = examen_mcp.examen_get(["net/linear", "missing"])
        self.assertEqual(fetched["notes"][0]["id"], "net/retry")
        self.assertEqual(fetched["notes"][0]["chain"], ["net/linear", "net/retry"])
        self.assertIn("use jitter", fetched["notes"][0]["body"])
        self.assertEqual(fetched["errors"], [{"id": "missing", "error": "no such note: missing"}])
        # The CLI reads the file the tool just wrote.
        code, out, err = _cli("get", "net/linear")
        self.assertEqual(code, 0, err)
        self.assertIn("use jitter", out)
        self.assertNotIn("1s then 2s", out)

    def test_ls_filters_kind(self):
        examen_mcp.examen_new("a", "finding", "saw it", "detail", author="child")
        examen_mcp.examen_new("b", "decision", "chose", "why", author="parent")
        ids = [row["id"] for row in examen_mcp.examen_ls(kind="finding")["notes"]]
        self.assertEqual(ids, ["a"])

    def test_new_rejects_bad_input(self):
        with self.assertRaises(examen.NoteError) as raised:
            examen_mcp.examen_new("Bad", "decision", "s", "b")
        self.assertIn("invalid note id", str(raised.exception))
        with self.assertRaises(examen.NoteError) as raised:
            examen_mcp.examen_new("ok", "nope", "s", "b")
        self.assertIn("kind must be", str(raised.exception))

    def test_get_requires_an_id(self):
        with self.assertRaises(examen.NoteError):
            examen_mcp.examen_get([])

    def test_check_reports_an_error_without_hiding_warnings(self):
        (Path(".swarmnotes") / "nofm.md").write_text("just text\n", encoding="utf-8")
        examen_mcp.examen_new("long", "decision", "s", "word " * 301, author="p")
        report = examen_mcp.examen_check()
        self.assertFalse(report["ok"])
        self.assertTrue(any("nofm: missing frontmatter" in item for item in report["errors"]))
        self.assertTrue(any("long: body is 301 words" in item for item in report["warnings"]))

    def test_missing_store_is_an_error(self):
        os.environ["SWARMNOTES_DIR"] = str(Path(self.tmp.name) / "nope")
        with self.assertRaises(examen.NoteError) as raised:
            examen_mcp.examen_ls()
        self.assertIn("not a directory", str(raised.exception))

    def test_main_rejects_arguments_and_a_missing_sdk(self):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code = examen_mcp.main(["--help"])
        self.assertEqual(code, 2)
        self.assertIn("no arguments", err.getvalue())

        real = examen_mcp.build_server

        def missing():
            exc = ImportError("No module named 'mcp'")
            exc.name = "mcp"
            raise exc

        examen_mcp.build_server = missing
        try:
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                code = examen_mcp.main([])
        finally:
            examen_mcp.build_server = real
        self.assertEqual(code, 1)
        self.assertIn("examen[mcp]", err.getvalue())

        def unrelated():
            exc = ImportError("pydantic")
            exc.name = "pydantic"
            raise exc

        examen_mcp.build_server = unrelated
        try:
            with self.assertRaises(ImportError):
                examen_mcp.main([])
        finally:
            examen_mcp.build_server = real


class RepoMcpTest(unittest.TestCase):
    def test_sdk_import_is_lazy(self):
        tree = ast.parse((REPO / "examen_mcp.py").read_text(encoding="utf-8"))
        mods = set()
        for node in tree.body:
            if isinstance(node, ast.Import):
                mods.update(alias.name.split(".")[0] for alias in node.names)
            elif isinstance(node, ast.ImportFrom) and node.module and node.level == 0:
                mods.add(node.module.split(".")[0])
        self.assertTrue(mods <= {"__future__", "sys", "examen"}, mods)

    def test_example_handlers_follow_supersede(self):
        old = os.getcwd()
        os.chdir(REPO / "examples" / "retry-swarm")
        os.environ.pop("SWARMNOTES_DIR", None)
        try:
            report = examen_mcp.examen_check()
            self.assertTrue(report["ok"], report)
            fetched = examen_mcp.examen_get(["net/backoff-linear"])
            self.assertEqual(fetched["errors"], [])
            self.assertEqual(fetched["notes"][0]["id"], "net/retry-decision")
            self.assertIn("full jitter", fetched["notes"][0]["body"])
        finally:
            os.chdir(old)


@unittest.skipUnless(HAS_MCP, "mcp extra not installed")
class ProtocolTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.old_cwd = os.getcwd()
        os.chdir(self.tmp.name)
        os.environ.pop("SWARMNOTES_DIR", None)
        examen_mcp.examen_init(".")
        examen_mcp.examen_new("net/retry", "decision", "backoff", "use jitter", author="parent")

    def tearDown(self):
        os.chdir(self.old_cwd)
        os.environ.pop("SWARMNOTES_DIR", None)
        self.tmp.cleanup()

    def test_in_memory_client_round_trip(self):
        from mcp import Client

        server = examen_mcp.build_server()

        async def run():
            async with Client(server) as client:
                listed = await client.list_tools()
                names = sorted(tool.name for tool in listed.tools)
                got = await client.call_tool("examen_get", {"ids": ["net/retry"]})
                bad = await client.call_tool(
                    "examen_new",
                    {"id": "x", "kind": "nope", "summary": "s", "body": "b"},
                )
                return names, got, bad

        names, got, bad = asyncio.run(run())
        self.assertEqual(
            names,
            [
                "examen_check",
                "examen_get",
                "examen_init",
                "examen_ls",
                "examen_new",
                "examen_supersede",
            ],
        )
        payload = json.loads(got.content[0].text)
        self.assertFalse(got.is_error)
        self.assertEqual(payload["notes"][0]["summary"], "backoff")
        self.assertTrue(bad.is_error)
        self.assertIn("kind must be", bad.content[0].text)

    def test_stdio_server_lists_the_same_files(self):
        from mcp import Client, StdioServerParameters

        env = os.environ.copy()
        env["PYTHONPATH"] = str(REPO) + (
            os.pathsep + env["PYTHONPATH"] if env.get("PYTHONPATH") else ""
        )
        env["SWARMNOTES_DIR"] = str(Path(self.tmp.name) / ".swarmnotes")
        params = StdioServerParameters(
            command=sys.executable,
            args=["-m", "examen_mcp"],
            cwd=self.tmp.name,
            env=env,
        )

        async def run():
            async with Client(params, read_timeout_seconds=15) as client:
                result = await client.call_tool("examen_ls", {"scope": "net"})
                return result

        result = asyncio.run(asyncio.wait_for(run(), timeout=20))
        self.assertFalse(result.is_error)
        payload = json.loads(result.content[0].text)
        self.assertEqual([row["id"] for row in payload["notes"]], ["net/retry"])


def _cli(*argv):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = examen.main(list(argv))
    return code, out.getvalue(), err.getvalue()


if __name__ == "__main__":
    unittest.main()

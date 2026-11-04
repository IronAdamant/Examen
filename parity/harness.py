#!/usr/bin/env python3
"""Side-by-side parity: Examen 0.3.0 (python-legacy) against the Relatum binary.

The Python program name is forced to the same string the Rust binary prints
(`relatum` or `swarmnotes`). That is the rename. File bytes, exit codes, and
the remaining text are compared as-is. MCP tool names are compared after
rewriting relatum_ to examen_, and pydantic's versioned error URL is
normalised on both sides.
"""
from __future__ import annotations

import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PY_URL = re.compile(r"https://errors\.pydantic\.dev/\d+\.\d+/")


def legacy_ref() -> str:
    for ref in ("python-legacy", "origin/python-legacy"):
        probe = subprocess.run(
            ["git", "rev-parse", "--verify", ref],
            cwd=ROOT,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        if probe.returncode == 0:
            return ref
    raise SystemExit("python-legacy is not available. Fetch it with: git fetch origin python-legacy")


def extract_legacy(dest: Path) -> None:
    ref = legacy_ref()
    for name in ("examen.py", "examen_mcp.py"):
        text = subprocess.check_output(["git", "show", f"{ref}:{name}"], cwd=ROOT)
        (dest / name).write_bytes(text)


def load_examen(path: Path, prog: str):
    spec = importlib.util.spec_from_file_location("examen_legacy", path / "examen.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    mod.prog_name = lambda argv0=None, prog=prog: prog
    sys.modules["examen"] = mod
    return mod


def py_call(mod, args, cwd: Path, env: dict, stdin: str):
    import contextlib
    import io

    old_cwd = os.getcwd()
    old_env = {key: os.environ.get(key) for key in ("SWARMNOTES_DIR", "TZ")}
    out, err = io.StringIO(), io.StringIO()
    old_in = sys.stdin
    try:
        os.chdir(cwd)
        os.environ["TZ"] = "UTC"
        if hasattr(os, "tzset"):
            os.tzset()
        if "SWARMNOTES_DIR" in env:
            os.environ["SWARMNOTES_DIR"] = env["SWARMNOTES_DIR"]
        else:
            os.environ.pop("SWARMNOTES_DIR", None)
        sys.stdin = io.StringIO(stdin)
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            try:
                code = mod.main(list(args))
            except SystemExit as exc:
                code = exc.code if isinstance(exc.code, int) else 1
    finally:
        sys.stdin = old_in
        os.chdir(old_cwd)
        for key, value in old_env.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
    return code or 0, logical_newlines(out.getvalue()), logical_newlines(err.getvalue())


def rust_call(binary: Path, args, cwd: Path, env: dict, stdin: str):
    proc_env = os.environ.copy()
    proc_env["TZ"] = "UTC"
    proc_env.pop("SWARMNOTES_DIR", None)
    proc_env.update(env)
    proc = subprocess.run(
        [str(binary), *args],
        cwd=cwd,
        env=proc_env,
        input=stdin.encode(),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return proc.returncode, logical_newlines(proc.stdout.decode()), logical_newlines(proc.stderr.decode())


def snapshot(root: Path) -> dict[str, bytes]:
    found = {}
    if not root.is_dir():
        return found
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames.sort()
        for name in sorted(filenames):
            path = Path(dirpath) / name
            rel = path.relative_to(root).as_posix()
            found[rel] = path.read_bytes()
    return found


def absolute_tool(arg: str) -> Path:
    """Program path for subprocess.

    On POSIX, subprocess chdirs to the child's cwd before execve. A relative
    program path is therefore looked up inside the temp directory and the
    exec fails with ENOENT. Windows CreateProcess resolves the program
    against the parent's directory, which is why only Linux and macOS died
    on the first CLI call. The path is made absolute from this file's
    repository root before any chdir.
    """
    path = Path(arg)
    if not path.is_absolute():
        path = ROOT / path
    path = path.resolve()
    if not path.is_file():
        parent = path.parent
        entries = sorted(item.name for item in parent.iterdir()) if parent.is_dir() else []
        raise SystemExit(f"missing tool binary {path}; cwd={os.getcwd()}; entries={entries}")
    return path


def logical_newlines(text: str) -> str:
    """Fold Windows text-mode newlines.

    CPython opens stderr as text. On Windows that writes os.linesep, so each
    MCP log record is CRLF. The harness reads the pipe as bytes. A carriage
    return is not a different log line. Folding to \\n compares the line
    text; a missing or extra line still fails.
    """
    return text.replace("\r\n", "\n").replace("\r", "\n")


def scrub_temp_names(text: str, folders: list[Path]) -> str:
    """Replace only the unique final component of each side's temp directory.

    The two processes get different directories (py-* and rs-*). That name
    shows up in absolute paths, in JSON (where backslashes are escaped), and
    in Python repr. The final component is not escaped, so one replacement
    covers every spelling. The rest of the path is left in place: a different
    drive, slash, prefix, or a missing .swarmnotes segment still fails.
    """
    for folder in folders:
        name = folder.name
        if name:
            text = text.replace(name, "$DIR")
    return text


def norm_mcp(text: str) -> str:
    text = PY_URL.sub("https://errors.pydantic.dev/VER/", text)
    text = text.replace("relatum_", "examen_")
    text = text.replace("Relatum", "Examen")
    text = text.replace("relatum", "examen")
    text = text.replace("1.0.0", "0.3.0")
    return text


class Diffs:
    def __init__(self):
        self.items = []

    def add(self, title: str, left, right):
        self.items.append((title, left, right))
        print(f"DIFF {title}")
        print("--- python ---")
        print(left)
        print("--- rust ---")
        print(right)

    def ok(self) -> bool:
        return not self.items


def cli_suite(mod, binary: Path, diffs: Diffs):
    scenarios = [
        [
            ("init", []),
            ("ls", []),
            ("check", []),
            ("new", ["net/retry", "--kind", "decision", "--summary", "backoff", "--body", "use jitter", "--author", "parent", "--tags", "net, retry"]),
            ("get", ["net/retry"]),
            ("ls", ["net", "--kind", "decision", "--tag", "net"]),
            ("new", ["net/linear", "--kind", "decision", "--summary", "old", "--body", "1s then 2s"]),
            ("supersede", ["net/linear", "net/retry"]),
            ("ls", []),
            ("ls", ["--all"]),
            ("get", ["net/linear", "net/retry"]),
            ("check", []),
        ],
        [
            ("init", ["nested"]),
            ("init", ["nested"]),
            ("new", ["Bad/ID", "--kind", "decision", "--summary", "s", "--body", "b"]),
            ("new", ["x", "--kind", "nope", "--summary", "s", "--body", "b"]),
            ("new", ["e", "--kind", "decision", "--summary", "s", "--body", "  \n"]),
            ("new", ["piped", "--kind", "context", "--summary", "from stdin"], "piped body\n"),
            ("ls", ["missing-scope"]),
            ("get", ["missing", "piped"]),
            ("check", ["extra"]),
            ("new", []),
            ("ls", ["--kind"]),
            ("--help", []),
            ("new", ["-h"]),
        ],
    ]
    for index, steps in enumerate(scenarios):
        py_dir = Path(tempfile.mkdtemp(prefix="py-cli-"))
        rs_dir = Path(tempfile.mkdtemp(prefix="rs-cli-"))
        try:
            for step in steps:
                args, stdin = (step[1], step[2] if len(step) > 2 else "")
                # The first element may itself be flags, so the command is the whole argv.
                argv = [step[0], *args] if step[0] != "--help" else ["--help"]
                if step[0] == "--help":
                    argv = ["--help"]
                py = py_call(mod, argv, py_dir, {}, stdin)
                rs = rust_call(binary, argv, rs_dir, {}, stdin)
                label = f"cli[{index}] {argv}"
                if py[0] != rs[0] or py[1] != rs[1] or py[2] != rs[2]:
                    diffs.add(label, py, rs)
                    return
                py_files = snapshot(py_dir / ".swarmnotes")
                rs_files = snapshot(rs_dir / ".swarmnotes")
                # init nested writes under nested/.swarmnotes
                py_files.update({f"nested/{k}": v for k, v in snapshot(py_dir / "nested" / ".swarmnotes").items()})
                rs_files.update({f"nested/{k}": v for k, v in snapshot(rs_dir / "nested" / ".swarmnotes").items()})
                if py_files != rs_files:
                    diffs.add(label + " files", sorted(py_files), sorted(rs_files))
                    for key in sorted(set(py_files) | set(rs_files)):
                        if py_files.get(key) != rs_files.get(key):
                            diffs.add(label + " file " + key, py_files.get(key), rs_files.get(key))
                    return
        finally:
            shutil.rmtree(py_dir, ignore_errors=True)
            shutil.rmtree(rs_dir, ignore_errors=True)

    example = ROOT / "examples" / "retry-swarm"
    py = py_call(mod, ["check"], example, {}, "")
    rs = rust_call(binary, ["check"], example, {}, "")
    if py != rs:
        diffs.add("example check", py, rs)
    py = py_call(mod, ["get", "net/backoff-linear"], example, {}, "")
    rs = rust_call(binary, ["get", "net/backoff-linear"], example, {}, "")
    if py != rs:
        diffs.add("example get", py, rs)
    py = py_call(mod, ["ls", "net", "--all"], example, {}, "")
    rs = rust_call(binary, ["ls", "net", "--all"], example, {}, "")
    if py != rs:
        diffs.add("example ls", py, rs)


def mcp_round(command, cwd, env, calls):
    proc = subprocess.Popen(
        command,
        cwd=cwd,
        env=env,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    responses = []
    stderr_notes = []

    def send(message, expect=True):
        proc.stdin.write((json.dumps(message) + "\n").encode())
        proc.stdin.flush()
        if not expect:
            return None
        line = proc.stdout.readline()
        return json.loads(line.decode())

    send({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "parity", "version": "0"},
        },
    })
    send({"jsonrpc": "2.0", "method": "notifications/initialized"}, expect=False)
    listed = send({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    responses.append(("list", listed))
    next_id = 3
    for name, arguments in calls:
        responses.append((name, send({
            "jsonrpc": "2.0",
            "id": next_id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        })))
        next_id += 1
    proc.stdin.close()
    proc.wait(timeout=10)
    err = proc.stderr.read().decode()
    return responses, err


def mcp_suite(legacy: Path, binary_mcp: Path, diffs: Diffs):
    py_dir = Path(tempfile.mkdtemp(prefix="py-mcp-"))
    rs_dir = Path(tempfile.mkdtemp(prefix="rs-mcp-"))
    try:
        env_py = os.environ.copy()
        env_py["PYTHONPATH"] = str(legacy)
        env_py["TZ"] = "UTC"
        env_rs = os.environ.copy()
        env_rs["TZ"] = "UTC"
        calls_py = [
            ("examen_init", {"path": "."}),
            ("examen_ls", {}),
            ("examen_check", {}),
            ("examen_new", {"id": "net/retry", "kind": "decision", "summary": "backoff", "body": "use jitter", "author": "parent", "tags": ["net", "retry"]}),
            ("examen_new", {"id": "net/linear", "kind": "decision", "summary": "old", "body": "1s then 2s", "author": "parent"}),
            ("examen_supersede", {"old": "net/linear", "new": "net/retry"}),
            ("examen_ls", {"scope": "net", "include_superseded": True}),
            ("examen_get", {"ids": ["net/linear", "missing"]}),
            ("examen_new", {"id": "x", "kind": "nope", "summary": "s", "body": "b"}),
            ("examen_get", {"ids": []}),
            ("examen_new", {"id": "only"}),
            ("examen_init", {"path": "nested"}),
        ]
        calls_rs = [(name.replace("examen_", "relatum_"), args) for name, args in calls_py]
        py_resp, py_err = mcp_round([sys.executable, "-m", "examen_mcp"], py_dir, env_py, calls_py)
        rs_resp, rs_err = mcp_round([str(binary_mcp)], rs_dir, env_rs, calls_rs)
        folders = [py_dir, rs_dir]
        py_norm = scrub_temp_names(norm_mcp(json.dumps(py_resp, sort_keys=True)), folders)
        rs_norm = scrub_temp_names(norm_mcp(json.dumps(rs_resp, sort_keys=True)), folders)
        if py_norm != rs_norm:
            diffs.add("mcp responses", py_norm, rs_norm)
        py_err_n = scrub_temp_names(logical_newlines(norm_mcp(py_err)), folders)
        rs_err_n = scrub_temp_names(logical_newlines(norm_mcp(rs_err)), folders)
        if py_err_n != rs_err_n:
            diffs.add("mcp stderr", py_err_n, rs_err_n)
        if snapshot(py_dir / ".swarmnotes") != snapshot(rs_dir / ".swarmnotes"):
            diffs.add("mcp files", snapshot(py_dir / ".swarmnotes"), snapshot(rs_dir / ".swarmnotes"))
    finally:
        shutil.rmtree(py_dir, ignore_errors=True)
        shutil.rmtree(rs_dir, ignore_errors=True)


def extra_suite(mod, binary: Path, binary_mcp: Path, diffs: Diffs):
    py_dir = Path(tempfile.mkdtemp(prefix="py-extra-"))
    rs_dir = Path(tempfile.mkdtemp(prefix="rs-extra-"))
    try:
        steps = [
            ("init", []),
            ("new", ["a", "--kind", "decision", "--summary", "s", "--body", "body-a"]),
            ("new", ["b", "--kind", "decision", "--summary", "s", "--body", "body-b"]),
            ("new", ["c", "--kind", "decision", "--summary", "s", "--body", "body-c"]),
            ("supersede", ["b", "c"]),
            ("supersede", ["a", "b"]),
            ("get", ["a"]),
            ("supersede", ["c", "a"]),
            ("new", ["edge", "--kind", "decision", "--summary", "s", "--body", "word " * 300, "--author", "p"]),
            ("new", ["long", "--kind", "decision", "--summary", "s", "--body", "word " * 301, "--author", "p"]),
            ("check", []),
            ("ls", ["--all"]),
        ]
        for step in steps:
            argv = [step[0], *step[1]]
            py = py_call(mod, argv, py_dir, {}, "")
            rs = rust_call(binary, argv, rs_dir, {}, "")
            if py != rs or snapshot(py_dir / ".swarmnotes") != snapshot(rs_dir / ".swarmnotes"):
                diffs.add(f"extra {argv}", (py, "files-differ" if snapshot(py_dir / ".swarmnotes") != snapshot(rs_dir / ".swarmnotes") else "text"), (rs, snapshot(rs_dir / ".swarmnotes") if snapshot(py_dir / ".swarmnotes") != snapshot(rs_dir / ".swarmnotes") else ""))
                return
        # Hand-written notes, then check.
        for folder in (py_dir, rs_dir):
            root = folder / ".swarmnotes"
            (root / "empty.md").write_text("---\nkind: decision\nsummary: s\nauthor: p\nupdated: 2026-10-02\n---\n\n", encoding="utf-8")
            (root / "bad.md").write_text("---\nkind: decision\nsummary: s\nauthor: p\nstatus: stale\nupdated: 2026-13-40\n---\n\nbody\n", encoding="utf-8")
            (root / "open.md").write_text("---\nkind: decision\nsummary: s\n", encoding="utf-8")
            (root / "bin.md").write_bytes(b"\xff\xfe not utf-8")
            (root / "dup.md").write_text("---\nkind: decision\nkind: finding\nsummary: s\nauthor: p\nupdated: 2026-10-02\nowner: me\ntags: nope\n---\n\nbody\n", encoding="utf-8")
        py = py_call(mod, ["check"], py_dir, {}, "")
        rs = rust_call(binary, ["check"], rs_dir, {}, "")
        if py != rs:
            diffs.add("handwritten check", py, rs)
        other_py = py_dir / "elsewhere"
        other_rs = rs_dir / "elsewhere"
        other_py.mkdir()
        other_rs.mkdir()
        py = py_call(mod, ["new", "x", "--kind", "finding", "--summary", "s", "--body", "b"], py_dir, {"SWARMNOTES_DIR": str(other_py)}, "")
        rs = rust_call(binary, ["new", "x", "--kind", "finding", "--summary", "s", "--body", "b"], rs_dir, {"SWARMNOTES_DIR": str(other_rs)}, "")
        if py[0] != rs[0] or py[1] != rs[1] or py[2] != rs[2].replace("relatum", "examen"):
            # stderr carries the program name only when it fails. Success prints the id.
            if (py[0], py[1], py[2]) != (rs[0], rs[1], rs[2]):
                diffs.add("env new", py, rs)
        if snapshot(other_py) != snapshot(other_rs):
            diffs.add("env files", snapshot(other_py), snapshot(other_rs))
    finally:
        shutil.rmtree(py_dir, ignore_errors=True)
        shutil.rmtree(rs_dir, ignore_errors=True)

    # swarmnotes alias: same bytes as prog_name swarmnotes.
    swarm = binary.parent / "swarmnotes"
    mod_swarm = load_examen(binary.parent / "unused", "swarmnotes") if False else None
    legacy = Path(tempfile.mkdtemp(prefix="alias-"))
    extract_legacy(legacy)
    swarm_mod = load_examen(legacy, "swarmnotes")
    py_dir = Path(tempfile.mkdtemp(prefix="py-alias-"))
    rs_dir = Path(tempfile.mkdtemp(prefix="rs-alias-"))
    try:
        for argv in (["init"], ["ls"], ["-h"], ["check"]):
            py = py_call(swarm_mod, argv, py_dir, {}, "")
            rs = rust_call(swarm, argv, rs_dir, {}, "")
            if py != rs or snapshot(py_dir / ".swarmnotes") != snapshot(rs_dir / ".swarmnotes"):
                diffs.add(f"swarmnotes {argv}", py, rs)
                return
    finally:
        shutil.rmtree(py_dir, ignore_errors=True)
        shutil.rmtree(rs_dir, ignore_errors=True)
        shutil.rmtree(legacy, ignore_errors=True)

    mcp_init_matrix(binary_mcp, diffs)


def mcp_init_matrix(binary_mcp: Path, diffs: Diffs):
    """Compare examen_init path rules. Roots are rewritten to $DIR."""
    legacy = Path(tempfile.mkdtemp(prefix="init-legacy-"))
    extract_legacy(legacy)
    spec = importlib.util.spec_from_file_location("examen_mcp_legacy", legacy / "examen_mcp.py")
    # examen_mcp imports examen, which load_examen already registered only after exec.
    load_examen(legacy, "relatum")
    mcp_mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mcp_mod)
    cases = [
        ("cwd", ".", None),
        ("outside-store", None, "store"),
        ("project-of-store", None, "parent"),
        ("refuse-nested", "nested", None),
        ("blank", "   ", None),
    ]
    for name, path, mode in cases:
        py_dir = Path(tempfile.mkdtemp(prefix=f"py-{name}-"))
        rs_dir = Path(tempfile.mkdtemp(prefix=f"rs-{name}-"))
        try:
            old = os.getcwd()
            os.chdir(py_dir)
            os.environ.pop("SWARMNOTES_DIR", None)
            if mode == "store":
                os.environ["SWARMNOTES_DIR"] = str(py_dir.parent / f"{name}-store")
                path = os.environ["SWARMNOTES_DIR"]
            elif mode == "parent":
                store = py_dir.parent / f"{name}-store" / ".swarmnotes"
                os.environ["SWARMNOTES_DIR"] = str(store)
                path = str(store.parent)
            try:
                if path is None:
                    py_result = "skip"
                else:
                    try:
                        py_result = mcp_mod.examen_init(path)
                    except Exception as exc:
                        py_result = f"ERR {exc}"
            finally:
                os.chdir(old)
                os.environ.pop("SWARMNOTES_DIR", None)
            env = os.environ.copy()
            env["TZ"] = "UTC"
            env.pop("SWARMNOTES_DIR", None)
            rs_path = path
            if mode == "store":
                env["SWARMNOTES_DIR"] = str(rs_dir.parent / f"{name}-store")
                rs_path = env["SWARMNOTES_DIR"]
            elif mode == "parent":
                store = rs_dir.parent / f"{name}-store" / ".swarmnotes"
                env["SWARMNOTES_DIR"] = str(store)
                rs_path = str(store.parent)
            rs_result, _rs_err = mcp_one(binary_mcp, rs_dir, env, "relatum_init", {"path": rs_path})
            folders = [py_dir, rs_dir]
            if isinstance(py_result, dict):
                py_text = norm_mcp(json.dumps(py_result, sort_keys=True))
                inner = rs_result.get("result", {}).get("structuredContent", {}).get("result")
                rs_text = norm_mcp(json.dumps(inner, sort_keys=True))
            else:
                py_text = norm_mcp(str(py_result)).removeprefix("ERR ")
                rs_text = rs_result.get("result", {}).get("content", [{}])[0].get("text", "")
                if rs_text.startswith("Error executing tool "):
                    rs_text = rs_text.split(": ", 1)[-1]
                rs_text = norm_mcp(rs_text)
            py_text = scrub_temp_names(py_text, folders)
            rs_text = scrub_temp_names(rs_text, folders)
            if py_text != rs_text:
                diffs.add(f"init {name}", py_text, rs_text)
        finally:
            shutil.rmtree(py_dir, ignore_errors=True)
            shutil.rmtree(rs_dir, ignore_errors=True)
            os.environ.pop("SWARMNOTES_DIR", None)
    shutil.rmtree(legacy, ignore_errors=True)


def mcp_one(binary, cwd, env, name, arguments):
    proc = subprocess.Popen([str(binary)], cwd=cwd, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    def send(message, expect=True):
        proc.stdin.write((json.dumps(message) + "\n").encode())
        proc.stdin.flush()
        if expect:
            return json.loads(proc.stdout.readline().decode())
        return None
    send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "p", "version": "0"}}})
    send({"jsonrpc": "2.0", "method": "notifications/initialized"}, expect=False)
    result = send({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": name, "arguments": arguments}})
    proc.stdin.close()
    proc.wait(timeout=10)
    err = proc.stderr.read().decode()
    return result, err


def main():
    binary = absolute_tool(sys.argv[1] if len(sys.argv) > 1 else str(ROOT / "target" / "debug" / "relatum"))
    binary_mcp = absolute_tool(sys.argv[2] if len(sys.argv) > 2 else str(ROOT / "target" / "debug" / "relatum-mcp"))
    os.environ["TZ"] = "UTC"
    if hasattr(os, "tzset"):
        os.tzset()
    legacy = Path(tempfile.mkdtemp(prefix="examen-legacy-"))
    extract_legacy(legacy)
    mod = load_examen(legacy, "relatum")
    diffs = Diffs()
    cli_suite(mod, binary, diffs)
    extra_suite(mod, binary, binary_mcp, diffs)
    mcp_suite(legacy, binary_mcp, diffs)
    shutil.rmtree(legacy, ignore_errors=True)
    print(f"diffs {len(diffs.items)}")
    return 1 if diffs.items else 0


if __name__ == "__main__":
    sys.exit(main())

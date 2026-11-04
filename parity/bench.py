#!/usr/bin/env python3
"""Measure Examen 0.3.0 against the Relatum release binary on one machine.

Prints a markdown table of medians. Exits 1 if any Rust median is not lower.
"""
from __future__ import annotations

import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RUNS = {
    "startup": 11,
    "small": 9,
    "large": 7,
    "big": 5,
    "memory": 5,
}


def extract(dest: Path) -> Path:
    dest.mkdir(parents=True, exist_ok=True)
    ref = "python-legacy"
    if subprocess.run(["git", "rev-parse", "--verify", ref], cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode != 0:
        ref = "origin/python-legacy"
    text = subprocess.check_output(["git", "show", f"{ref}:examen.py"], cwd=ROOT)
    path = dest / "examen.py"
    path.write_bytes(text)
    mcp = subprocess.check_output(["git", "show", f"{ref}:examen_mcp.py"], cwd=ROOT)
    (dest / "examen_mcp.py").write_bytes(mcp)
    return path


def note(i: int) -> str:
    return (
        "---\n"
        "kind: decision\n"
        f"summary: bench note {i}\n"
        "author: bench\n"
        "updated: 2026-10-02\n"
        "tags: [bench]\n"
        "---\n\n"
        f"Body of note {i}. A short fact for the checker.\n"
    )


def make_store(root: Path, count: int) -> None:
    shared = root / "_shared"
    shared.mkdir(parents=True, exist_ok=True)
    (shared / "report-format.md").write_text(note(0), encoding="utf-8")
    for i in range(count):
        bucket = root / f"b{i // 100}"
        bucket.mkdir(exist_ok=True)
        (bucket / f"n{i}.md").write_text(note(i + 1), encoding="utf-8")


def median(samples: list[float]) -> float:
    return statistics.median(samples)


def time_call(command, cwd, env, runs: int) -> float:
    samples = []
    for i in range(runs):
        start = time.perf_counter()
        proc = subprocess.run(
            command, cwd=cwd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
        )
        elapsed = (time.perf_counter() - start) * 1000
        if proc.returncode not in (0, 1):
            raise SystemExit(f"{command} exited {proc.returncode}")
        if i == 0:
            continue
        samples.append(elapsed)
    return median(samples)


def mcp_round(command, cwd, env, tool: str) -> float:
    start = time.perf_counter()
    proc = subprocess.Popen(
        command, cwd=cwd, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL
    )
    def send(obj, expect=True):
        import json
        proc.stdin.write((json.dumps(obj) + "\n").encode())
        proc.stdin.flush()
        if expect:
            proc.stdout.readline()
    send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "b", "version": "0"}}})
    send({"jsonrpc": "2.0", "method": "notifications/initialized"}, expect=False)
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": tool, "arguments": {"scope": "b0"}}})
    proc.kill()
    proc.wait()
    return (time.perf_counter() - start) * 1000


def time_mcp(command, cwd, env, tool: str, runs: int) -> float:
    samples = []
    for i in range(runs):
        elapsed = mcp_round(command, cwd, env, tool)
        if i == 0:
            continue
        samples.append(elapsed)
    return median(samples)


def rss_kb(command, cwd, env) -> int:
    """Peak resident set (VmHWM) in kilobytes, sampled from /proc while it runs."""
    proc = subprocess.Popen(
        command, cwd=cwd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
    )
    peak = 0
    while True:
        try:
            text = Path(f"/proc/{proc.pid}/status").read_text()
        except FileNotFoundError:
            break
        for line in text.splitlines():
            if line.startswith("VmHWM:"):
                peak = max(peak, int(line.split()[1]))
        if proc.poll() is not None:
            break
    proc.wait()
    if peak == 0:
        raise SystemExit(f"no RSS sample for {command}")
    return peak


def main():
    rust = Path(sys.argv[1])
    rust_mcp = Path(sys.argv[2])
    work = Path(tempfile.mkdtemp(prefix="relatum-bench-"))
    legacy = extract(work / "legacy")
    py = [sys.executable, str(legacy)]
    env = os.environ.copy()
    env["TZ"] = "UTC"
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    rows = []

    def add(name, py_cmd, rs_cmd, cwd, runs, unit="ms"):
        py_v = time_call(py_cmd, cwd, env, runs)
        rs_v = time_call(rs_cmd, cwd, env, runs)
        rows.append((name, py_v, rs_v, unit))
        print(f"{name}: python {py_v:.1f} rust {rs_v:.1f} {unit}", flush=True)

    help_dir = work / "empty"
    help_dir.mkdir()
    add("cli startup (--help)", py + ["--help"], [str(rust), "--help"], help_dir, RUNS["startup"])

    small = work / "small"
    large = work / "large"
    big = work / "big"
    make_store(small / ".swarmnotes", 30)
    make_store(large / ".swarmnotes", 2000)
    make_store(big / ".swarmnotes", 8000)
    # A live target for supersede, not part of the counted chain.
    for base in (small, large):
        (base / ".swarmnotes" / "live.md").write_text(note(99999), encoding="utf-8")
        (base / ".swarmnotes" / "old.md").write_text(note(99998), encoding="utf-8")

    py_mcp = [sys.executable, "-m", "examen_mcp"]
    mcp_env = env.copy()
    mcp_env["PYTHONPATH"] = str(legacy.parent)
    mcp_env["SWARMNOTES_DIR"] = str(small / ".swarmnotes")
    rs_env = env.copy()
    rs_env["SWARMNOTES_DIR"] = str(small / ".swarmnotes")
    py_samples, rs_samples = [], []
    for i in range(RUNS["small"]):
        order = [
            (py_mcp, mcp_env, "examen_ls", py_samples),
            ([str(rust_mcp)], rs_env, "relatum_ls", rs_samples),
        ]
        if i % 2:
            order.reverse()
        for command, menv, tool, bucket in order:
            elapsed = mcp_round(command, small, menv, tool)
            if i:
                bucket.append(elapsed)
    py_m, rs_m = median(py_samples), median(rs_samples)
    rows.append(("mcp startup + ls round trip", py_m, rs_m, "ms"))
    print(f"mcp: python {py_m:.1f} rust {rs_m:.1f} ms", flush=True)

    def fresh(label, args, runs, scale):
        # init is measured in a new directory each run, so average the runs here.
        py_samples, rs_samples = [], []
        for i in range(runs):
            dpy = work / f"{label}-py-{i}"
            drs = work / f"{label}-rs-{i}"
            dpy.mkdir()
            drs.mkdir()
            if scale:
                make_store(dpy / ".swarmnotes", scale)
                make_store(drs / ".swarmnotes", scale)
            start = time.perf_counter()
            subprocess.run(py + args, cwd=dpy, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
            py_samples.append((time.perf_counter() - start) * 1000)
            start = time.perf_counter()
            subprocess.run([str(rust), *args], cwd=drs, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
            rs_samples.append((time.perf_counter() - start) * 1000)
        py_samples, rs_samples = py_samples[1:], rs_samples[1:]
        rows.append((label, median(py_samples), median(rs_samples), "ms"))
        print(f"{label}: python {rows[-1][1]:.1f} rust {rows[-1][2]:.1f} ms", flush=True)

    fresh("init empty", ["init"], RUNS["small"], 0)
    fresh("init on large store", ["init"], RUNS["small"], 2000)

    def both(label, args, cwd_py, cwd_rs, runs, mutate=None):
        py_samples, rs_samples = [], []
        for i in range(runs):
            order = [("py", py_samples, py + args, cwd_py), ("rs", rs_samples, [str(rust), *args], cwd_rs)]
            if i % 2:
                order.reverse()
            for _name, bucket, command, cwd in order:
                start = time.perf_counter()
                subprocess.run(command, cwd=cwd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
                if i:
                    bucket.append((time.perf_counter() - start) * 1000)
        rows.append((label, median(py_samples), median(rs_samples), "ms"))
        print(f"{label}: python {rows[-1][1]:.1f} rust {rows[-1][2]:.1f} ms", flush=True)

    # Separate copies so writes do not collide. new/supersede use unique ids per run via a wrapper.
    for label, count, folder in (("small", 30, small), ("large", 2000, large)):
        both(f"ls {label}", ["ls"], folder, folder, RUNS["small"] if label == "small" else RUNS["large"])
        both(f"get {label}", ["get", "b0/n0"], folder, folder, RUNS["small"] if label == "small" else RUNS["large"])
        both(f"check {label}", ["check"], folder, folder, RUNS["small"] if label == "small" else RUNS["large"])

    def mutating(label, kind, folder, runs):
        py_copy = work / f"{label}-py"
        rs_copy = work / f"{label}-rs"
        if py_copy.exists():
            shutil.rmtree(py_copy)
        if rs_copy.exists():
            shutil.rmtree(rs_copy)
        shutil.copytree(folder, py_copy)
        shutil.copytree(folder, rs_copy)
        py_samples, rs_samples = [], []
        for i in range(runs):
            if kind == "new":
                args = ["new", f"extra/e{i}", "--kind", "finding", "--summary", "s", "--body", "b", "--author", "bench"]
            else:
                args = ["supersede", f"b0/n{i}", "live"]
            order = [
                (py + args, py_copy, py_samples),
                ([str(rust), *args], rs_copy, rs_samples),
            ]
            if i % 2:
                order.reverse()
            for command, cwd, bucket in order:
                start = time.perf_counter()
                proc = subprocess.run(command, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                if proc.returncode != 0:
                    raise SystemExit(f"{label} {command} -> {proc.returncode} {proc.stderr.decode()}")
                if i:
                    bucket.append((time.perf_counter() - start) * 1000)
        py_samples, rs_samples = py_samples[1:], rs_samples[1:]
        rows.append((label, median(py_samples), median(rs_samples), "ms"))
        print(f"{label}: python {rows[-1][1]:.1f} rust {rows[-1][2]:.1f} ms", flush=True)

    mutating("new small", "new", small, RUNS["small"])
    mutating("new large", "new", large, RUNS["large"])
    mutating("supersede small", "sup", small, RUNS["small"])
    mutating("supersede large", "sup", large, RUNS["large"])
    both("check big", ["check"], big, big, RUNS["big"])

    py_rss = [rss_kb(py + ["check"], big, env) for _ in range(RUNS["memory"])]
    rs_rss = [rss_kb([str(rust), "check"], big, env) for _ in range(RUNS["memory"])]
    rows.append(("memory check big (RSS)", float(median(py_rss)), float(median(rs_rss)), "KB"))
    print(f"memory: python {rows[-1][1]:.0f} rust {rows[-1][2]:.0f} KB", flush=True)

    print("\n| Row | Python 0.3.0 | Relatum 1.0.1 |")
    print("|---|---:|---:|")
    slower = []
    for name, py_v, rs_v, unit in rows:
        if unit == "KB":
            print(f"| {name} | {py_v:.0f} {unit} | {rs_v:.0f} {unit} |")
        else:
            print(f"| {name} | {py_v:.1f} {unit} | {rs_v:.1f} {unit} |")
        if not rs_v < py_v:
            slower.append(name)
    if slower:
        print("NOT FASTER:", ", ".join(slower))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Stdio MCP server for Examen.

Same .swarmnotes/ files as the CLI. The official MCP Python SDK is imported
only when the server starts, so `import examen_mcp` works without the extra.
Install from GitHub (Python >= 3.10). The name examen on PyPI is unrelated:

    pip install "examen[mcp] @ git+https://github.com/IronAdamant/Examen"
"""
from __future__ import annotations

import os
import sys
from pathlib import Path

import examen
from examen import NoteError

GITHUB_INSTALL = "git+https://github.com/IronAdamant/Examen"
MCP_INSTALL = f'pip install "examen[mcp] @ {GITHUB_INSTALL}"'

INSTRUCTIONS = """\
Examen stores pass-by-reference notes as markdown files under .swarmnotes/.
Do not rename that directory. A note ID is its path without .md.

When you spawn subagents, write each shared fact once and send a brief:

Task: <what to do>
Read first: <note IDs>
Your scope: <directory under .swarmnotes/>
Done when: <checkable end condition>

Use these tools. They read and write the same files as the examen CLI.
examen_get follows superseded_by. examen_ls hides superseded notes unless
include_superseded is true. If the working directory is not the project,
the host must set SWARMNOTES_DIR to the .swarmnotes path.
examen_init creates a store only in the current working directory or at
SWARMNOTES_DIR.
"""


def _resolved(path):
    return Path(path).expanduser().resolve()


def _paths_match(left, right):
    return os.path.normcase(str(_resolved(left))) == os.path.normcase(str(_resolved(right)))


def _init_refusal(path):
    cwd = _resolved(Path.cwd())
    env = os.environ.get("SWARMNOTES_DIR", "").strip()
    if env:
        allowed = f"the current working directory ({cwd}) or SWARMNOTES_DIR ({env})"
    else:
        allowed = (
            f"the current working directory ({cwd}). "
            "Set SWARMNOTES_DIR to use another store"
        )
    return f"examen_init only creates a store in {allowed}. Refusing {path!r}."


def _store_for_init(path):
    """Return the store directory examen_init is allowed to create.

    path may be the working directory (store is `<cwd>/.swarmnotes`), the
    directory named by SWARMNOTES_DIR, or the project directory whose
    `.swarmnotes` child is exactly SWARMNOTES_DIR.
    """
    if not isinstance(path, str) or not path.strip():
        raise NoteError(_init_refusal(path))
    try:
        requested = _resolved(path)
    except (OSError, ValueError) as exc:
        raise NoteError(f"{_init_refusal(path)} ({exc})") from None

    cwd = _resolved(Path.cwd())
    env = os.environ.get("SWARMNOTES_DIR", "").strip()
    store = None
    if env:
        try:
            store = _resolved(env)
        except (OSError, ValueError) as exc:
            raise NoteError(f"SWARMNOTES_DIR={env} is not usable: {exc}") from None
        # The env var names the store itself. Honour that before the cwd rule
        # so a store that *is* the working directory is not nested.
        if _paths_match(requested, store):
            return store

    if _paths_match(requested, cwd):
        return cwd / examen.DIR_NAME

    if store is not None and _paths_match(requested / examen.DIR_NAME, store):
        return store

    raise NoteError(_init_refusal(path))


def examen_init(path: str = ".") -> dict:
    """Create the default store and its _shared/report-format note.

    Idempotent. path must be the current working directory, the directory in
    SWARMNOTES_DIR, or the project directory whose .swarmnotes is
    SWARMNOTES_DIR. Any other path is an error. Returns root (the store path)
    and report_format (its note id).
    """
    root = examen.ensure_store(_store_for_init(path))
    return {"root": str(root.resolve()), "report_format": "_shared/report-format"}


def examen_ls(
    scope: str = "",
    kind: str | None = None,
    include_superseded: bool = False,
    tag: str | None = None,
) -> dict:
    """List notes as id, kind, summary, and status.

    scope is a directory under the store; empty lists everything. kind filters
    to one of decision, constraint, convention, context, format, finding,
    attempt. Superseded notes are omitted unless include_superseded is true.
    tag filters to one slug. The store is .swarmnotes/ or $SWARMNOTES_DIR.
    """
    notes = examen.list_notes(
        examen.find_root(),
        scope=scope,
        kind=kind or None,
        tag=tag or None,
        include_superseded=include_superseded,
    )
    return {"notes": notes}


def _fetched(root, note_id: str) -> dict:
    final_id, meta, body, chain = examen.resolve(root, note_id)
    note = {
        "requested_id": note_id,
        "id": final_id,
        "kind": meta.get("kind", ""),
        "summary": meta.get("summary", ""),
        "status": meta.get("status", "active"),
        "body": body.rstrip("\n"),
        "chain": chain,
    }
    if meta.get("author"):
        note["author"] = meta["author"]
    if meta.get("tags"):
        note["tags"] = meta["tags"]
    return note


def examen_get(ids: list[str]) -> dict:
    """Fetch one or more notes by ID. Follows superseded_by, including multi-hop chains.

    Returns notes (the live note, with chain) and errors (ids that failed).
    Other ids are still returned when one id is missing.
    """
    if not ids:
        raise NoteError("examen_get needs at least one note id")
    root = examen.find_root()
    notes = []
    errors = []
    for note_id in ids:
        try:
            notes.append(_fetched(root, note_id))
        except NoteError as exc:
            errors.append({"id": note_id, "error": str(exc)})
    return {"notes": notes, "errors": errors}


def examen_new(
    id: str,
    kind: str,
    summary: str,
    body: str,
    author: str | None = None,
    tags: list[str] | None = None,
) -> dict:
    """Create a note. Fails if the id already exists; never overwrites.

    kind is decision, constraint, convention, context, format, finding, or
    attempt. summary is one line, at most 100 characters. body is the fact
    (keep it under ~300 words). tags is a list of slugs.
    """
    examen.add_note(examen.find_root(), id, kind, summary, body, author, tags)
    return {"id": id}


def examen_supersede(old: str, new: str) -> dict:
    """Mark old as replaced by new. new must already exist and resolve to a live note.

    Later examen_get calls on old follow the chain. examen_ls hides old unless
    include_superseded is true.
    """
    old_id, new_id = examen.supersede_note(examen.find_root(), old, new)
    return {"old": old_id, "new": new_id}


def examen_check() -> dict:
    """Lint every note in the store.

    ok is false when there are errors. warnings (long bodies, missing author
    or updated) do not fail the check. errors and warnings are the same
    strings the CLI prints.
    """
    errors, warnings = examen.check(examen.find_root())
    return {"ok": not errors, "errors": errors, "warnings": warnings}


def _missing_mcp(exc: ImportError) -> bool:
    name = exc.name or ""
    return name == "mcp" or name.startswith("mcp.")


def _bind(fn):
    """Register fn so NoteError becomes a tool error the model can read."""
    import functools
    import inspect

    from mcp.server.mcpserver.exceptions import ToolError

    @functools.wraps(fn)
    def wrapped(*args, **kwargs):
        try:
            return fn(*args, **kwargs)
        except NoteError as exc:
            raise ToolError(str(exc)) from None

    wrapped.__signature__ = inspect.signature(fn)
    return wrapped


def build_server():
    """Build the MCP server. Imports the SDK; raise ImportError if it is absent."""
    from mcp.server import MCPServer

    server = MCPServer(
        "examen",
        instructions=INSTRUCTIONS,
        version=examen.__version__,
    )
    for fn in (
        examen_init,
        examen_ls,
        examen_get,
        examen_new,
        examen_supersede,
        examen_check,
    ):
        server.tool()(_bind(fn))
    return server


def main(argv=None):
    """Serve MCP on stdio. Exit 1 if the optional extra is not installed."""
    args = list(sys.argv[1:] if argv is None else argv)
    if args:
        print(
            "examen-mcp takes no arguments. Set SWARMNOTES_DIR to choose a store.",
            file=sys.stderr,
        )
        return 2
    try:
        server = build_server()
    except ImportError as exc:
        if not _missing_mcp(exc):
            raise
        print(
            f"examen-mcp needs the optional extra: {MCP_INSTALL}",
            file=sys.stderr,
        )
        return 1
    server.run(transport="stdio")
    return 0


if __name__ == "__main__":
    sys.exit(main())

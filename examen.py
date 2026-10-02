#!/usr/bin/env python3
"""Examen: pass-by-reference notes for agent swarms (see SPEC.md).

Notes are plain markdown files under .swarmnotes/; this CLI only makes
listing, fetching and linting them cheaper. Stdlib only.

The storage directory stays .swarmnotes/ in v0.2 so existing dogfood paths
keep working. `swarmnotes` on PATH is an alias of this program.
"""
import argparse
import os
import re
import sys
from datetime import date
from pathlib import Path

DIR_NAME = ".swarmnotes"
SEGMENT = r"_?[a-z0-9][a-z0-9-]*"
ID_RE = re.compile(rf"^{SEGMENT}(/{SEGMENT})*$")
TAG_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")
DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
KINDS = ("decision", "constraint", "convention", "context", "format", "finding", "attempt")
STATUSES = ("active", "draft", "superseded")
KNOWN_KEYS = ("kind", "summary", "author", "updated", "tags", "status", "superseded_by")
SUMMARY_MAX = 100
WORD_BUDGET = 300
MAX_SUPERSEDE_HOPS = 32

DEFAULT_REPORT_FORMAT = """\
End your work with a report in this shape, and nothing longer:

Status: done | blocked | partial
Changed: files touched, one line each
Notes written: IDs of finding/attempt notes you created
Open questions: anything the parent must decide (or "none")

Put details in notes, not in the report. The parent fetches them only if needed.
"""


class NoteError(Exception):
    pass


def prog_name(argv0=None):
    """Console-script name used in help text and error prefixes."""
    base = Path(argv0 if argv0 is not None else sys.argv[0]).name
    stem = base.split(".", 1)[0].lower()
    if stem == "swarmnotes":
        return "swarmnotes"
    return "examen"


# --- files ---------------------------------------------------------------

def find_root(start=None):
    env = os.environ.get("SWARMNOTES_DIR")
    if env:
        root = Path(env)
        if not root.is_dir():
            raise NoteError(f"SWARMNOTES_DIR={env} is not a directory")
        return root
    here = Path(start or Path.cwd()).resolve()
    for d in (here, *here.parents):
        if (d / DIR_NAME).is_dir():
            return d / DIR_NAME
    raise NoteError(f"no {DIR_NAME}/ found (run `{prog_name()} init` or set SWARMNOTES_DIR)")


def note_path(root, note_id):
    if not isinstance(note_id, str) or not ID_RE.match(note_id):
        raise NoteError(f"invalid note id: {note_id!r}")
    return root / f"{note_id}.md"


def note_id_of(root, path):
    return path.relative_to(root).with_suffix("").as_posix()


def read_text(path):
    try:
        data = path.read_bytes()
    except OSError as e:
        raise NoteError(str(e)) from None
    try:
        text = data.decode("utf-8-sig")
    except UnicodeDecodeError:
        raise NoteError("not valid utf-8") from None
    return text.replace("\r\n", "\n").replace("\r", "\n")


def split_frontmatter(text):
    """Return the raw frontmatter block, None if missing, False if unclosed."""
    if not text.startswith("---\n"):
        return None
    end = text.find("\n---\n", 4)
    if end == -1:
        return False
    return text[4:end]


def parse(text):
    """Split a note into (meta, body). Frontmatter is flat `key: value` lines."""
    meta = {}
    block = split_frontmatter(text)
    if not isinstance(block, str):
        return meta, text
    for line in block.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        key, sep, value = line.partition(":")
        if not sep:
            continue
        value = value.strip()
        if value.startswith("[") and value.endswith("]"):
            value = [v.strip() for v in value[1:-1].split(",") if v.strip()]
        meta[key.strip()] = value
    end = text.find("\n---\n", 4)
    return meta, text[end + 5:].lstrip("\n")


def render(meta, body):
    lines = ["---"]
    for key, value in meta.items():
        if isinstance(value, list):
            value = "[" + ", ".join(value) + "]"
        lines.append(f"{key}: {value}")
    lines.append("---")
    return "\n".join(lines) + "\n\n" + body.rstrip("\n") + "\n"


def load(root, note_id):
    path = note_path(root, note_id)
    if not path.is_file():
        raise NoteError(f"no such note: {note_id}")
    return parse(read_text(path))


def iter_notes(root, scope=""):
    base = root / scope if scope else root
    if not base.is_dir():
        raise NoteError(f"no such scope: {scope}")
    for path in sorted(base.rglob("*.md")):
        yield note_id_of(root, path), path


def create(root, note_id, meta, body):
    path = note_path(root, note_id)
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        with open(path, "x", encoding="utf-8") as f:
            f.write(render(meta, body))
    except FileExistsError:
        raise NoteError(f"note already exists: {note_id} (pick another id)") from None
    return path


def valid_date(value):
    if not isinstance(value, str) or not DATE_RE.match(value):
        return False
    try:
        date.fromisoformat(value)
    except ValueError:
        return False
    return True


# --- commands ------------------------------------------------------------

def cmd_init(args):
    root = Path(args.path) / DIR_NAME
    (root / "_shared").mkdir(parents=True, exist_ok=True)
    meta = {
        "kind": "format",
        "summary": "Shape of the final report every agent returns.",
        "author": "examen",
        "updated": date.today().isoformat(),
    }
    try:
        create(root, "_shared/report-format", meta, DEFAULT_REPORT_FORMAT)
    except NoteError:
        pass  # already initialised
    print(root)


def _stdin_body():
    isatty = getattr(sys.stdin, "isatty", None)
    if isatty and isatty():
        return ""
    return sys.stdin.read()


def cmd_new(args):
    root = find_root()
    if args.kind not in KINDS:
        raise NoteError(f"kind must be one of: {', '.join(KINDS)}")
    summary = args.summary.strip()
    if not summary:
        raise NoteError("missing summary")
    if len(summary) > SUMMARY_MAX:
        raise NoteError(f"summary is {len(summary)} chars; max {SUMMARY_MAX}")
    body = args.body if args.body is not None else _stdin_body()
    if not body.strip():
        raise NoteError("empty body (pass --body or pipe text on stdin)")
    meta = {"kind": args.kind, "summary": summary}
    if args.author is not None:
        author = args.author.strip()
        if not author:
            raise NoteError("empty author")
        meta["author"] = author
    meta["updated"] = date.today().isoformat()
    if args.tags:
        tags = [t.strip() for t in args.tags.split(",") if t.strip()]
        bad = [t for t in tags if not TAG_RE.match(t)]
        if bad:
            raise NoteError(f"invalid tags: {', '.join(bad)}")
        meta["tags"] = tags
    create(root, args.id, meta, body)
    print(args.id)


def cmd_ls(args):
    root = find_root()
    if args.kind and args.kind not in KINDS:
        raise NoteError(f"kind must be one of: {', '.join(KINDS)}")
    if args.tag and not TAG_RE.match(args.tag):
        raise NoteError(f"invalid tag: {args.tag}")
    rows = []
    for note_id, path in iter_notes(root, args.scope.strip("/")):
        meta, _ = parse(read_text(path))
        status = meta.get("status", "active")
        if status == "superseded" and not args.all:
            continue
        if args.kind and meta.get("kind") != args.kind:
            continue
        tags = meta.get("tags") or []
        if args.tag and (not isinstance(tags, list) or args.tag not in tags):
            continue
        if status == "superseded":
            mark = f" [-> {meta.get('superseded_by', '?')}]"
        elif status == "draft":
            mark = " [draft]"
        else:
            mark = ""
        rows.append((note_id, meta.get("kind", "?"), meta.get("summary", "") + mark))
    width = max((len(r[0]) for r in rows), default=0)
    for note_id, kind, summary in rows:
        print(f"{note_id:<{width}}  {kind:<10}  {summary}")


def resolve(root, note_id):
    """Follow superseded_by links; return (final_id, meta, body, chain)."""
    chain = [note_id]
    meta, body = load(root, note_id)
    while meta.get("status") == "superseded":
        nxt = meta.get("superseded_by")
        if not nxt:
            if len(chain) == 1:
                raise NoteError("superseded but no superseded_by")
            raise NoteError(f"supersede chain ends at {chain[-1]} with no superseded_by")
        if not isinstance(nxt, str) or not ID_RE.match(nxt):
            raise NoteError(f"invalid superseded_by: {nxt!r}")
        if nxt in chain:
            raise NoteError(f"supersede cycle: {' -> '.join(chain + [nxt])}")
        if len(chain) >= MAX_SUPERSEDE_HOPS:
            raise NoteError(f"supersede chain longer than {MAX_SUPERSEDE_HOPS}")
        chain.append(nxt)
        meta, body = load(root, nxt)
    if meta.get("superseded_by"):
        raise NoteError("superseded_by set but status is not superseded")
    return chain[-1], meta, body, chain


def cmd_get(args):
    root = find_root()
    failed = False
    for i, note_id in enumerate(args.ids):
        try:
            final_id, meta, body, chain = resolve(root, note_id)
        except NoteError as e:
            print(f"{prog_name()}: {e}", file=sys.stderr)
            failed = True
            continue
        if i:
            print()
        print(f"## {final_id} ({meta.get('kind', '?')}) — {meta.get('summary', '')}")
        if len(chain) > 1:
            print(f"(superseded: {' -> '.join(chain)})")
        print(body.rstrip("\n"))
    return 1 if failed else 0


def cmd_supersede(args):
    root = find_root()
    if args.old == args.new:
        raise NoteError("a note cannot supersede itself")
    path = note_path(root, args.old)
    meta, body = load(root, args.old)
    _, _, _, chain = resolve(root, args.new)
    if args.old in chain:
        raise NoteError(f"supersede cycle: {' -> '.join([args.old, *chain])}")
    meta["status"] = "superseded"
    meta["superseded_by"] = args.new
    meta["updated"] = date.today().isoformat()
    path.write_text(render(meta, body), encoding="utf-8")
    print(f"{args.old} -> {args.new}")


def frontmatter_issues(text):
    block = split_frontmatter(text)
    if block is None:
        return ["missing frontmatter"]
    if block is False:
        return ["unclosed frontmatter"]
    issues = []
    seen = set()
    for line in block.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        key, sep, _value = line.partition(":")
        if not sep:
            issues.append(f"frontmatter line is not key: value ({line.strip()})")
            continue
        key = key.strip()
        if key in seen:
            issues.append(f"duplicate frontmatter key: {key}")
        seen.add(key)
        if key not in KNOWN_KEYS:
            issues.append(f"unknown frontmatter key: {key}")
    return issues


def tag_issues(tags):
    if not isinstance(tags, list):
        return ["tags must be an inline list like [a, b]"]
    bad = [str(t) for t in tags if not isinstance(t, str) or not TAG_RE.match(t)]
    if bad:
        return [f"invalid tags: {', '.join(bad)}"]
    return []


def check(root):
    """Return (errors, warnings) as lists of strings.

    Errors fail the command. Warnings (long bodies, missing optional
    author/updated) are printed and still exit 0 when nothing else is wrong.
    """
    errors, warnings = [], []
    for note_id, path in iter_notes(root):
        where = f"{note_id}:"
        if not ID_RE.match(note_id):
            errors.append(f"{where} invalid id (segments must match {SEGMENT})")
        try:
            text = read_text(path)
        except NoteError as e:
            errors.append(f"{where} {e}")
            continue
        structural = frontmatter_issues(text)
        if structural:
            errors.extend(f"{where} {issue}" for issue in structural)
            if any(issue.endswith("frontmatter") for issue in structural):
                continue
        meta, body = parse(text)
        if meta.get("kind") not in KINDS:
            errors.append(f"{where} kind must be one of: {', '.join(KINDS)}")
        summary = meta.get("summary", "")
        if not isinstance(summary, str) or not summary.strip():
            errors.append(f"{where} missing summary")
        elif len(summary) > SUMMARY_MAX:
            errors.append(f"{where} summary is {len(summary)} chars; max {SUMMARY_MAX}")
        status = meta.get("status", "active")
        if status not in STATUSES:
            errors.append(f"{where} status must be one of: {', '.join(STATUSES)}")
        elif status == "superseded" or "superseded_by" in meta:
            try:
                resolve(root, note_id)
            except NoteError as e:
                errors.append(f"{where} {e}")
        if "tags" in meta:
            errors.extend(f"{where} {issue}" for issue in tag_issues(meta["tags"]))
        if "author" not in meta:
            warnings.append(f"{where} missing author")
        elif not isinstance(meta["author"], str) or not meta["author"].strip():
            errors.append(f"{where} empty author")
        if "updated" not in meta:
            warnings.append(f"{where} missing updated")
        elif not valid_date(meta["updated"]):
            errors.append(f"{where} updated must be YYYY-MM-DD")
        words = len(body.split())
        if words == 0:
            errors.append(f"{where} empty body")
        elif words > WORD_BUDGET:
            warnings.append(
                f"{where} body is {words} words; consider splitting (budget {WORD_BUDGET})"
            )
    return errors, warnings


def cmd_check(args):
    errors, warnings = check(find_root())
    for w in warnings:
        print(f"warning: {w}")
    for e in errors:
        print(f"error: {e}")
    if not errors and not warnings:
        print("ok")
    return 1 if errors else 0


# --- entry point ---------------------------------------------------------

def build_parser():
    p = argparse.ArgumentParser(
        prog=prog_name(),
        description="Pass-by-reference notes for agent swarms.",
    )
    sub = p.add_subparsers(dest="cmd", required=True)

    s = sub.add_parser("init", help=f"create {DIR_NAME}/ with a default report format")
    s.add_argument("path", nargs="?", default=".")
    s.set_defaults(func=cmd_init)

    s = sub.add_parser("new", help="create a note (body from --body or stdin)")
    s.add_argument("id")
    s.add_argument("--kind", required=True, help="|".join(KINDS))
    s.add_argument("--summary", required=True)
    s.add_argument("--author")
    s.add_argument("--tags", help="comma-separated")
    s.add_argument("--body")
    s.set_defaults(func=cmd_new)

    s = sub.add_parser("ls", help="list notes: id, kind, summary")
    s.add_argument("scope", nargs="?", default="")
    s.add_argument("--kind")
    s.add_argument("--tag")
    s.add_argument("--all", action="store_true", help="include superseded notes")
    s.set_defaults(func=cmd_ls)

    s = sub.add_parser("get", help="print note bodies, following superseded_by")
    s.add_argument("ids", nargs="+")
    s.set_defaults(func=cmd_get)

    s = sub.add_parser("supersede", help="mark OLD as replaced by NEW")
    s.add_argument("old")
    s.add_argument("new")
    s.set_defaults(func=cmd_supersede)

    s = sub.add_parser("check", help="lint every note")
    s.set_defaults(func=cmd_check)
    return p


def main(argv=None):
    args = build_parser().parse_args(argv)
    try:
        return args.func(args) or 0
    except NoteError as e:
        print(f"{prog_name()}: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

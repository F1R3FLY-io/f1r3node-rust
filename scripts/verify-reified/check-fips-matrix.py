#!/usr/bin/env python3
"""Check the FIPS conformance matrix of Reified RSpaces (P10 step S4, design v2 section 2.7).

The matrix docs/rspace/reified-rspaces/fips-conformance-matrix.json binds each normative
clause of two FIPS documents to its evidence: the Reifying RSpaces FIPS, and the agent
section (lines 8 to 57) of the Agents FIPS. The Markdown file beside it is the rendering of
the JSON, so the two cannot drift.

The checker makes these checks:

1. Schema. The top-level keys, the FIPS entries, the spec holes and the rows have their
   fields, and each row has one of the five status values.
2. FIPS files. Each FIPS file exists under the FIPS root and has its pinned SHA-256 digest.
3. Coverage. Every non-blank line in the normative ranges of a FIPS lies in the lines of a
   row of that FIPS. No row names a line outside its file.
4. Rows by status. An implemented row has code, tests and docs, and a formal cell or a
   no_model reason. A deviation, future or out-of-scope row cites a decision. An
   informative row states its reason.
5. Cells. A code citation names one Rust source, and each range starts and ends on a
   non-blank line (rust_citations.py, the rule rust-cite of doc_lint.py). A Rocq name is
   declared at its cited line (the rule rocq-cite of doc_lint.py). A TLA+ invariant is
   defined in its module. A test is a test function that is not ignored (scan_tests of
   check-consensus-checklist.py), or a test that the macro example_tests! generates. A
   docs link names an existing file and an anchor of it (the rule link of doc_lint.py).
   A decision is an anchor of decision-register.md.
6. Rendering. The Markdown file equals the rendering of the JSON. With --write, the
   checker writes the rendering instead of comparing it.

Usage: check-fips-matrix.py [--repo-root DIR] [--fips-root DIR] [--matrix FILE] [--write]

The FIPS root defaults to the directory FIPS beside the repository. The verifier sandbox
mounts that repository as an input root and passes --fips-root.

Exit codes: 0 every check passes, 1 a check fails, 2 a usage or input error.
Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_ROOT = HERE.parents[1]
MATRIX_PATH = Path("docs/rspace/reified-rspaces/fips-conformance-matrix.json")
REGISTER_PATH = Path("docs/rspace/reified-rspaces/decision-register.md")
THEORIES_PATH = Path("formal/rocq/reified_rspaces/theories")
SCHEMA = "reified-rspaces-fips-matrix/1"
STATUSES = ("implemented", "deviation", "future", "out-of-scope", "informative")
DECISION_STATUSES = ("deviation", "future", "out-of-scope")
TOP_KEYS = ("schema", "title", "checker", "rendering", "fips", "spec_holes", "rows")
FIPS_KEYS = ("key", "title", "date", "path", "sha256", "normative", "outside")
HOLE_KEYS = ("id", "hole", "fips", "lines", "resolution", "rows", "decision")
ROW_KEYS = ("id", "fips", "lines", "clause", "status", "code", "formal", "no_model", "tests",
            "docs", "decision", "reason", "note")
ROW_ID = re.compile(r"^[A-Z]{2}-[0-9]{2}$")
HOLE_ID = re.compile(r"^SH-[0-9]$")
DECISION_ID = re.compile(r"^(?:decision-[0-9]+|[im]-[0-9]+)$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
TEST_REF = re.compile(r"^(?P<file>[A-Za-z0-9_+./-]+\.rs)::(?P<name>[a-z_][a-z0-9_]*)$")
MACRO_TEST = re.compile(r"^\s+(?P<name>example_[a-z0-9_]+)\s*=>\s*[A-Z0-9_]+,\s*$", re.M)
TLA_DEFINITION = r"^{name}\s*(?:\([^)]*\))?\s*=="
REGISTER_LINK = "decision-register.md"
EXIT_PASS, EXIT_FAIL, EXIT_USAGE = 0, 1, 2


class InputError(Exception):
    """The matrix file cannot be read or does not have the schema."""


def load_by_path(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


DOC_LINT = load_by_path("doc_lint", HERE / "doc_lint.py")
RUST_CITATIONS = load_by_path("rust_citations", HERE / "rust_citations.py")
CHECKLIST = load_by_path("check_consensus_checklist", HERE / "check-consensus-checklist.py")


@dataclass
class Report:
    lines: list[str] = field(default_factory=list)
    failures: int = 0
    checks: int = 0

    def check(self, ok: bool, message: str) -> None:
        self.checks += 1
        if not ok:
            self.failures += 1
            self.lines.append(f"FAIL {message}")

    def fail(self, message: str) -> None:
        self.check(False, message)


# ---------------------------------------------------------------------------
# Loading and schema
# ---------------------------------------------------------------------------


def load_matrix(path: Path) -> dict:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise InputError(f"{path} is missing") from error
    except json.JSONDecodeError as error:
        raise InputError(f"{path} is not valid JSON: {error}") from error
    if not isinstance(data, dict):
        raise InputError(f"{path} is not a JSON object")
    return data


def ranges_ok(value) -> bool:
    return (isinstance(value, list) and bool(value) and all(
        isinstance(r, list) and len(r) == 2 and all(isinstance(n, int) for n in r) and 1 <= r[0] <= r[1]
        for r in value))


def string_list(value) -> bool:
    return isinstance(value, list) and all(isinstance(item, str) and item for item in value)


def check_schema(data: dict, report: Report) -> None:
    report.check(tuple(data.keys()) == TOP_KEYS, f"the top-level keys are {TOP_KEYS}, not {tuple(data.keys())}")
    report.check(data.get("schema") == SCHEMA, f"the schema is {SCHEMA!r}")
    keys = set()
    for entry in data.get("fips", []):
        ok = isinstance(entry, dict) and tuple(entry.keys()) == FIPS_KEYS
        report.check(ok, f"a FIPS entry has the keys {FIPS_KEYS}")
        if not ok:
            continue
        keys.add(entry["key"])
        report.check(bool(SHA256.match(entry["sha256"])), f"FIPS {entry['key']}: a well-formed SHA-256 digest")
        report.check(ranges_ok(entry["normative"]), f"FIPS {entry['key']}: well-formed normative ranges")
    ids = set()
    for row in data.get("rows", []):
        ok = isinstance(row, dict) and tuple(row.keys()) == ROW_KEYS
        report.check(ok, f"row {row.get('id') if isinstance(row, dict) else row!r}: the keys {ROW_KEYS}")
        if not ok:
            continue
        rid = row["id"]
        report.check(bool(ROW_ID.match(rid)) and rid not in ids, f"row {rid}: a unique id of the form XX-00")
        ids.add(rid)
        report.check(row["fips"] in keys and rid.startswith(row["fips"] + "-"),
                     f"row {rid}: its FIPS key {row['fips']!r} is a FIPS entry and its id prefix")
        report.check(ranges_ok(row["lines"]), f"row {rid}: well-formed line ranges")
        report.check(row["status"] in STATUSES, f"row {rid}: the status {row['status']!r} is one of {STATUSES}")
        report.check(isinstance(row["clause"], str) and bool(row["clause"].strip()), f"row {rid}: a clause")
        for key in ("code", "tests", "docs", "decision"):
            report.check(string_list(row[key]), f"row {rid}: {key} is a list of strings")
        report.check(isinstance(row["formal"], list) and all(isinstance(f, dict) for f in row["formal"]),
                     f"row {rid}: formal is a list of objects")
        for key in ("reason", "note"):
            report.check(row[key] is None or (isinstance(row[key], str) and bool(row[key].strip())),
                         f"row {rid}: {key} is null or a non-empty string")
        nm = row["no_model"]
        report.check(nm is None or (isinstance(nm, dict) and tuple(nm.keys()) == ("reason", "link")
                                    and all(isinstance(v, str) and v.strip() for v in nm.values())),
                     f"row {rid}: no_model is null or an object with a reason and a link")
    for hole in data.get("spec_holes", []):
        ok = isinstance(hole, dict) and tuple(hole.keys()) == HOLE_KEYS
        report.check(ok, f"a spec hole has the keys {HOLE_KEYS}")
        if not ok:
            continue
        report.check(bool(HOLE_ID.match(hole["id"])), f"spec hole {hole['id']}: an id of the form SH-0")
        report.check(hole["fips"] in keys and ranges_ok(hole["lines"]), f"spec hole {hole['id']}: FIPS lines")
        report.check(string_list(hole["rows"]) and all(r in ids for r in hole["rows"]),
                     f"spec hole {hole['id']}: its rows exist")
        report.check(string_list(hole["decision"]), f"spec hole {hole['id']}: decisions")


# ---------------------------------------------------------------------------
# FIPS files and coverage
# ---------------------------------------------------------------------------


def check_fips(data: dict, fips_root: Path, report: Report) -> dict[str, list[str]]:
    """Check each FIPS file and return its lines by key (an empty list when it is missing)."""
    texts: dict[str, list[str]] = {}
    for entry in data["fips"]:
        path = fips_root / entry["path"]
        try:
            raw = path.read_bytes()
        except OSError:
            report.fail(f"FIPS {entry['key']}: {path} is missing; pass --fips-root")
            texts[entry["key"]] = []
            continue
        digest = hashlib.sha256(raw).hexdigest()
        report.check(digest == entry["sha256"],
                     f"FIPS {entry['key']}: {entry['path']} has the SHA-256 {digest}, not the pinned {entry['sha256']}")
        texts[entry["key"]] = raw.decode("utf-8").split("\n")
    return texts


def covered_lines(ranges: list[list[int]]) -> set[int]:
    return {n for first, last in ranges for n in range(first, last + 1)}


def check_coverage(data: dict, texts: dict[str, list[str]], report: Report) -> None:
    for entry in data["fips"]:
        lines = texts.get(entry["key"], [])
        if not lines:
            continue
        count = len(lines) - (1 if lines and lines[-1] == "" else 0)
        rows = [row for row in data["rows"] if row["fips"] == entry["key"]]
        covered: set[int] = set()
        for row in rows:
            lines_of_row = covered_lines(row["lines"])
            report.check(max(lines_of_row) <= count, f"row {row['id']}: its lines lie inside {entry['path']} ({count} lines)")
            covered |= lines_of_row
        normative = {n for n in covered_lines(entry["normative"]) if n <= count and lines[n - 1].strip()}
        missing = sorted(normative - covered)
        report.check(not missing, f"FIPS {entry['key']}: the normative lines {compress(missing)} have no row")


def compress(numbers: list[int]) -> str:
    parts: list[tuple[int, int]] = []
    for n in numbers:
        if parts and parts[-1][1] == n - 1:
            parts[-1] = (parts[-1][0], n)
        else:
            parts.append((n, n))
    return ", ".join(f"{a}" if a == b else f"{a}-{b}" for a, b in parts)


# ---------------------------------------------------------------------------
# Cells
# ---------------------------------------------------------------------------


class Resolver:
    """Resolves the cells of the rows against one repository. Each file loads once."""

    def __init__(self, root: Path) -> None:
        self.root = root
        self.citations = RUST_CITATIONS.Citations(root)
        self.declarations: dict[Path, dict[str, list[int]] | None] = {}
        self.scans: dict[Path, tuple[dict[str, bool], set[str]] | None] = {}
        self.macro_tests: dict[Path, set[str]] = {}
        self.anchors: dict[Path, set[str] | None] = {}
        self.tla: dict[Path, str | None] = {}

    def code(self, cite: str) -> list[str]:
        count, problems = self.citations.check_offsets(f"`{cite}`")
        if count == 0 and not problems:
            return [f"{cite} is not a Rust citation path.rs:N or path.rs:N-M"]
        return [message for _, message in problems]

    def rocq(self, item: dict) -> list[str]:
        if tuple(item.keys()) != ("rocq", "name", "line"):
            return [f"{item} has not the keys rocq, name, line"]
        file, name, line = item["rocq"], item["name"], item["line"]
        path = self.root / file if "/" in file else self.root / THEORIES_PATH / file
        if path not in self.declarations:
            self.declarations[path] = DOC_LINT.rocq_declarations(path)
        decls = self.declarations[path]
        if decls is None:
            return [f"{file} is missing"]
        if name not in decls:
            return [f"{name} is not declared in {file}"]
        if line not in decls[name]:
            return [f"{name} is declared at {file}:{', '.join(map(str, decls[name]))}, not at line {line}"]
        return []

    def tla_invariant(self, item: dict) -> list[str]:
        if tuple(item.keys()) != ("tla", "invariant"):
            return [f"{item} has not the keys tla, invariant"]
        path = self.root / item["tla"]
        if path not in self.tla:
            try:
                self.tla[path] = path.read_text(encoding="utf-8")
            except OSError:
                self.tla[path] = None
        text = self.tla[path]
        if text is None:
            return [f"{item['tla']} is missing"]
        if not re.search(TLA_DEFINITION.format(name=re.escape(item["invariant"])), text, re.M):
            return [f"{item['invariant']} is not defined in {item['tla']}"]
        return []

    def formal(self, item: dict) -> list[str]:
        if "rocq" in item:
            return self.rocq(item)
        if "tla" in item:
            return self.tla_invariant(item)
        return [f"{item} is neither a Rocq nor a TLA+ item"]

    def test(self, ref: str) -> list[str]:
        m = TEST_REF.match(ref)
        if not m:
            return [f"{ref} is not of the form path.rs::test_name"]
        path = self.root / m.group("file")
        if not path.is_file():
            return [f"{m.group('file')} is missing"]
        if path not in self.scans:
            text = path.read_text(encoding="utf-8")
            self.scans[path] = CHECKLIST.scan_tests(text)
            self.macro_tests[path] = ({x.group("name") for x in MACRO_TEST.finditer(text)}
                                      if "example_tests!" in text else set())
        scan, name = self.scans[path], m.group("name")
        if scan is None:
            return [f"{m.group('file')}: a block comment has no end"]
        tests, functions = scan
        if name in tests:
            return [f"{ref} is an ignored test"] if tests[name] else []
        if name in self.macro_tests[path]:
            return []
        if name in functions:
            return [f"{ref} is a function without a test attribute"]
        return [f"{ref}: no such test"]

    def anchors_of(self, path: Path) -> set[str] | None:
        if path not in self.anchors:
            try:
                text = path.read_text(encoding="utf-8")
            except OSError:
                self.anchors[path] = None
            else:
                self.anchors[path] = DOC_LINT.anchors_of(DOC_LINT.parse_markdown(text))
        return self.anchors[path]

    def doc(self, link: str) -> list[str]:
        target, _, fragment = link.partition("#")
        path = self.root / target
        if not path.is_file():
            return [f"{link}: {target} is missing"]
        if not fragment:
            return []
        if path.suffix != ".md":
            return [f"{link}: only a Markdown file takes a fragment"]
        anchors = self.anchors_of(path)
        return [] if anchors is not None and fragment in anchors else [f"{link}: no anchor #{fragment}"]

    def decision(self, anchor: str) -> list[str]:
        if not DECISION_ID.match(anchor):
            return [f"{anchor} is not a decision anchor (decision-N, i-N or m-N)"]
        anchors = self.anchors_of(self.root / REGISTER_PATH)
        return [] if anchors is not None and anchor in anchors else [f"{anchor} is not an anchor of {REGISTER_PATH}"]


def check_rows(data: dict, resolver: Resolver, report: Report) -> None:
    for row in data["rows"]:
        rid, status = row["id"], row["status"]
        if status == "implemented":
            report.check(bool(row["code"]), f"row {rid}: an implemented row needs code")
            report.check(bool(row["tests"]), f"row {rid}: an implemented row needs tests")
            report.check(bool(row["docs"]), f"row {rid}: an implemented row needs docs")
            report.check(bool(row["formal"]) != (row["no_model"] is not None),
                         f"row {rid}: an implemented row needs a formal cell or a no_model reason, not both")
        if status in DECISION_STATUSES:
            report.check(bool(row["decision"]), f"row {rid}: a {status} row cites a decision")
        if status == "informative":
            report.check(row["reason"] is not None, f"row {rid}: an informative row states its reason")
        for cite in row["code"]:
            for problem in resolver.code(cite):
                report.fail(f"row {rid}: code: {problem}")
        for item in row["formal"]:
            for problem in resolver.formal(item):
                report.fail(f"row {rid}: formal: {problem}")
        if row["no_model"] is not None:
            for problem in resolver.doc(row["no_model"]["link"]):
                report.fail(f"row {rid}: no_model: {problem}")
        for ref in row["tests"]:
            for problem in resolver.test(ref):
                report.fail(f"row {rid}: tests: {problem}")
        for link in row["docs"]:
            for problem in resolver.doc(link):
                report.fail(f"row {rid}: docs: {problem}")
        for anchor in row["decision"]:
            for problem in resolver.decision(anchor):
                report.fail(f"row {rid}: decision: {problem}")
    for hole in data["spec_holes"]:
        for anchor in hole["decision"]:
            for problem in resolver.decision(anchor):
                report.fail(f"spec hole {hole['id']}: decision: {problem}")


# ---------------------------------------------------------------------------
# Rendering
# ---------------------------------------------------------------------------

DOC_DIR = Path("docs/rspace/reified-rspaces")
STATUS_MEANING = {
    "implemented": "The code, its tests, its documents, and a formal model or a stated reason for none give the clause.",
    "deviation": "The campaign differs from the clause, and a recorded decision states how and why.",
    "future": "A later phase gives the clause, and a recorded decision names the phase.",
    "out-of-scope": "Another owner gives the clause, and a recorded decision states the owner.",
    "informative": "The text states background, motivation or an open question, and requires no behavior.",
}


def rel_link(target: str) -> str:
    """Return the link from the rendering in DOC_DIR to the repository path `target`."""
    path, _, fragment = target.partition("#")
    parts = Path(path).parts
    doc = DOC_DIR.parts
    common = 0
    while common < min(len(parts), len(doc)) and parts[common] == doc[common]:
        common += 1
    link = "/".join([".."] * (len(doc) - common) + list(parts[common:]))
    return link + (f"#{fragment}" if fragment else "")


def lines_text(ranges: list[list[int]]) -> str:
    return ", ".join(f"{a}" if a == b else f"{a}–{b}" for a, b in ranges)


def decision_links(anchors: list[str]) -> str:
    names = []
    for anchor in anchors:
        kind, _, number = anchor.rpartition("-")
        label = f"decision {number}" if kind == "decision" else f"{kind.upper()}-{number}"
        names.append(f"[{label}]({REGISTER_LINK}#{anchor})")
    return ", ".join(names)


def cell(text: str) -> str:
    return text.replace("|", "\\|")


def doc_label(target: str) -> str:
    path, _, fragment = target.partition("#")
    name = Path(path).name
    return f"{name}#{fragment}" if fragment else name


def render_fields(row: dict) -> list[str]:
    """Return the field table of one row. A table keeps the long evidence lists out of the prose."""
    fields: list[tuple[str, str]] = [
        ("Lines", lines_text(row["lines"])),
        ("Status", f"`{row['status']}`"),
        ("Clause", cell(row["clause"])),
    ]
    if row["code"]:
        fields.append(("Code", ", ".join(f"`{c}`" for c in row["code"])))
    if row["formal"]:
        items = []
        for item in row["formal"]:
            if "rocq" in item:
                items.append(f"`{item['name']}` (`{item['rocq']}:{item['line']}`)")
            else:
                items.append(f"the TLA+ invariant `{item['invariant']}` of `{Path(item['tla']).name}`")
        fields.append(("Formal", ", ".join(items)))
    if row["no_model"] is not None:
        fields.append(("No model", f"{cell(row['no_model']['reason'])} ([reason]({rel_link(row['no_model']['link'])}))"))
    if row["tests"]:
        fields.append(("Tests", ", ".join(
            f"`{ref.split('::')[1]}` in `{ref.split('::')[0]}`" for ref in row["tests"])))
    if row["docs"]:
        fields.append(("Documents", ", ".join(f"[{doc_label(d)}]({rel_link(d)})" for d in row["docs"])))
    if row["decision"]:
        fields.append(("Decisions", decision_links(row["decision"])))
    if row["reason"] is not None:
        fields.append(("Reason", cell(row["reason"])))
    if row["note"] is not None:
        fields.append(("Note", cell(row["note"])))
    return ["| Field | Value |", "|---|---|"] + [f"| {name} | {value} |" for name, value in fields]


def render(data: dict) -> str:
    out: list[str] = [
        "# Reified RSpaces FIPS conformance matrix",
        "",
        "This document is generated. Do not edit it.",
        f"`{MATRIX_PATH.name}` is the source, and `scripts/verify-reified/check-fips-matrix.py --write` renders it.",
        "The checker also proves that every normative line of each FIPS has a row and that every cell resolves.",
        "",
        "Each row binds one clause of a FIPS to its evidence, or to the decision that records a deviation.",
        "The [specification](specification.md) states the rules that the rows cite, and the [decision register](decision-register.md) holds the decisions.",
        "",
        "## Contents",
        "",
        "- [The FIPS documents](#the-fips-documents)",
        "- [Status values](#status-values)",
        "- [Summary](#summary)",
    ]
    for entry in data["fips"]:
        out.append(f"- [The rows of {entry['title']}](#{DOC_LINT.github_slug('The rows of ' + entry['title'])})")
    out += [
        "- [Spec holes](#spec-holes)",
        "",
        "## The FIPS documents",
        "",
        "*Table 1. The FIPS documents and their normative lines.*",
        "",
        "| Key | FIPS | Date | Path under the FIPS repository | SHA-256 | Normative lines | Outside the normative lines |",
        "|---|---|---|---|---|---|---|",
    ]
    for entry in data["fips"]:
        out.append(f"| `{entry['key']}` | {entry['title']} | {entry['date']} | `{entry['path']}` | "
                   f"`{entry['sha256']}` | {lines_text(entry['normative'])} | {cell(entry['outside'])} |")
    out += [
        "",
        "The normative lines are the non-blank lines of the ranges.",
        "A row may cover more lines than the clause needs, and two rows may share a line.",
        "",
        "## Status values",
        "",
        "*Table 2. The status values of a row.*",
        "",
        "| Status | Meaning |",
        "|---|---|",
    ]
    for status in STATUSES:
        out.append(f"| `{status}` | {STATUS_MEANING[status]} |")
    out += ["", "## Summary", "", "*Table 3. The number of rows of each status.*", "",
            "| FIPS | " + " | ".join(f"`{s}`" for s in STATUSES) + " | Rows |",
            "|---|" + "---|" * (len(STATUSES) + 1)]
    for entry in data["fips"]:
        rows = [r for r in data["rows"] if r["fips"] == entry["key"]]
        counts = [sum(1 for r in rows if r["status"] == s) for s in STATUSES]
        out.append(f"| {entry['title']} | " + " | ".join(str(c) for c in counts) + f" | {len(rows)} |")
    table = 4
    for entry in data["fips"]:
        rows = [r for r in data["rows"] if r["fips"] == entry["key"]]
        out += ["", f"## The rows of {entry['title']}", "",
                f"*Table {table}. The clauses of {entry['title']} and their status.*", "",
                "| Row | Lines | Clause | Status |", "|---|---|---|---|"]
        table += 1
        for row in rows:
            anchor = row["id"].lower()
            out.append(f"| [{row['id']}](#{anchor}) | {lines_text(row['lines'])} | {cell(row['clause'])} | `{row['status']}` |")
        for row in rows:
            out += ["", f"### {row['id']}", ""]
            out += render_fields(row)
    out += ["", "## Spec holes", "",
            "The design ledger names four holes of the FIPS that the legacy branches filled ad hoc.",
            f"Table {table} gives the resolution of each hole and the rows that carry it.",
            "", f"*Table {table}. The spec holes and their resolutions.*", "",
            "| Hole | FIPS lines | Resolution | Rows | Decisions |", "|---|---|---|---|---|"]
    for hole in data["spec_holes"]:
        rows = ", ".join(f"[{r}](#{r.lower()})" for r in hole["rows"])
        out.append(f"| {hole['id']}: {cell(hole['hole'])} | {lines_text(hole['lines'])} | "
                   f"{cell(hole['resolution'])} | {rows} | {decision_links(hole['decision'])} |")
    out.append("")
    return "\n".join(out)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def run(root: Path, fips_root: Path, matrix: Path, write: bool, out=sys.stdout) -> int:
    try:
        data = load_matrix(matrix)
    except InputError as error:
        print(f"check-fips-matrix: {error}", file=out)
        return EXIT_USAGE
    report = Report()
    check_schema(data, report)
    if report.failures:
        for line in report.lines:
            print(line, file=out)
        print(f"check-fips-matrix: CHECKS={report.checks} FAIL={report.failures} (schema)", file=out)
        return EXIT_FAIL
    texts = check_fips(data, fips_root, report)
    check_coverage(data, texts, report)
    check_rows(data, Resolver(root), report)
    rendering = root / data["rendering"]
    text = render(data)
    if write:
        rendering.write_text(text, encoding="utf-8")
    else:
        try:
            current = rendering.read_text(encoding="utf-8")
        except OSError:
            current = None
        report.check(current == text, f"{data['rendering']} is not the rendering of the JSON; run with --write")
    for line in report.lines:
        print(line, file=out)
    rows = len(data["rows"])
    print(f"check-fips-matrix: ROWS={rows} CHECKS={report.checks} FAIL={report.failures}", file=out)
    return EXIT_PASS if report.failures == 0 else EXIT_FAIL


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n", 1)[0])
    parser.add_argument("--repo-root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--fips-root", type=Path, default=None)
    parser.add_argument("--matrix", type=Path, default=None)
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args(argv)
    root = args.repo_root.resolve()
    fips_root = (args.fips_root or root.parent / "FIPS").resolve()
    matrix = args.matrix or root / MATRIX_PATH
    return run(root, fips_root, matrix, args.write)


if __name__ == "__main__":
    sys.exit(main())

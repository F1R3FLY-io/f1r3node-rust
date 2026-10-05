#!/usr/bin/env python3
"""Check the verification closure of the Reified RSpaces realignment leaves.

The checker reads scripts/verify-reified/closure-policies.json. That file
binds each pgmcp leaf of the realignment epic to its evidence. The evidence
is a set of model items, headlines, entry points, document headings,
document references and anchors. The file also describes two audit scripts
and the redacted ledger copy.

Each subcommand checks one group of rules. It prints one PASS or FAIL line
for each rule instance, and then a summary line. A subcommand that finds no
rule instance in the policy fails, so an empty policy never passes.

modeled
    Each model item of a leaf is in the model map. Its required flag is
    true, or false when the leaf lists it in optional_model_items. Its
    covers list overlaps the leaf headlines. Each leaf headline is in the
    model map. Each unmodeled item names a fix rule with an unmodeled
    reason. That rule lists the item, and the model map gives the item no
    covers.

invariants
    Each leaf headline has at least one property in the policy phase of
    the invariant map. The file of each such property defines its test.

entry-points
    Each entry-point regex matches at least one line of its file.

docs
    Each heading line occurs exactly once in its file. Each reference text
    occurs in its file. Each anchor section holds every keyword, with a
    case-sensitive match. A section starts at the line of its anchor tag
    and stops before the next line with an anchor tag.

audit --script PATH
    This subcommand runs the audit harness. The docstring of run_audit
    describes the harness.

ledger-copy
    This subcommand runs the five checks of the redacted ledger copy (see
    ledger_rules). The environment variable that the policy names gives
    the redaction home.

ledger-copy-controls
    This subcommand runs the negative controls of ledger-copy. A one-byte
    change of the copy must fail the ledger-copy rules. A changed copy
    digest in the README must also fail them.

The common options --root and --policies come before or after the
subcommand.

Exit codes: 0 every rule holds, 1 a rule failed, 2 a usage or input error.
Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import collections
import hashlib
import io
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path, PurePosixPath

SCHEMA = "reified-rspaces-closure-policies/1"
DEFAULT_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_POLICIES = Path("scripts") / "verify-reified" / "closure-policies.json"
EXIT_PASS = 0
EXIT_FAIL = 1
EXIT_INPUT = 2

TOP_KEYS = frozenset({"schema", "model_map", "invariant_map", "invariant_phase", "leaves", "audits", "ledger_copy"})
TOP_OPTIONAL_KEYS = frozenset({"description"})
LEAF_KEYS = frozenset({
    "note", "model_items", "optional_model_items", "headlines", "unmodeled_items",
    "entry_points", "doc_headings", "doc_references", "doc_anchors",
})
MODEL_LEAF_KEYS = ("model_items", "headlines", "unmodeled_items")
AUDIT_KEYS = frozenset({"args", "expected_exit", "cited_total", "mismatches", "links"})
AUDIT_OPTIONAL_KEYS = frozenset({"finding", "note"})
LEDGER_KEYS = frozenset({"path", "sha256", "lines", "original_sha256", "token", "token_count", "env", "readme"})
README_KEYS = frozenset({"path", "must_hold"})
HEX_DIGEST = re.compile(r"^[0-9a-f]{64}$")
ENV_NAME = re.compile(r"^[A-Z_][A-Z0-9_]*$")

# The P1 test-function rule of check-invariant-map.py.
TEST_FN = re.compile(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*[(<]", re.M)
ANCHOR_TAG = "<a id="

AUDIT_TIMEOUT_SECONDS = 600
# Built from parts: the checker detects this path and never uses it.
SYSTEM_TMP = "/" + "tmp"
SYSTEM_TMP_PATH = re.compile(r"(?<![A-Za-z0-9_.~})\]])" + re.escape(SYSTEM_TMP) + r"(?![A-Za-z0-9_.-])")
MKTEMP_CALL = re.compile(r"\bmktemp\b([^\n)`|;&]*)")
TEMP_NAME_IDIOMS = (
    (re.compile(r"\$\$"), "a file name from the process id ($$)"),
    (re.compile(r"\$\{?RANDOM\b"), "a file name from $RANDOM"),
    (re.compile(r"\bTMPDIR:?="), "an assignment to TMPDIR"),
)
MISMATCH_MARKERS = ("✗ ", "⚠ ")
FOUND_SUMMARY = re.compile(r"^Found (\d+) mismatch\(es\) out of (\d+) cited .+\.$")
CLEAN_SUMMARY = re.compile(r"^✓ all (\d+) cited .+$")
SHOWN_LINES = 10

HOME_PREFIX = "/home/"


class InputError(Exception):
    """An input of the checker is missing or malformed."""


class Report:
    """Collects and prints the PASS and FAIL lines of one subcommand."""

    def __init__(self, out) -> None:
        self.out = out
        self.rows: list[tuple[str, str, str]] = []

    def ok(self, rule: str, message: str) -> None:
        self._add("PASS", rule, message)

    def fail(self, rule: str, message: str) -> None:
        self._add("FAIL", rule, message)

    def _add(self, status: str, rule: str, message: str) -> None:
        self.rows.append((status, rule, message))
        print(f"{status}: {rule}: {message}", file=self.out)

    @property
    def passed(self) -> int:
        return sum(1 for status, _, _ in self.rows if status == "PASS")

    @property
    def failed(self) -> int:
        return sum(1 for status, _, _ in self.rows if status == "FAIL")

    def finish(self, title: str) -> int:
        if not self.rows:
            self.fail("coverage", "the policy defines no rule instance for this subcommand")
        print(f"closure {title}: PASS={self.passed} FAIL={self.failed}", file=self.out)
        return EXIT_PASS if self.failed == 0 else EXIT_FAIL


# ── Policy loading ─────────────────────────────────────────────────────────

def require(condition: bool, message: str) -> None:
    if not condition:
        raise InputError(message)


def relative_path(value, where: str) -> str:
    require(isinstance(value, str) and bool(value), f"{where} must be a non-empty string")
    pure = PurePosixPath(value)
    require(not pure.is_absolute() and ".." not in pure.parts and str(pure) == value,
            f"{where} must be a normalized relative path, not {value!r}")
    return value


def string_list(value, where: str, allow_empty: bool = False) -> list[str]:
    require(isinstance(value, list) and all(isinstance(entry, str) and entry for entry in value),
            f"{where} must be a list of non-empty strings")
    require(allow_empty or bool(value), f"{where} must not be empty")
    require(len(set(value)) == len(value), f"{where} names an entry twice")
    return value


def object_list(value, where: str, keys: frozenset[str]) -> list[dict]:
    require(isinstance(value, list) and bool(value), f"{where} must be a non-empty list")
    for index, entry in enumerate(value):
        require(isinstance(entry, dict) and set(entry) == keys,
                f"{where}[{index}] must be an object with exactly the keys {', '.join(sorted(keys))}")
    return value


def integer(value, where: str) -> int:
    require(isinstance(value, int) and not isinstance(value, bool) and value >= 0,
            f"{where} must be a non-negative integer")
    return value


def validate_leaf(leaf_id: str, leaf) -> None:
    where = f"leaves.{leaf_id}"
    require(isinstance(leaf, dict) and bool(leaf), f"{where} must be a non-empty object")
    unknown = sorted(set(leaf) - LEAF_KEYS)
    require(not unknown, f"{where} has unknown keys: {', '.join(unknown)}")
    if "note" in leaf:
        require(isinstance(leaf["note"], str) and bool(leaf["note"]), f"{where}.note must be a non-empty string")
    items = string_list(leaf.get("model_items", []), f"{where}.model_items", allow_empty=True)
    optional = string_list(leaf.get("optional_model_items", []), f"{where}.optional_model_items", allow_empty=True)
    outside = sorted(set(optional) - set(items))
    require(not outside, f"{where}.optional_model_items names items outside model_items: {', '.join(outside)}")
    string_list(leaf.get("headlines", []), f"{where}.headlines", allow_empty=True)
    if "unmodeled_items" in leaf:
        unmodeled = object_list(leaf["unmodeled_items"], f"{where}.unmodeled_items", frozenset({"item", "rule"}))
        names = [entry["item"] for entry in unmodeled]
        string_list(names, f"{where}.unmodeled_items[*].item")
        require(all(isinstance(entry["rule"], str) and entry["rule"] for entry in unmodeled),
                f"{where}.unmodeled_items[*].rule must be a non-empty string")
        both = sorted(set(names) & set(items))
        require(not both, f"{where} lists items as modeled and as unmodeled: {', '.join(both)}")
    for key, keys in (("entry_points", frozenset({"file", "pattern"})),
                      ("doc_headings", frozenset({"file", "heading"})),
                      ("doc_references", frozenset({"file", "text"})),
                      ("doc_anchors", frozenset({"file", "anchor", "keywords"}))):
        if key not in leaf:
            continue
        for index, entry in enumerate(object_list(leaf[key], f"{where}.{key}", keys)):
            label = f"{where}.{key}[{index}]"
            relative_path(entry["file"], f"{label}.file")
            if key == "entry_points":
                require(isinstance(entry["pattern"], str) and bool(entry["pattern"]), f"{label}.pattern must be a non-empty string")
                try:
                    re.compile(entry["pattern"])
                except re.error as error:
                    raise InputError(f"{label}.pattern is not a valid regex: {error}") from error
            elif key == "doc_anchors":
                require(isinstance(entry["anchor"], str) and bool(entry["anchor"]), f"{label}.anchor must be a non-empty string")
                string_list(entry["keywords"], f"{label}.keywords")
            else:
                field = "heading" if key == "doc_headings" else "text"
                require(isinstance(entry[field], str) and bool(entry[field]), f"{label}.{field} must be a non-empty string")


def validate_audit(script: str, entry) -> None:
    where = f"audits.{script}"
    relative_path(script, f"the key of {where}")
    top = PurePosixPath(script).parts[0]
    require(len(PurePosixPath(script).parts) >= 2, f"{where}: the script must lie in a directory below the root")
    require(isinstance(entry, dict), f"{where} must be an object")
    missing = sorted(AUDIT_KEYS - set(entry))
    unknown = sorted(set(entry) - AUDIT_KEYS - AUDIT_OPTIONAL_KEYS)
    require(not missing, f"{where} has no keys {', '.join(missing)}")
    require(not unknown, f"{where} has unknown keys: {', '.join(unknown)}")
    string_list(entry["args"], f"{where}.args", allow_empty=True)
    require(isinstance(entry["expected_exit"], int) and not isinstance(entry["expected_exit"], bool),
            f"{where}.expected_exit must be an integer")
    integer(entry["cited_total"], f"{where}.cited_total")
    require(isinstance(entry["mismatches"], list) and all(isinstance(line, str) and line for line in entry["mismatches"]),
            f"{where}.mismatches must be a list of non-empty strings")
    for link in string_list(entry["links"], f"{where}.links"):
        require(len(PurePosixPath(link).parts) == 1 and link not in (".", "..", ".git", top),
                f"{where}.links: {link!r} must be one top-level name other than .git and {top}")
    for key in ("finding", "note"):
        if key in entry:
            require(isinstance(entry[key], str) and bool(entry[key]), f"{where}.{key} must be a non-empty string")


def validate_ledger(spec) -> None:
    where = "ledger_copy"
    require(isinstance(spec, dict) and set(spec) == LEDGER_KEYS,
            f"{where} must be an object with exactly the keys {', '.join(sorted(LEDGER_KEYS))}")
    relative_path(spec["path"], f"{where}.path")
    for key in ("sha256", "original_sha256"):
        require(isinstance(spec[key], str) and bool(HEX_DIGEST.match(spec[key])), f"{where}.{key} must be a sha256 hex digest")
    integer(spec["lines"], f"{where}.lines")
    integer(spec["token_count"], f"{where}.token_count")
    require(isinstance(spec["token"], str) and bool(spec["token"]), f"{where}.token must be a non-empty string")
    require(isinstance(spec["env"], str) and bool(ENV_NAME.match(spec["env"])), f"{where}.env must be an environment variable name")
    readme = spec["readme"]
    require(isinstance(readme, dict) and set(readme) == README_KEYS,
            f"{where}.readme must be an object with exactly the keys {', '.join(sorted(README_KEYS))}")
    relative_path(readme["path"], f"{where}.readme.path")
    string_list(readme["must_hold"], f"{where}.readme.must_hold")


def load_policies(path: Path) -> dict:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise InputError(f"the policy file {path.name} does not exist") from error
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise InputError(f"the policy file {path.name} is not valid UTF-8 JSON: {error}") from error
    require(isinstance(data, dict), "the policy file must hold a JSON object")
    require(data.get("schema") == SCHEMA, f"the policy schema must be {SCHEMA}, not {data.get('schema')!r}")
    missing = sorted(TOP_KEYS - set(data))
    unknown = sorted(set(data) - TOP_KEYS - TOP_OPTIONAL_KEYS)
    require(not missing, f"the policy file has no keys {', '.join(missing)}")
    require(not unknown, f"the policy file has unknown keys: {', '.join(unknown)}")
    relative_path(data["model_map"], "model_map")
    relative_path(data["invariant_map"], "invariant_map")
    require(isinstance(data["invariant_phase"], str) and bool(data["invariant_phase"]), "invariant_phase must be a non-empty string")
    require(isinstance(data["leaves"], dict) and bool(data["leaves"]), "leaves must be a non-empty object")
    for leaf_id, leaf in data["leaves"].items():
        validate_leaf(leaf_id, leaf)
    require(isinstance(data["audits"], dict) and bool(data["audits"]), "audits must be a non-empty object")
    for script, entry in data["audits"].items():
        validate_audit(script, entry)
    validate_ledger(data["ledger_copy"])
    return data


def load_json(root: Path, rel: str, what: str) -> dict:
    try:
        data = json.loads((root / rel).read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise InputError(f"the {what} {rel} does not exist") from error
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise InputError(f"the {what} {rel} is not valid UTF-8 JSON: {error}") from error
    require(isinstance(data, dict), f"the {what} {rel} must hold a JSON object")
    return data


def read_text(root: Path, rel: str) -> tuple[str | None, str]:
    """Return the UTF-8 text of a checked file, or None and the reason."""
    path = root / rel
    if not path.is_file():
        return None, f"{rel} does not exist"
    try:
        return path.read_text(encoding="utf-8"), ""
    except UnicodeDecodeError:
        return None, f"{rel} is not valid UTF-8"


def counted(number: int, noun: str) -> str:
    return f"{number} {noun}" if number == 1 else f"{number} {noun}s"


def joined(words: list[str]) -> str:
    return words[0] if len(words) == 1 else ", ".join(words[:-1]) + " and " + words[-1]


def leaves_with(policies: dict, keys) -> list[tuple[str, dict]]:
    return [(leaf_id, leaf) for leaf_id, leaf in sorted(policies["leaves"].items())
            if any(leaf.get(key) for key in keys)]


# ── modeled ────────────────────────────────────────────────────────────────

def model_map_parts(root: Path, policies: dict) -> tuple[dict, dict, dict]:
    model_map = load_json(root, policies["model_map"], "model map")
    headlines, rules, entries = model_map.get("headlines"), model_map.get("rules"), model_map.get("items")
    if not isinstance(headlines, dict):
        raise InputError("the model map has no 'headlines' object")
    if not isinstance(rules, dict):
        raise InputError("the model map has no 'rules' object")
    if not isinstance(entries, list):
        raise InputError("the model map has no 'items' list")
    items: dict[str, dict] = {}
    for entry in entries:
        require(isinstance(entry, dict) and isinstance(entry.get("item"), str), "a model map item has no 'item' name")
        require(entry["item"] not in items, f"the model map names the item {entry['item']} twice")
        items[entry["item"]] = entry
    return headlines, rules, items


def check_modeled(root: Path, policies: dict, report: Report) -> None:
    headlines, rules, items = model_map_parts(root, policies)
    for leaf_id, leaf in leaves_with(policies, MODEL_LEAF_KEYS):
        leaf_headlines = leaf.get("headlines", [])
        for headline in leaf_headlines:
            entry = headlines.get(headline)
            if isinstance(entry, dict):
                report.ok("headline", f"{leaf_id}: {headline} is the {entry.get('kind')} headline {entry.get('name')} of the model map")
            else:
                report.fail("headline", f"{leaf_id}: the model map has no headline {headline}")
        optional = set(leaf.get("optional_model_items", []))
        for item in leaf.get("model_items", []):
            entry = items.get(item)
            if entry is None:
                report.fail("item", f"{leaf_id}: the model map has no item {item}")
                continue
            required = entry.get("required", False) is True
            if item in optional and required:
                report.fail("item", f"{leaf_id}: {item} has required true in the model map, but optional_model_items lists it")
            elif item in optional:
                report.ok("item", f"{leaf_id}: {item} is in the model map with required false, as optional_model_items declares")
            elif required:
                report.ok("item", f"{leaf_id}: {item} is in the model map with required true")
            else:
                report.fail("item", f"{leaf_id}: {item} has required false in the model map, and optional_model_items does not list it")
            covers = entry.get("covers") or []
            shared = [headline for headline in covers if headline in leaf_headlines]
            if shared:
                report.ok("overlap", f"{leaf_id}: {item} covers {joined(shared)} of the leaf headlines")
            else:
                held = joined(covers) if covers else "no headline"
                report.fail("overlap", f"{leaf_id}: {item} covers {held}, and no leaf headline is among them")
        for unmodeled in leaf.get("unmodeled_items", []):
            check_unmodeled(leaf_id, unmodeled["item"], unmodeled["rule"], rules, items, report)


def check_unmodeled(leaf_id: str, item: str, rule_id: str, rules: dict, items: dict, report: Report) -> None:
    problems = []
    rule = rules.get(rule_id)
    reason = ""
    if not isinstance(rule, dict):
        problems.append(f"the model map has no fix rule {rule_id}")
    else:
        stated = rule.get("unmodeled")
        reason = stated if isinstance(stated, str) else ""
        if not reason.strip():
            problems.append(f"fix rule {rule_id} gives no unmodeled reason")
        if item not in (rule.get("items") or []):
            problems.append(f"fix rule {rule_id} does not list {item}")
    entry = items.get(item)
    if entry is None:
        problems.append(f"the model map has no item {item}")
    elif entry.get("covers"):
        problems.append(f"the model map gives {item} the covers {', '.join(entry['covers'])}")
    for problem in problems:
        report.fail("unmodeled", f"{leaf_id}: {item}: {problem}")
    if not problems:
        report.ok("unmodeled", f"{leaf_id}: {item} is unmodeled, and fix rule {rule_id} gives the reason: {reason}")


# ── invariants ─────────────────────────────────────────────────────────────

def test_functions(text: str) -> set[str]:
    stripped = "\n".join(line.split("//", 1)[0] for line in text.split("\n"))
    return set(TEST_FN.findall(stripped))


def check_invariants(root: Path, policies: dict, report: Report) -> None:
    invariant_map = load_json(root, policies["invariant_map"], "invariant map")
    phase_name = policies["invariant_phase"]
    phases = invariant_map.get("phases")
    phase = phases.get(phase_name) if isinstance(phases, dict) else None
    if not isinstance(phase, dict):
        raise InputError(f"the invariant map has no phase {phase_name}")
    properties = phase.get("properties")
    if not isinstance(properties, list):
        raise InputError(f"phase {phase_name} of the invariant map has no 'properties' list")
    for prop in properties:
        require(isinstance(prop, dict) and isinstance(prop.get("test"), str) and isinstance(prop.get("file"), str)
                and isinstance(prop.get("covers"), list),
                f"a property of phase {phase_name} has no test, file or covers")
    defined: dict[str, set[str] | None] = {}
    for leaf_id, leaf in leaves_with(policies, ("headlines",)):
        for headline in leaf["headlines"]:
            covering = [prop for prop in properties if headline in prop["covers"]]
            if not covering:
                report.fail("property", f"{leaf_id}: {headline} has no property in phase {phase_name} of the invariant map")
                continue
            problems = []
            for prop in covering:
                rel = prop["file"]
                if rel not in defined:
                    text, _ = read_text(root, rel)
                    defined[rel] = None if text is None else test_functions(text)
                functions = defined[rel]
                label = f"property {prop.get('number')} ({prop['test']})"
                if functions is None:
                    problems.append(f"{label}: the file {rel} does not exist or is not UTF-8")
                elif prop["test"] not in functions:
                    problems.append(f"{label}: {rel} does not define the test")
            for problem in problems:
                report.fail("property", f"{leaf_id}: {headline}: {problem}")
            if not problems:
                names = joined([f"{prop.get('number')} ({prop['test']})" for prop in covering])
                noun = "property" if len(covering) == 1 else "properties"
                report.ok("property", f"{leaf_id}: {headline} has the {noun} {names}, and the property files define the tests")


# ── entry-points ───────────────────────────────────────────────────────────

def check_entry_points(root: Path, policies: dict, report: Report) -> None:
    for leaf_id, leaf in leaves_with(policies, ("entry_points",)):
        for entry in leaf["entry_points"]:
            rel, pattern = entry["file"], entry["pattern"]
            text, reason = read_text(root, rel)
            if text is None:
                report.fail("entry-point", f"{leaf_id}: {reason}, so the pattern {pattern} has no line")
                continue
            regex = re.compile(pattern)
            hits = [number for number, line in enumerate(text.split("\n"), start=1) if regex.search(line)]
            if hits:
                report.ok("entry-point", f"{leaf_id}: {rel}:{hits[0]} matches {pattern}")
            else:
                report.fail("entry-point", f"{leaf_id}: no line of {rel} matches {pattern}")


# ── docs ───────────────────────────────────────────────────────────────────

def shorten(text: str, limit: int = 72) -> str:
    return text if len(text) <= limit else text[:limit] + "..."


def check_docs(root: Path, policies: dict, report: Report) -> None:
    cache: dict[str, tuple[str | None, str]] = {}

    def text_of(rel: str) -> tuple[str | None, str]:
        if rel not in cache:
            cache[rel] = read_text(root, rel)
        return cache[rel]

    def lines_of(rel: str) -> tuple[list[str] | None, str]:
        text, reason = text_of(rel)
        return (None, reason) if text is None else (text.split("\n"), "")

    for leaf_id, leaf in leaves_with(policies, ("doc_headings", "doc_references", "doc_anchors")):
        for entry in leaf.get("doc_headings", []):
            rel, heading = entry["file"], entry["heading"]
            lines, reason = lines_of(rel)
            if lines is None:
                report.fail("heading", f"{leaf_id}: {reason}, so it has no heading {heading!r}")
                continue
            hits = [number for number, line in enumerate(lines, start=1) if line == heading]
            if len(hits) == 1:
                report.ok("heading", f"{leaf_id}: {rel}:{hits[0]} is the heading {heading!r}")
            elif not hits:
                report.fail("heading", f"{leaf_id}: {rel} has no line {heading!r}")
            else:
                report.fail("heading", f"{leaf_id}: {rel} has the heading {heading!r} {len(hits)} times, at lines "
                                       + ", ".join(str(hit) for hit in hits))
        for entry in leaf.get("doc_references", []):
            rel, reference = entry["file"], entry["text"]
            text, reason = text_of(rel)
            if text is None:
                report.fail("reference", f"{leaf_id}: {reason}, so it holds no text {shorten(reference)!r}")
            elif reference in text:
                number = text[:text.index(reference)].count("\n") + 1
                report.ok("reference", f"{leaf_id}: {rel}:{number} holds the text {shorten(reference)!r}")
            else:
                report.fail("reference", f"{leaf_id}: {rel} does not hold the text {shorten(reference)!r}")
        for entry in leaf.get("doc_anchors", []):
            check_anchor(leaf_id, entry, lines_of, report)


def check_anchor(leaf_id: str, entry: dict, lines_of, report: Report) -> None:
    rel, anchor = entry["file"], entry["anchor"]
    lines, reason = lines_of(rel)
    if lines is None:
        report.fail("anchor", f"{leaf_id}: {reason}, so it has no anchor {anchor}")
        return
    tag = f'{ANCHOR_TAG}"{anchor}"'
    starts = [index for index, line in enumerate(lines) if tag in line]
    if len(starts) != 1:
        count = "no" if not starts else f"{len(starts)}"
        report.fail("anchor", f"{leaf_id}: {rel} has {count} anchor tags for {anchor}, and the section needs exactly one")
        return
    start = starts[0]
    stop = next((index for index in range(start + 1, len(lines)) if ANCHOR_TAG in lines[index]), len(lines))
    section = "\n".join(lines[start:stop])
    for keyword in entry["keywords"]:
        where = f"{rel}:{start + 1}-{stop}"
        if keyword in section:
            report.ok("anchor", f"{leaf_id}: section {anchor} ({where}) holds {keyword!r}")
        else:
            report.fail("anchor", f"{leaf_id}: section {anchor} ({where}) does not hold {keyword!r}")


# ── audit ──────────────────────────────────────────────────────────────────

def script_key(root: Path, script: str) -> str:
    path = Path(script)
    if path.is_absolute():
        try:
            path = path.resolve().relative_to(root)
        except ValueError as error:
            raise InputError("the audit script must lie under the root") from error
    return PurePosixPath(path.as_posix()).as_posix()


def static_problems(text: str) -> tuple[list[str], list[str]]:
    """Return the problems of the two static rules of an audit script."""
    tmp_problems = [f"line {number} names the system temporary directory"
                    for number, line in enumerate(text.split("\n"), start=1) if SYSTEM_TMP_PATH.search(line)]
    temp_problems = []
    for number, line in enumerate(text.split("\n"), start=1):
        for pattern, label in TEMP_NAME_IDIOMS:
            if pattern.search(line):
                temp_problems.append(f"line {number} uses {label}")
        for match in MKTEMP_CALL.finditer(line):
            for token in match.group(1).split():
                token = token.strip("\"'")
                named = (token.startswith("--tmpdir=")
                         or (token.startswith("-") and not token.startswith("--") and "p" in token[1:])
                         or (not token.startswith("-") and "/" in token))
                if named:
                    temp_problems.append(f"line {number} calls mktemp with a directory ({token})")
    return tmp_problems, temp_problems


def snapshot(directory: Path) -> dict[str, tuple]:
    """Record every entry under directory without a walk through symbolic links."""
    state: dict[str, tuple] = {}
    for current, dirnames, filenames in os.walk(directory):
        for name in dirnames + filenames:
            path = Path(current) / name
            rel = path.relative_to(directory).as_posix()
            info = os.lstat(path)
            if stat.S_ISLNK(info.st_mode):
                state[rel] = ("link", os.readlink(path))
            elif stat.S_ISDIR(info.st_mode):
                state[rel] = ("directory", stat.S_IMODE(info.st_mode))
            elif stat.S_ISREG(info.st_mode):
                digest = hashlib.sha256(path.read_bytes()).hexdigest()
                state[rel] = ("file", stat.S_IMODE(info.st_mode), info.st_size, info.st_mtime_ns, digest)
            else:
                state[rel] = ("other", info.st_mode)
    return state


def listed(lines: list[str]) -> str:
    shown = lines[:SHOWN_LINES]
    more = len(lines) - len(shown)
    return " | ".join(shown) + (f" | and {more} more" if more else "")


def run_audit(root: Path, policies: dict, script_arg: str, environ, report: Report,
              scratch_parent: Path | None = None) -> str:
    """Run one audit script in a hermetic harness and check its result.

    The audit scripts find the repository with git rev-parse --show-toplevel
    from their own directory. The pgmcp snapshot has no .git directory, so
    the harness builds a minimal repository in a fresh directory. That
    directory comes from tempfile.mkdtemp, which honors TMPDIR.

    1. The harness checks two static rules on the script text. The text names
       no path in the system temporary directory. Each temporary file comes
       from mktemp in its default directory, with no name from $$ or $RANDOM
       and no assignment to TMPDIR. A script that fails a static rule does
       not run, because it can write where the harness cannot look.
    2. root/.git holds HEAD (ref: refs/heads/main), objects/ and refs/heads/.
    3. The directory of the script (scripts/audit/) is a copy, because the
       script finds the repository from its own location. Each root that the
       policy links is a symbolic link to the real root.
    4. bash -n checks the syntax. Then bash runs the script with the policy
       arguments in a cleared environment. That environment holds PATH from
       the caller, LC_ALL=C.UTF-8, empty HOME and TMPDIR directories,
       GIT_CONFIG_NOSYSTEM=1 and GIT_CONFIG_GLOBAL=/dev/null.
       GIT_CEILING_DIRECTORIES names the harness directory, so git never
       finds a repository outside the harness. The locale matters, because
       an audit script can sort its output with the collation of LC_ALL.
    5. The harness removes its root prefix from each output line. Then it
       compares the exit code, the citation total and the multiset of
       mismatch lines with the policy. Every other output line and every
       stderr line is a failure.
    6. HOME and TMPDIR must be empty after the run. No entry under the
       harness root may change.
    7. The harness directory is removed on every path.

    It returns the policy key of the script.
    """
    script = script_key(root, script_arg)
    entry = policies["audits"].get(script)
    require(entry is not None, f"the policy has no audit entry for {script}")
    caller_path = environ.get("PATH", "")
    require(bool(caller_path), "PATH is not set, and the harness needs it to find bash and git")
    require(shutil.which("bash", path=caller_path) is not None, "bash is not on PATH")
    for link in entry["links"]:
        require((root / link).exists(), f"the link root {link} does not exist in the repository")
    text, reason = read_text(root, script)
    if text is None:
        report.fail("script", reason)
        return script
    tmp_problems, temp_problems = static_problems(text)
    for rule, problems, success in (
            ("static-tmp", tmp_problems, "names no path in the system temporary directory"),
            ("static-mktemp", temp_problems, "makes temporary files only with mktemp in its default directory")):
        for problem in problems:
            report.fail(rule, f"{script}: {problem}")
        if not problems:
            report.ok(rule, f"{script} {success}")
    if tmp_problems or temp_problems:
        report.fail("run", f"{script} does not run, because it fails a static rule")
        return script
    base = Path(os.path.realpath(tempfile.mkdtemp(prefix="closure-audit-", dir=scratch_parent)))
    try:
        exercise(root, script, entry, caller_path, base, report)
    except OSError as error:
        report.fail("harness", f"the harness for {script} cannot be built or read: {error.strerror}")
    finally:
        remove_harness(base, entry["links"], report)
    return script


def exercise(root: Path, script: str, entry: dict, caller_path: str, base: Path, report: Report) -> None:
    harness, home, scratch = base / "root", base / "home", base / "tmp"
    for directory in (harness, home, scratch):
        directory.mkdir()
    (harness / ".git" / "objects").mkdir(parents=True)
    (harness / ".git" / "refs" / "heads").mkdir(parents=True)
    (harness / ".git" / "HEAD").write_text("ref: refs/heads/main\n", encoding="utf-8")
    script_dir = PurePosixPath(script).parent
    shutil.copytree(root / script_dir, harness / script_dir, symlinks=True)
    for link in entry["links"]:
        (harness / link).symlink_to(root / link, target_is_directory=(root / link).is_dir())
    before = snapshot(harness)
    env = {
        "PATH": caller_path,
        "LC_ALL": "C.UTF-8",
        "HOME": str(home),
        "TMPDIR": str(scratch),
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "GIT_CEILING_DIRECTORIES": str(base),
    }

    def normalize(stream: bytes) -> list[str]:
        decoded = stream.decode("utf-8", errors="replace").replace(f"{harness}/", "").replace(str(harness), ".")
        lines = decoded.split("\n")
        return lines[:-1] if lines and lines[-1] == "" else lines

    def execute(arguments: list[str]):
        return subprocess.run(["bash", *arguments], env=env, cwd=harness, stdin=subprocess.DEVNULL,
                              capture_output=True, timeout=AUDIT_TIMEOUT_SECONDS, check=False)

    syntax = execute(["-n", str(harness / script)])
    if syntax.returncode != 0:
        report.fail("syntax", f"bash -n rejects {script}: {listed(normalize(syntax.stderr))}")
    else:
        report.ok("syntax", f"bash -n accepts {script}")
        try:
            run = execute([str(harness / script), *entry["args"]])
        except subprocess.TimeoutExpired:
            report.fail("exit", f"{script} ran longer than {AUDIT_TIMEOUT_SECONDS} seconds")
        else:
            compare_output(script, entry, run.returncode, normalize(run.stdout), normalize(run.stderr), report)
    for rule, directory, name in (("tmpdir", scratch, "TMPDIR"), ("home", home, "HOME")):
        left = sorted(os.listdir(directory))
        if left:
            report.fail(rule, f"{script} left entries in its {name}: {', '.join(left)}")
        else:
            report.ok(rule, f"the {name} of {script} is empty after the run")
    after = snapshot(harness)
    changed = sorted(rel for rel in set(before) | set(after) if before.get(rel) != after.get(rel))
    if changed:
        report.fail("harness-root", f"{script} changed entries under the harness root: {listed(changed)}")
    else:
        report.ok("harness-root", f"no entry under the harness root changed ({len(before)} entries)")


def compare_output(script: str, entry: dict, code: int, stdout: list[str], stderr: list[str], report: Report) -> None:
    if code == entry["expected_exit"]:
        report.ok("exit", f"{script} exits with {code}")
    else:
        report.fail("exit", f"{script} exits with {code}, and the policy expects {entry['expected_exit']}")
    if stderr:
        report.fail("stderr", f"{script} writes to stderr: {listed(stderr)}")
    else:
        report.ok("stderr", f"{script} writes nothing to stderr")
    mismatches, unexpected, summary = [], [], None
    for line in stdout:
        if line.startswith(MISMATCH_MARKERS):
            mismatches.append(line)
        elif line == "":
            continue
        elif (found := FOUND_SUMMARY.match(line)) is not None and summary is None:
            summary = (int(found.group(1)), int(found.group(2)))
        elif (clean := CLEAN_SUMMARY.match(line)) is not None and summary is None:
            summary = (0, int(clean.group(1)))
        else:
            unexpected.append(line)
    if unexpected:
        report.fail("output", f"{script} prints lines that are not mismatches or the summary: {listed(unexpected)}")
    else:
        report.ok("output", f"each output line of {script} is a mismatch line, a blank line or the summary")
    if summary is None:
        report.fail("total", f"the output of {script} holds no citation total")
    else:
        count, total = summary
        if total == entry["cited_total"]:
            report.ok("total", f"{script} checks {total} citations")
        else:
            report.fail("total", f"{script} checks {total} citations, and the policy expects {entry['cited_total']}")
        if count == len(mismatches):
            report.ok("count", f"the summary of {script} counts its {count} mismatch lines")
        else:
            report.fail("count", f"the summary of {script} counts {count} mismatches, and the output holds {len(mismatches)} mismatch lines")
    observed = collections.Counter(mismatches)
    expected = collections.Counter(entry["mismatches"])
    extra = sorted((observed - expected).elements())
    missing = sorted((expected - observed).elements())
    for line in extra:
        report.fail("mismatch", f"{script} prints a mismatch line that the policy does not list: {line}")
    for line in missing:
        report.fail("mismatch", f"{script} does not print the policy mismatch line: {line}")
    if not extra and not missing:
        finding = f" (finding {entry['finding']})" if "finding" in entry else ""
        report.ok("mismatch", f"the {len(mismatches)} mismatch lines of {script} equal the policy set{finding}")


def remove_harness(base: Path, links: list[str], report: Report) -> None:
    try:
        for link in links:
            candidate = base / "root" / link
            if candidate.is_symlink():
                candidate.unlink()
        shutil.rmtree(base)
    except OSError as error:
        report.fail("cleanup", f"the harness directory cannot be removed: {error.strerror}")
        return
    if base.exists():
        report.fail("cleanup", "the harness directory still exists after the removal")
    else:
        report.ok("cleanup", "the harness directory is removed")


# ── ledger-copy ────────────────────────────────────────────────────────────

def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def ledger_rules(copy: bytes, readme: str, spec: dict, home: str) -> list[tuple[str, bool, str]]:
    """Evaluate the five ledger-copy checks. Never put the home in a message.

    1. The copy has the policy digest and line count.
    2. The copy holds the token exactly token_count times.
    3. The copy holds no home-directory path.
    4. The copy with each token replaced by the redaction home has the
       original digest (inverse redaction).
    5. The README holds every must_hold string.
    """
    return copy_rules(copy, spec, home) + readme_rules(readme, spec)


def copy_rules(copy: bytes, spec: dict, home: str) -> list[tuple[str, bool, str]]:
    """Evaluate the checks 1 to 4 of ledger_rules on the copy."""
    rel, token, env = spec["path"], spec["token"], spec["env"]
    rows: list[tuple[str, bool, str]] = []
    digest = sha256_hex(copy)
    holds = digest == spec["sha256"]
    rows.append(("copy-digest", holds, f"{rel} has the sha256 {digest}"
                 + ("" if holds else f", and the policy expects {spec['sha256']}")))
    count = copy.count(b"\n")
    holds = count == spec["lines"]
    rows.append(("copy-lines", holds, f"{rel} has {count} lines"
                 + ("" if holds else f", and the policy expects {spec['lines']}")))
    tokens = copy.count(token.encode("utf-8"))
    holds = tokens == spec["token_count"]
    rows.append(("token-count", holds, f"{rel} holds {counted(tokens, token + ' token')}"
                 + ("" if holds else f", and the policy expects {spec['token_count']}")))
    paths = copy.count(HOME_PREFIX.encode("utf-8"))
    rows.append(("no-home-path", paths == 0, f"{rel} holds {counted(paths, HOME_PREFIX + ' string')}"))
    restored = sha256_hex(copy.replace(token.encode("utf-8"), home.encode("utf-8")))
    holds = restored == spec["original_sha256"]
    rows.append(("inverse-redaction", holds, f"the copy with each {token} replaced by ${env} has the sha256 {restored}"
                 + ("" if holds else f", and the policy expects the original sha256 {spec['original_sha256']}")))
    return rows


def readme_rules(readme: str, spec: dict) -> list[tuple[str, bool, str]]:
    """Evaluate the check 5 of ledger_rules on the README."""
    readme_rel = spec["readme"]["path"]
    return [("readme", text in readme, f"{readme_rel} {'holds' if text in readme else 'does not hold'} {shorten(text)!r}")
            for text in spec["readme"]["must_hold"]]


def ledger_inputs(root: Path, policies: dict, environ, report: Report) -> tuple[bytes | None, str | None, dict, str]:
    spec = policies["ledger_copy"]
    home = environ.get(spec["env"], "")
    require(bool(home), f"the environment variable {spec['env']} is not set. "
                        f"The inverse redaction needs it to restore the original ledger.")
    copy_path = root / spec["path"]
    copy = copy_path.read_bytes() if copy_path.is_file() else None
    if copy is None:
        report.fail("copy-digest", f"{spec['path']} does not exist")
    readme, reason = read_text(root, spec["readme"]["path"])
    if readme is None:
        report.fail("readme", reason)
    return copy, readme, spec, home


def check_ledger_copy(root: Path, policies: dict, environ, report: Report) -> None:
    copy, readme, spec, home = ledger_inputs(root, policies, environ, report)
    rows = (copy_rules(copy, spec, home) if copy is not None else []) + (readme_rules(readme, spec) if readme is not None else [])
    for rule, holds, message in rows:
        if holds:
            report.ok(rule, message)
        else:
            report.fail(rule, message)


def failing_rules(rows: list[tuple[str, bool, str]]) -> list[str]:
    return sorted({rule for rule, holds, _ in rows if not holds})


def one_byte_offset(copy: bytes, token: bytes) -> int | None:
    """Find an ASCII letter or digit outside every token, from the middle on."""
    covered = set()
    start = copy.find(token)
    while start >= 0 and token:
        covered.update(range(start, start + len(token)))
        start = copy.find(token, start + 1)
    middle = len(copy) // 2
    for offset in list(range(middle, len(copy))) + list(range(0, middle)):
        if offset not in covered and chr(copy[offset]).isascii() and chr(copy[offset]).isalnum():
            return offset
    return None


def check_ledger_controls(root: Path, policies: dict, environ, report: Report) -> None:
    copy, readme, spec, home = ledger_inputs(root, policies, environ, report)
    if copy is None or readme is None:
        report.fail("baseline", "the controls need the copy and the README")
        return
    baseline = ledger_rules(copy, readme, spec, home)
    failed = failing_rules(baseline)
    if failed:
        report.fail("baseline", f"the unchanged copy and README fail {joined(failed)}, and the controls need a passing baseline")
    else:
        report.ok("baseline", f"the unchanged copy and README pass all {len(baseline)} ledger-copy checks")
    token = spec["token"].encode("utf-8")
    offset = one_byte_offset(copy, token)
    if offset is None:
        report.fail("control-copy-byte", "the copy has no ASCII letter or digit outside the tokens to change")
    else:
        replacement = b"x" if copy[offset] != ord("x") else b"y"
        mutated = copy[:offset] + replacement + copy[offset + 1:]
        failed = failing_rules(ledger_rules(mutated, readme, spec, home))
        needed = sorted({"copy-digest", "inverse-redaction"} - set(failed))
        if needed:
            report.fail("control-copy-byte", f"a change of byte {offset} of the copy does not fail {joined(needed)}")
        else:
            report.ok("control-copy-byte", f"a change of byte {offset} of the copy fails {joined(failed)}")
    if spec["sha256"] not in readme:
        report.fail("control-readme-digest", "the README does not hold the copy digest, so the control cannot change it")
        return
    last = spec["sha256"][-1]
    changed = spec["sha256"][:-1] + ("1" if last == "0" else "0")
    failed = failing_rules(ledger_rules(copy, readme.replace(spec["sha256"], changed), spec, home))
    if "readme" in failed:
        report.ok("control-readme-digest", f"a changed copy digest in the README fails {joined(failed)}")
    else:
        report.fail("control-readme-digest", "a changed copy digest in the README does not fail the readme check")


# ── main ───────────────────────────────────────────────────────────────────

def parser() -> argparse.ArgumentParser:
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--root", default=argparse.SUPPRESS,
                        help="worktree root (default: two directories above this script)")
    common.add_argument("--policies", default=argparse.SUPPRESS,
                        help="policy file (default: <root>/scripts/verify-reified/closure-policies.json)")
    top = argparse.ArgumentParser(prog="check-closure.py", parents=[common],
                                  description="Check the verification closure of the Reified RSpaces realignment leaves.")
    commands = top.add_subparsers(dest="command", required=True)
    for name, text in (("modeled", "model items, headlines and unmodeled items"),
                       ("invariants", "the properties of each leaf headline"),
                       ("entry-points", "the entry-point patterns"),
                       ("docs", "document headings, references and anchor keywords"),
                       ("ledger-copy", "the redacted ledger copy"),
                       ("ledger-copy-controls", "the negative controls of ledger-copy")):
        commands.add_parser(name, parents=[common], help=text)
    audit = commands.add_parser("audit", parents=[common], help="run an audit script in the harness")
    audit.add_argument("--script", required=True, help="audit script, relative to the root")
    return top


def main(argv: list[str] | None = None, environ=None, out=None, err=None,
         scratch_parent: Path | None = None) -> int:
    out = sys.stdout if out is None else out
    err = sys.stderr if err is None else err
    environ = os.environ if environ is None else environ
    arguments = parser().parse_args(argv)
    root = Path(getattr(arguments, "root", DEFAULT_ROOT)).resolve()
    policy_path = Path(getattr(arguments, "policies", root / DEFAULT_POLICIES))
    report = Report(out)
    title = arguments.command
    try:
        policies = load_policies(policy_path)
        if arguments.command == "modeled":
            check_modeled(root, policies, report)
        elif arguments.command == "invariants":
            check_invariants(root, policies, report)
        elif arguments.command == "entry-points":
            check_entry_points(root, policies, report)
        elif arguments.command == "docs":
            check_docs(root, policies, report)
        elif arguments.command == "audit":
            title = f"audit {run_audit(root, policies, arguments.script, environ, report, scratch_parent)}"
        elif arguments.command == "ledger-copy":
            check_ledger_copy(root, policies, environ, report)
        else:
            check_ledger_controls(root, policies, environ, report)
    except InputError as error:
        print(f"ERROR: {error}", file=err)
        return EXIT_INPUT
    return report.finish(title)


if __name__ == "__main__":
    for stream in (sys.stdout, sys.stderr):
        if isinstance(stream, io.TextIOWrapper):
            stream.reconfigure(encoding="utf-8", errors="backslashreplace")
    sys.exit(main())

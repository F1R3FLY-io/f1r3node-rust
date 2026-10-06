#!/usr/bin/env python3
"""Check formal/reified-rspaces-invariant-map.json (Reified RSpaces).

The invariant map connects each formal headline (a Rocq theorem, a TLA+
invariant or a consensus-checklist item) with its property-based tests. The
map has one section for each phase: P1 (the storage substrate: H1-H23 and
T1-T6, with H23 and T6 of decision 36), P2 (the wire encoding: E1-E12, the
12 headlines of Encoding.v) and
ORD (ordinary execution: O1, item 10 of the consensus checklist). The checker
fails when:

  * a required id of a phase is missing, or an entry names no test (an
    unmapped entry).
  * a headline names a theorem that the .v file does not define or does not
    check with "Print Assumptions" (an unknown theorem).
  * a T# names an operator that the .tla file does not define (an unknown
    invariant).
  * a "checklist" entry names an unknown item of the consensus-checklist
    map, or a name that is not the id of the item.
  * a "checklist" entry names a test that no rust_test check of the item
    names.
  * a .cfg file checks an invariant that is neither a mapped T# nor a listed
    auxiliary invariant, or a T# is in no safe .cfg file.
  * the property table of a phase is not numbered 1..N, has the wrong size,
    names a test twice, cites an unknown entry, or disagrees with the
    per-entry test lists.
  * (test side) a test of a phase with "test_status": "required" is missing
    from its test file, or a test file has a prop_ function that the map
    does not name.
  * a witness test (the optional "witness_tests" list of a phase: a
    deterministic test that witnesses headlines) names an unknown entry,
    covers nothing and declares no "unmodeled" reason, occurs twice, or
    (test side) is missing from its file.

A test of a phase with "test_status": "planned" may not exist yet. The
checker reports each such test on a MISSING line. When every other check
passes, it then exits with code 3: the map is correct, and the tests do not
exist yet. A property or a witness test with "status": "pending" is a test
that another change still writes: its absence is a PENDING note (exit code
4 when nothing else fails or is missing), and its presence is a FAIL
(remove the status). --formal-only skips the test side.

Exit codes: 0 all checks pass, 1 a check failed, 2 usage or input error,
3 the map passes and planned tests do not exist yet, 4 the map passes and
pending tests do not exist yet.
Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_MAP = REPO_ROOT / "formal" / "reified-rspaces-invariant-map.json"
SCHEMA = "reified-rspaces-invariant-map/3"
CHECKLIST_SCHEMA = "reified-rspaces-consensus-checklist/1"
# The ids that each known phase must map. Another phase takes its ids from
# the map. Decision 36 adds H23 and T6 to P1 (replay reproduces play).
REQUIRED_IDS = {
    "P1": tuple(f"H{i}" for i in range(1, 24)) + tuple(f"T{i}" for i in range(1, 7)),
    "P2": tuple(f"E{i}" for i in range(1, 13)),
    "ORD": ("O1",),
}
TEST_STATUSES = ("required", "planned")
EXIT_MISSING_TESTS = 3
EXIT_PENDING_TESTS = 4


class Report:
    def __init__(self) -> None:
        self.passed = 0
        self.failed = 0
        self.missing = 0
        self.pending = 0

    def ok(self, message: str) -> None:
        self.passed += 1
        print(f"PASS: {message}")

    def fail(self, message: str) -> None:
        self.failed += 1
        print(f"FAIL: {message}")

    def absent(self, message: str) -> None:
        self.missing += 1
        print(f"MISSING: {message}")

    def wait(self, message: str) -> None:
        self.pending += 1
        print(f"PENDING: {message}")

    @staticmethod
    def note(message: str) -> None:
        print(f"NOTE: {message}")


def strip_block_comments(text: str) -> str:
    """Remove nested (* ... *) comments. Keep newlines for line numbers."""
    out = []
    depth = 0
    i = 0
    while i < len(text):
        pair = text[i:i + 2]
        if pair == "(*":
            depth += 1
            i += 2
            continue
        if pair == "*)" and depth > 0:
            depth -= 1
            i += 2
            continue
        ch = text[i]
        if depth == 0 or ch == "\n":
            out.append(ch)
        i += 1
    return "".join(out)


def strip_line_comments(text: str, marker: str) -> str:
    return "\n".join(line.split(marker, 1)[0] for line in text.split("\n"))


ROCQ_THEOREM = re.compile(
    r"^\s*(?:Theorem|Lemma|Corollary|Proposition|Fact|Remark)\s+([A-Za-z_][A-Za-z0-9_']*)\b",
    re.M)
ROCQ_PRINT_ASSUMPTIONS = re.compile(r"^\s*Print\s+Assumptions\s+([A-Za-z_][A-Za-z0-9_'.]*)\s*\.", re.M)
TLA_OPERATOR = re.compile(r"^([A-Za-z_][A-Za-z0-9_]*)\s*(?:\([^)]*\))?\s*==", re.M)
CFG_SECTION = re.compile(
    r"^\s*(SPECIFICATION|INIT|NEXT|INVARIANTS?|PROPERTY|PROPERTIES|CONSTANTS?|"
    r"CONSTRAINTS?|ACTION_CONSTRAINTS?|SYMMETRY|VIEW|CHECK_DEADLOCK|POSTCONDITION|ALIAS)\b(.*)$")
TEST_FN = re.compile(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*[(<]", re.M)


def id_key(fid: str) -> tuple[str, int]:
    match = re.match(r"^([A-Za-z]+)(\d+)$", fid)
    return (match.group(1), int(match.group(2))) if match else (fid, 0)


def rocq_theorems(path: Path) -> tuple[set[str], set[str]]:
    text = strip_block_comments(path.read_text(encoding="utf-8"))
    checked = {name.split(".")[-1] for name in ROCQ_PRINT_ASSUMPTIONS.findall(text)}
    return set(ROCQ_THEOREM.findall(text)), checked


def tla_operators(path: Path) -> set[str]:
    text = strip_line_comments(strip_block_comments(path.read_text(encoding="utf-8")), "\\*")
    return set(TLA_OPERATOR.findall(text))


def checklist_items(path: Path) -> dict[int, tuple[str, set[str]]] | str:
    """Map each item number of a consensus-checklist map to (id, the tests of its rust_test checks).

    The result is an error message when the file is not a consensus-checklist map.
    """
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        return f"{path.name} is not valid JSON: {error}"
    if not isinstance(data, dict) or data.get("schema") != CHECKLIST_SCHEMA:
        schema = data.get("schema") if isinstance(data, dict) else None
        return f"{path.name} has schema {schema!r}, expected {CHECKLIST_SCHEMA}"
    items: dict[int, tuple[str, set[str]]] = {}
    for item in data.get("items") or []:
        tests = {check.get("test") for check in item.get("checks") or [] if check.get("kind") == "rust_test"}
        items[item.get("number")] = (item.get("id"), tests)
    return items


def cfg_invariants(path: Path) -> list[str]:
    """The names in the INVARIANT and INVARIANTS sections of a TLC config."""
    text = strip_line_comments(strip_block_comments(path.read_text(encoding="utf-8")), "\\*")
    names: list[str] = []
    section = None
    for line in text.split("\n"):
        match = CFG_SECTION.match(line)
        if match:
            section = match.group(1)
            rest = match.group(2)
        else:
            rest = line
        if section in ("INVARIANT", "INVARIANTS"):
            names.extend(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", rest))
    return names


def load_map(path: Path) -> dict:
    try:
        with path.open(encoding="utf-8") as handle:
            data = json.load(handle)
    except FileNotFoundError:
        print(f"ERROR: invariant map not found: {path}", file=sys.stderr)
        sys.exit(2)
    except json.JSONDecodeError as error:
        print(f"ERROR: {path} is not valid JSON: {error}", file=sys.stderr)
        sys.exit(2)
    for key in ("schema", "phases", "auxiliary_tla_invariants"):
        if key not in data:
            print(f"ERROR: {path} has no '{key}' key", file=sys.stderr)
            sys.exit(2)
    if data["schema"] != SCHEMA:
        print(f"ERROR: unknown schema {data['schema']!r} (expected {SCHEMA})", file=sys.stderr)
        sys.exit(2)
    if "tla_config_dir" not in data and "tla_config_dirs" not in data:
        print(f"ERROR: {path} has no 'tla_config_dir' or 'tla_config_dirs' key", file=sys.stderr)
        sys.exit(2)
    for name, phase in data["phases"].items():
        for key in ("test_status", "test_files", "formal", "properties", "expected_properties"):
            if key not in phase:
                print(f"ERROR: phase {name} has no '{key}' key", file=sys.stderr)
                sys.exit(2)
    return data


class Formal:
    """Caches of the Rocq theorems, the TLA+ operators and the consensus-checklist items."""

    def __init__(self) -> None:
        self.rocq: dict[Path, tuple[set[str], set[str]]] = {}
        self.tla: dict[Path, set[str]] = {}
        self.checklists: dict[Path, dict[int, tuple[str, set[str]]] | str] = {}

    def check_entry(self, fid: str, entry: dict, report: Report) -> bool:
        kind, name, rel = entry.get("kind"), entry.get("name"), entry.get("file", "")
        path = REPO_ROOT / rel
        if not path.is_file():
            report.fail(f"{fid}: formal file is missing: {rel}")
            return False
        if kind == "rocq":
            if path not in self.rocq:
                self.rocq[path] = rocq_theorems(path)
            theorems, checked = self.rocq[path]
            if name not in theorems:
                report.fail(f"{fid}: unknown theorem {name} (not defined in {rel})")
                return False
            if name not in checked:
                report.fail(f"{fid}: theorem {name} has no 'Print Assumptions {name}.' line")
                return False
            return True
        if kind == "tla":
            if path not in self.tla:
                self.tla[path] = tla_operators(path)
            if name not in self.tla[path]:
                report.fail(f"{fid}: unknown invariant {name} (not defined in {rel})")
                return False
            return True
        if kind == "checklist":
            if path not in self.checklists:
                self.checklists[path] = checklist_items(path)
            items = self.checklists[path]
            if isinstance(items, str):
                report.fail(f"{fid}: {items}")
                return False
            number = entry.get("item")
            if isinstance(number, bool) or not isinstance(number, int) or number not in items:
                report.fail(f"{fid}: unknown checklist item {number!r} (not in {rel})")
                return False
            item_id, item_tests = items[number]
            if item_id != name:
                report.fail(f"{fid}: checklist item {number} has the id {item_id!r}, not {name!r}")
                return False
            unnamed = sorted(set(entry.get("tests") or []) - item_tests)
            if unnamed:
                report.fail(f"{fid}: checklist item {number} ({name}) names no rust_test check for "
                            f"{', '.join(unnamed)}")
                return False
            return True
        report.fail(f"{fid}: unknown kind {kind!r}")
        return False


def check_phase(name: str, phase: dict, expected: int, formal_cache: Formal, report: Report) -> None:
    formal: dict = phase["formal"]
    if phase["test_status"] not in TEST_STATUSES:
        report.fail(f"{name}: unknown test_status {phase['test_status']!r}")
    for fid in REQUIRED_IDS.get(name, ()):
        if fid not in formal:
            report.fail(f"{name}: unmapped: {fid} is not in the invariant map")
    extra = sorted(set(formal) - set(REQUIRED_IDS.get(name, formal)), key=id_key)
    for fid in extra:
        report.fail(f"{name}: {fid} is not an id of phase {name}")
    for fid, entry in sorted(formal.items(), key=lambda kv: id_key(kv[0])):
        if not formal_cache.check_entry(f"{name} {fid}", entry, report):
            continue
        tests = entry.get("tests") or []
        if not tests:
            report.fail(f"{name}: unmapped: {fid} ({entry.get('name')}) names no property test")
            continue
        report.ok(f"{name} {fid} {entry.get('name')} -> {', '.join(tests)}")

    properties: list = phase["properties"]
    numbers = [p.get("number") for p in properties]
    if numbers != list(range(1, len(properties) + 1)):
        report.fail(f"{name}: the property table is not numbered 1..{len(properties)}: {numbers}")
    if len(properties) != expected:
        report.fail(f"{name}: the property table has {len(properties)} properties, expected {expected}")
    tests = [p.get("test") for p in properties]
    duplicates = sorted({t for t in tests if tests.count(t) > 1})
    if duplicates:
        report.fail(f"{name}: the property table names a test twice: {', '.join(duplicates)}")
    from_table: dict[str, set[str]] = {fid: set() for fid in formal}
    test_files = set(phase["test_files"])
    for prop in properties:
        if prop.get("status") not in (None, "pending"):
            report.fail(f"{name}: property {prop.get('number')} has unknown status {prop.get('status')!r}")
        covers = prop.get("covers") or []
        if not covers:
            report.fail(f"{name}: property {prop.get('number')} ({prop.get('test')}) covers nothing")
        if prop.get("file") is not None and prop["file"] not in test_files:
            report.fail(f"{name}: property {prop.get('number')} names a file outside test_files: {prop['file']}")
        for fid in covers:
            if fid not in formal:
                report.fail(f"{name}: property {prop.get('number')} cites unknown entry {fid}")
            else:
                from_table[fid].add(prop.get("test"))
    consistent = True
    for fid, entry in formal.items():
        listed = set(entry.get("tests") or [])
        if listed != from_table.get(fid, set()):
            consistent = False
            report.fail(f"{name}: {fid}: tests {sorted(listed)} disagree with the property table "
                        f"{sorted(from_table.get(fid, set()))}")
    if consistent and not duplicates and numbers == list(range(1, len(properties) + 1)):
        report.ok(f"{name}: the property table ({len(properties)} properties) agrees with the per-entry test lists")

    witnesses: list = phase.get("witness_tests") or []
    seen_witnesses: set[str] = set()
    for witness in witnesses:
        test, covers = witness.get("test"), witness.get("covers") or []
        problems = []
        if not test:
            problems.append("has no test name")
        elif test in seen_witnesses:
            problems.append("occurs twice")
        elif test in tests:
            problems.append("is also in the property table")
        seen_witnesses.add(test or "")
        if not witness.get("file"):
            problems.append("names no file")
        for fid in covers:
            if fid not in formal:
                problems.append(f"cites unknown entry {fid}")
        if not covers and not witness.get("unmodeled"):
            problems.append("covers nothing and declares no 'unmodeled' reason")
        if covers and witness.get("unmodeled"):
            problems.append("covers entries but also declares an 'unmodeled' reason")
        if witness.get("status") not in (None, "pending"):
            problems.append(f"has unknown status {witness.get('status')!r}")
        if problems:
            report.fail(f"{name}: witness test {test}: " + "; ".join(problems))
        elif covers:
            report.ok(f"{name}: witness test {test} -> {', '.join(covers)}")
        else:
            report.ok(f"{name}: witness test {test} is unmodeled: {witness['unmodeled']}")


def check_configs(data: dict, formal_cache: Formal, report: Report) -> None:
    """Every invariant that a .cfg checks is a mapped T# or an auxiliary one.

    The operators of every mapped TLA+ module load here, for all phases. Thus a
    phase selection without TLA+ entries (for example --phase ORD) still finds
    the auxiliary invariants.
    """
    tla_names: dict[str, str] = {}
    for name, phase in data["phases"].items():
        for fid, entry in phase["formal"].items():
            if entry.get("kind") == "tla":
                tla_names[entry["name"]] = f"{name} {fid}"
                path = REPO_ROOT / entry.get("file", "")
                if path.is_file() and path not in formal_cache.tla:
                    formal_cache.tla[path] = tla_operators(path)
    auxiliary: dict = data["auxiliary_tla_invariants"]
    all_operators = set().union(*formal_cache.tla.values()) if formal_cache.tla else set()
    for name in auxiliary:
        if name not in all_operators:
            report.fail(f"auxiliary invariant {name} is not defined in a mapped TLA+ module")
    dirs = data.get("tla_config_dirs") or [data["tla_config_dir"]]
    cfgs = sorted(cfg for rel in dirs for cfg in (REPO_ROOT / rel).glob("*.cfg"))
    if not cfgs:
        report.fail(f"no .cfg files in {', '.join(dirs)}")
    in_safe_cfg: set[str] = set()
    for cfg in cfgs:
        names = cfg_invariants(cfg)
        unknown = [n for n in names if n not in tla_names and n not in auxiliary]
        if unknown:
            report.fail(f"{cfg.name}: checks unmapped invariant(s) {', '.join(unknown)}")
        else:
            report.ok(f"{cfg.name}: every checked invariant is mapped or auxiliary")
        if "Unsafe" not in cfg.stem:
            in_safe_cfg |= set(names)
    for name, fid in sorted(tla_names.items(), key=lambda kv: kv[1]):
        if name not in in_safe_cfg:
            report.fail(f"{fid} {name} is in no safe .cfg file")


def test_functions(path: Path) -> set[str]:
    text = strip_line_comments(path.read_text(encoding="utf-8"), "//")
    return set(TEST_FN.findall(text))


def check_tests(name: str, phase: dict, report: Report) -> None:
    planned = phase["test_status"] == "planned"
    defined: dict[str, set[str]] = {}
    for rel in phase["test_files"]:
        path = REPO_ROOT / rel
        if path.is_file():
            defined[rel] = test_functions(path)
        elif planned:
            report.note(f"{name}: test file {rel} does not exist yet")
        else:
            report.fail(f"{name}: test file {rel} is missing")
    named = []
    for prop in phase["properties"]:
        test = prop.get("test")
        named.append(test)
        pending = prop.get("status") == "pending"
        files = [prop["file"]] if prop.get("file") else list(phase["test_files"])
        found = [rel for rel in files if test in defined.get(rel, set())]
        where = " or ".join(files)
        if found and pending:
            report.fail(f"{name}: test function {test} exists in {found[0]}. Remove its 'pending' status")
        elif found:
            report.ok(f"{name}: test function {test} exists in {found[0]}")
        elif pending:
            report.wait(f"{name}: test {test} does not exist yet (declared pending in {where})")
        elif planned:
            report.absent(f"{name}: missing test {test} (planned in {where})")
        else:
            report.fail(f"{name}: missing test function {test} in {where}")
    for witness in phase.get("witness_tests") or []:
        test, rel = witness.get("test"), witness.get("file", "")
        pending = witness.get("status") == "pending"
        path = REPO_ROOT / rel
        if rel not in defined and path.is_file():
            defined[rel] = test_functions(path)
        exists = test in defined.get(rel, set())
        if exists and pending:
            report.fail(f"{name}: witness test {test} exists in {rel}. Remove its 'pending' status")
        elif exists:
            report.ok(f"{name}: witness test {test} exists in {rel}")
        elif pending:
            report.wait(f"{name}: witness test {test} does not exist yet (declared pending in {rel})")
        else:
            report.fail(f"{name}: missing witness test {test} in {rel}")
    for rel, functions in sorted(defined.items()):
        if rel not in phase["test_files"]:
            continue
        for function in sorted(f for f in functions if f.startswith("prop_") and f not in named):
            report.fail(f"{name}: unmapped property test {function} in {rel}")


def parse_expected(values: list[str]) -> dict[str, int]:
    out: dict[str, int] = {}
    for value in values:
        phase, sep, count = value.partition("=")
        if not sep or not count.isdigit():
            print(f"ERROR: --expected-properties takes PHASE=N, got {value!r}", file=sys.stderr)
            sys.exit(2)
        out[phase] = int(count)
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n\n")[0])
    parser.add_argument("--map", type=Path, default=DEFAULT_MAP, help="invariant map JSON (default: %(default)s)")
    parser.add_argument("--formal-only", action="store_true",
                        help="check the formal side only (skip the test files)")
    parser.add_argument("--expected-properties", action="append", default=[], metavar="PHASE=N",
                        help="override the property count of a phase (default: the map's expected_properties)")
    parser.add_argument("--phase", action="append", default=[], metavar="PHASE",
                        help="check only these phases (repeatable; default: every phase)")
    args = parser.parse_args()
    data = load_map(args.map)
    overrides = parse_expected(args.expected_properties)
    report = Report()
    formal_cache = Formal()
    unknown = [name for name in args.phase if name not in data["phases"]]
    if unknown:
        print(f"unknown phase(s): {', '.join(unknown)}", file=sys.stderr)
        sys.exit(2)
    selected = {name: phase for name, phase in data["phases"].items() if not args.phase or name in args.phase}
    for name, phase in selected.items():
        expected = overrides[name] if name in overrides else int(phase["expected_properties"])
        check_phase(name, phase, expected, formal_cache, report)
    check_configs(data, formal_cache, report)
    if args.formal_only:
        report.note("--formal-only: the test files were not checked")
    else:
        for name, phase in selected.items():
            check_tests(name, phase, report)
    print(f"invariant-map: PASS={report.passed} FAIL={report.failed} MISSING={report.missing} "
          f"PENDING={report.pending}")
    if report.failed:
        return 1
    if report.missing:
        print(f"EXPECTED-FAIL: {report.missing} planned test(s) do not exist yet "
              f"(the map passes, and the tests do not exist yet)")
        return EXIT_MISSING_TESTS
    if report.pending:
        return EXIT_PENDING_TESTS
    return 0


if __name__ == "__main__":
    sys.exit(main())

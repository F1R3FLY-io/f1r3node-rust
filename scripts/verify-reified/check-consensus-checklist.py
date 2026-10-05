#!/usr/bin/env python3
"""Check scripts/verify-reified/consensus-checklist.json (Reified RSpaces).

The map holds the 22 items of the consensus-preservation checklist for
ordinary execution. Each item lists the checks that prove it. Each check
names the recipe step that runs it. For one milestone, the checker confirms
that each active check is in place:

  * rust_test: the cargo step runs the package and the target of the test
    file. The step filter selects the test. The file defines the test as a
    test function that is not ignored.
  * footprint: the footprint policy of the milestone protects each pinned
    file of each path glob. byte_identical needs an unchanged file.
    approved_edits also accepts an approved edit. The policy must also hold
    each no_new_files glob of the check, and its allowed_new_files list may
    name no file in such a glob.
  * complete: the completeness policies forbid each token of the check, in
    the production scan or in the phase of the milestone.
  * a step with a script: the checker script exists.

A check is active from its "from" milestone to its "until" milestone, in the
order of "milestones". A check without "from" is active from the first
milestone. A check without "until" stays active to the last milestone. The
checker prints SCHEDULED for a check before "from" and RETIRED for a check
after "until". It does not look for their tests. Each item needs at least one
active check.

The test lookup removes // comments and nested /* */ comments. It tracks
each attribute across lines until its brackets balance. A function is a test
only when a test attribute precedes it. A block comment without an end fails
each check of that file. A /* inside a string literal also confuses the
lookup. The lookup then fails loudly.

Output: one PASS, FAIL, SCHEDULED or RETIRED line for each check, and then a
summary line. CHECKS counts all status lines: the map rules, the item checks
and the step scripts.

Exit codes: 0 every active check passes, 1 a check failed, 2 usage or input
error. Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_ROOT = HERE.parents[1]
MAP_PATH = Path("scripts/verify-reified/consensus-checklist.json")
SCHEMA = "reified-rspaces-consensus-checklist/1"
MILESTONE = "@milestone"
PRODUCTION_SCAN = "production_scan"
CHECK_KINDS = ("rust_test", "footprint", "complete")
FOOTPRINT_MODES = ("byte_identical", "approved_edits")
REQUIRED_FIELDS = {
    "rust_test": ("file", "test"),
    "footprint": ("policy", "mode", "paths"),
    "complete": ("scope", "patterns"),
}
CRATE_PACKAGES = {"rholang": "rholang", "rspace++": "rspace_plus_plus", "casper": "casper", "models": "models"}
EXAMPLE_COUNT = 3

COMMENT_TOKEN = re.compile(r"//|/\*|\*/|\n")
TEST_ATTRIBUTE = re.compile(r"^#\[\s*(?:tokio::)?test\b")
IGNORE_ATTRIBUTE = re.compile(r"^#\[\s*ignore\b")
FUNCTION = re.compile(
    r"^(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\b"
)
# The tests of a Rust file ({name: ignored}) and all its function names.
TestScan = tuple[dict[str, bool], set[str]]


def load_footprint_module():
    """Load check-footprint.py, so that the coverage rules equal the rules of the byte checker."""
    path = HERE / "check-footprint.py"
    spec = importlib.util.spec_from_file_location("consensus_checklist_footprint", path)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


FOOTPRINT = load_footprint_module()


class MapInputError(Exception):
    """The map or an argument is not valid input. The checker exits with code 2."""


class Report:
    def __init__(self, milestone: str) -> None:
        self.milestone = milestone
        self.items = 0
        self.lines: list[str] = []
        self.counts = {"PASS": 0, "FAIL": 0, "SCHEDULED": 0, "RETIRED": 0}

    def add(self, status: str, message: str) -> None:
        self.counts[status] += 1
        self.lines.append(f"{status}: {message}")

    @property
    def passed(self) -> int:
        return self.counts["PASS"]

    @property
    def failed(self) -> int:
        return self.counts["FAIL"]

    @property
    def scheduled(self) -> int:
        return self.counts["SCHEDULED"]

    @property
    def retired(self) -> int:
        return self.counts["RETIRED"]

    @property
    def checks(self) -> int:
        return sum(self.counts.values())

    def summary(self) -> str:
        return (
            f"consensus-checklist (milestone {self.milestone}): ITEMS={self.items} CHECKS={self.checks} "
            f"PASS={self.passed} FAIL={self.failed} SCHEDULED={self.scheduled} RETIRED={self.retired}"
        )


def strip_comments(text: str) -> tuple[str, bool]:
    """Remove // comments and nested /* */ comments. Keep every newline.

    The second value is True when a block comment has no end.
    """
    out: list[str] = []
    depth = 0
    position = 0
    in_line_comment = False
    for match in COMMENT_TOKEN.finditer(text):
        token, start = match.group(0), match.start()
        if in_line_comment:
            if token == "\n":
                in_line_comment = False
                out.append("\n")
                position = match.end()
            continue
        if depth == 0:
            out.append(text[position:start])
            if token == "//":
                in_line_comment = True
            elif token == "/*":
                depth = 1
            else:
                out.append(token)
        elif token == "/*":
            depth += 1
        elif token == "*/":
            depth -= 1
        elif token == "\n":
            out.append("\n")
        position = match.end()
    if depth == 0 and not in_line_comment:
        out.append(text[position:])
    return "".join(out), depth > 0


def bracket_end(text: str, depth: int) -> tuple[int | None, int]:
    """Track [ and ] from DEPTH. Return (the index after the closing bracket, 0), or (None, the new depth)."""
    for index, character in enumerate(text):
        if character == "[":
            depth += 1
        elif character == "]":
            depth -= 1
            if depth == 0:
                return index + 1, 0
    return None, depth


def scan_tests(text: str) -> TestScan | None:
    """Return ({test name: ignored}, {function names}) of a Rust source.

    The result is None when a block comment has no end.
    """
    stripped, unterminated = strip_comments(text)
    if unterminated:
        return None
    tests: dict[str, bool] = {}
    functions: set[str] = set()
    pending_test = pending_ignore = False
    attribute: str | None = None
    depth = 0
    for raw in stripped.split("\n"):
        line = raw.strip()
        while True:
            if attribute is None:
                if not line:
                    break
                if not line.startswith(("#[", "#![")):
                    match = FUNCTION.match(line)
                    if match is not None:
                        functions.add(match.group(1))
                        if pending_test:
                            tests[match.group(1)] = tests.get(match.group(1), False) or pending_ignore
                    pending_test = pending_ignore = False
                    break
                opening = line.index("[")
                end, depth = bracket_end(line[opening:], 0)
                if end is None:
                    attribute = line
                    break
                attribute, line = line[:opening + end], line[opening + end:].strip()
            else:
                end, depth = bracket_end(line, depth)
                if end is None:
                    attribute = f"{attribute} {line}"
                    break
                attribute, line = f"{attribute} {line[:end]}", line[end:].strip()
            if TEST_ATTRIBUTE.match(attribute):
                pending_test = True
            if IGNORE_ATTRIBUTE.match(attribute):
                pending_ignore = True
            attribute = None
    return tests, functions


def resolve(value: str, milestone: str) -> str:
    return milestone if value == MILESTONE else value


def examples(paths: list[str]) -> str:
    shown = ", ".join(paths[:EXAMPLE_COUNT])
    return shown if len(paths) <= EXAMPLE_COUNT else f"{shown} and {len(paths) - EXAMPLE_COUNT} more"


class Context:
    """The files that the checks read. Each file loads once."""

    def __init__(self, root: Path, data: dict) -> None:
        self.root = root
        self.data = data
        self.json_files: dict[str, dict | str] = {}
        self.scans: dict[Path, TestScan | None] = {}
        self.matches: dict[str, list[str]] = {}
        self.pinned: list[str] | str | None = None

    def json_file(self, key: str) -> dict | str:
        """Return the JSON object that the map key names, or an error message."""
        if key not in self.json_files:
            relative = self.data[key]
            path = self.root / relative
            try:
                loaded = json.loads(path.read_text(encoding="utf-8"))
                self.json_files[key] = loaded if isinstance(loaded, dict) else f"{relative} is not a JSON object"
            except FileNotFoundError:
                self.json_files[key] = f"{relative} is missing"
            except json.JSONDecodeError as error:
                self.json_files[key] = f"{relative} is not valid JSON: {error}"
        return self.json_files[key]

    def pinned_files(self, policies: dict) -> list[str] | str:
        """Return the sorted pinned paths of the digest file of the footprint policies, or an error message."""
        if self.pinned is None:
            relative = policies.get("digests", "")
            try:
                document = json.loads((self.root / relative).read_text(encoding="utf-8"))
                self.pinned = sorted(document["files"])
            except (FileNotFoundError, IsADirectoryError):
                self.pinned = f"the digest file {relative!r} is missing"
            except (json.JSONDecodeError, KeyError, TypeError) as error:
                self.pinned = f"the digest file {relative!r} is not valid: {error}"
        return self.pinned

    def matched(self, glob: str, pinned: list[str]) -> list[str]:
        if glob not in self.matches:
            self.matches[glob] = [path for path in pinned if FOOTPRINT.matches_any(path, [glob])]
        return self.matches[glob]

    def scan(self, path: Path) -> TestScan | None:
        if path not in self.scans:
            self.scans[path] = scan_tests(path.read_text(encoding="utf-8"))
        return self.scans[path]


def rust_test_problems(check: dict, step_key: str, step: dict, context: Context) -> list[str]:
    package, target = step.get("package"), step.get("target")
    if package is None or target is None:
        return [f"step {step_key} names no cargo package and target"]
    relative, test = check["file"], check["test"]
    crate, _, inside = relative.partition("/")
    expected_package = CRATE_PACKAGES.get(crate)
    if expected_package is None:
        return [f"{relative} is in no known crate"]
    if expected_package != package:
        return [f"{relative} belongs to package {expected_package}, but step {step_key} runs package {package}"]
    if not inside.startswith("tests/") or not relative.endswith(".rs"):
        return [f"{relative} is not a Rust file under {crate}/tests/"]
    stem = inside[len("tests/"):-len(".rs")]
    module = check.get("module")
    if "/" not in stem:
        if module is not None:
            return [f"{relative} is a test target of its own, so the check takes no module"]
        if target != stem:
            return [f"step target mismatch: step {step_key} runs target {target}, but {relative} is target {stem}"]
        test_path = test
    else:
        if module is None:
            return [f"{relative} is nested under {crate}/tests/, so the check needs a module"]
        if f"{crate}/tests/{module.replace('::', '/')}.rs" != relative:
            return [f"module {module} does not name {relative}"]
        test_path = f"{module}::{test}"
    test_filter = step.get("filter")
    if test_filter is not None and not test_path.startswith(test_filter):
        return [f"the filter {test_filter} of step {step_key} does not select {test_path}"]
    path = context.root / relative
    if not path.is_file():
        return [f"test file {relative} is missing"]
    scan = context.scan(path)
    if scan is None:
        return [f"{relative}: a block comment has no end"]
    tests, functions = scan
    if test in tests:
        return ["ignored test"] if tests[test] else []
    if test in functions:
        return ["function without a test attribute"]
    return ["missing test function"]


def footprint_problems(check: dict, policy_name: str, context: Context) -> list[str]:
    policies = context.json_file("footprint_policies")
    if isinstance(policies, str):
        return [f"footprint policies: {policies}"]
    if policy_name not in policies.get("policies", {}):
        return [f"unknown footprint policy {policy_name}"]
    try:
        unchanged, no_new_files, edits = FOOTPRINT.resolve_policy(policies, policy_name)
    except SystemExit as error:
        return [f"footprint policy {policy_name}: {error}"]
    pinned = context.pinned_files(policies)
    if isinstance(pinned, str):
        return [pinned]
    edited = {edit.get("path") for edit in edits}
    byte_identical = check["mode"] == "byte_identical"
    problems = []
    for glob in check["paths"]:
        matched = context.matched(glob, pinned)
        if not matched:
            problems.append(f"{glob} names no pinned file (re-pin)")
            continue
        if byte_identical:
            weak = [path for path in matched if path in edited or not FOOTPRINT.matches_any(path, unchanged)]
            rule = "byte-identical"
        else:
            weak = [path for path in matched if path not in edited and not FOOTPRINT.matches_any(path, unchanged)]
            rule = "unchanged or an approved edit"
        if weak:
            problems.append(f"{glob}: {len(weak)} of {len(matched)} pinned files are not {rule} "
                            f"under policy {policy_name} ({examples(weak)})")
    for glob in check.get("no_new_files", []):
        if glob not in no_new_files:
            problems.append(f"policy {policy_name} has no no_new_files glob {glob}")
    try:
        allowed = FOOTPRINT.resolve_allowed_new_files(policies, policy_name) or []
    except SystemExit as error:
        return problems + [f"footprint policy {policy_name}: {error}"]
    for glob in check.get("no_new_files", []):
        exceptions = [path for path in allowed if FOOTPRINT.matches_any(path, [glob])]
        if exceptions:
            problems.append(f"policy {policy_name} allows new files in {glob} ({examples(exceptions)})")
    return problems


def complete_problems(check: dict, scope: str, context: Context) -> list[str]:
    policies = context.json_file("complete_policies")
    if isinstance(policies, str):
        return [f"completeness policies: {policies}"]
    if scope == PRODUCTION_SCAN:
        section = policies.get("production_scan")
        if not isinstance(section, dict):
            return ["the completeness policies have no production_scan section"]
        listed = {rule.get("pattern") for rule in section.get("patterns", [])}
        where = "the production scan"
    else:
        if scope not in policies.get("phases", {}):
            return [f"the completeness policies have no phase {scope}"]
        listed = {rule.get("pattern") for rule in policies.get("forbidden_anywhere", [])}
        where = f"forbidden_anywhere (phase {scope})"
    missing = [pattern for pattern in check["patterns"] if pattern not in listed]
    return [f"{where} does not forbid {', '.join(missing)}"] if missing else []


def describe(number: int, check: dict, milestone: str) -> str:
    kind, step = check["kind"], check["step"]
    if kind == "rust_test":
        return f"item {number} rust_test {check['test']} ({check['file']}, step {step})"
    if kind == "footprint":
        return f"item {number} footprint ({check['mode']}, policy {resolve(check['policy'], milestone)}, step {step})"
    return f"item {number} complete ({resolve(check['scope'], milestone)}, step {step})"


def validate(data: object, milestone: str, selected: list[int] | None) -> None:
    """Raise MapInputError when the map or an argument is not valid input."""
    if not isinstance(data, dict):
        raise MapInputError("the map is not a JSON object")
    if data.get("schema") != SCHEMA:
        raise MapInputError(f"unknown schema {data.get('schema')!r} (expected {SCHEMA})")
    for key in ("expected_items", "milestones", "steps", "items", "footprint_policies", "complete_policies"):
        if key not in data:
            raise MapInputError(f"the map has no {key!r} key")
    if isinstance(data["expected_items"], bool) or not isinstance(data["expected_items"], int):
        raise MapInputError("expected_items must be a whole number")
    milestones = data["milestones"]
    if not isinstance(milestones, list) or not milestones or not all(isinstance(name, str) for name in milestones):
        raise MapInputError("milestones must be a list of milestone names")
    if milestone not in milestones:
        raise MapInputError(f"unknown milestone {milestone} (the map knows {', '.join(milestones)})")
    steps = data["steps"]
    if not isinstance(steps, dict) or not all(isinstance(step, dict) for step in steps.values()):
        raise MapInputError("steps must map each step name to an object")
    if not isinstance(data["items"], list) or not all(isinstance(item, dict) for item in data["items"]):
        raise MapInputError("items must be a list of objects")
    numbers = []
    for item in data["items"]:
        number = item.get("number")
        numbers.append(number)
        checks = item.get("checks", [])
        if not isinstance(checks, list) or not all(isinstance(check, dict) for check in checks):
            raise MapInputError(f"item {number}: checks must be a list of objects")
        for check in checks:
            kind = check.get("kind")
            if kind not in CHECK_KINDS:
                raise MapInputError(f"item {number}: unknown kind {kind!r}")
            if check.get("step") not in steps:
                raise MapInputError(f"item {number}: step {check.get('step')!r} is not in steps")
            absent = [field for field in REQUIRED_FIELDS[kind] if field not in check]
            if absent:
                raise MapInputError(f"item {number}: a {kind} check has no {', '.join(absent)}")
            if kind == "footprint" and check["mode"] not in FOOTPRINT_MODES:
                raise MapInputError(f"item {number}: unknown mode {check['mode']!r}")
            for bound in ("from", "until"):
                if bound in check and check[bound] not in milestones:
                    raise MapInputError(f"item {number}: unknown {bound} milestone {check[bound]!r}")
    unknown = sorted(set(selected or []) - set(numbers))
    if unknown:
        raise MapInputError(f"unknown item(s): {', '.join(str(number) for number in unknown)}")


def check_numbering(data: dict, report: Report) -> None:
    items = data["items"]
    expected = data["expected_items"]
    numbers = [item.get("number") for item in items]
    ids = [item.get("id") for item in items]
    repeated = sorted({str(item_id) for item_id in ids if ids.count(item_id) > 1})
    holds = True
    if numbers != list(range(1, expected + 1)):
        holds = False
        report.add("FAIL", f"the items are numbered {numbers}, expected 1..{expected}")
    if repeated:
        holds = False
        report.add("FAIL", f"the item ids {', '.join(repeated)} occur more than once")
    if holds:
        report.add("PASS", f"the map numbers its {expected} items 1..{expected}, and the item ids are unique")


def check_map(root: Path, data: dict, milestone: str, items: list[int] | None = None) -> Report:
    """Check the map at MILESTONE. ITEMS limits the item checks to these numbers."""
    validate(data, milestone, items)
    milestones = data["milestones"]
    position = milestones.index(milestone)
    steps = data["steps"]
    context = Context(root, data)
    report = Report(milestone)
    check_numbering(data, report)
    for item in data["items"]:
        number = item["number"]
        if items is not None and number not in items:
            continue
        report.items += 1
        active = 0
        for check in item.get("checks", []):
            label = describe(number, check, milestone)
            if position < milestones.index(check.get("from", milestones[0])):
                owner = f" (owner {check['owner']})" if "owner" in check else ""
                report.add("SCHEDULED", f"{label}: active from {check['from']}{owner}")
                continue
            if position > milestones.index(check.get("until", milestones[-1])):
                report.add("RETIRED", f"{label}: retired after {check['until']}")
                continue
            active += 1
            kind = check["kind"]
            if kind == "rust_test":
                problems = rust_test_problems(check, check["step"], steps[check["step"]], context)
            elif kind == "footprint":
                problems = footprint_problems(check, resolve(check["policy"], milestone), context)
            else:
                problems = complete_problems(check, resolve(check["scope"], milestone), context)
            if problems:
                report.add("FAIL", f"{label}: {'. '.join(problems)}")
            else:
                report.add("PASS", label)
        if active:
            report.add("PASS", f"item {number} ({item.get('id')}) has {active} active checks at {milestone}")
        else:
            report.add("FAIL", f"item {number} has no active check at {milestone}")
    for key, step in steps.items():
        if "script" not in step:
            continue
        command = " ".join([step["script"], *(resolve(argument, milestone) for argument in step.get("args", []))])
        if (root / step["script"]).is_file():
            report.add("PASS", f"step {key} runs {command}")
        else:
            report.add("FAIL", f"step {key}: unknown checker {step['script']}")
    return report


def load_map(path: Path) -> dict:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise MapInputError(f"the map {path} is missing") from error
    except json.JSONDecodeError as error:
        raise MapInputError(f"the map {path} is not valid JSON: {error}") from error
    if not isinstance(data, dict):
        raise MapInputError(f"the map {path} is not a JSON object")
    if data.get("schema") != SCHEMA:
        raise MapInputError(f"unknown schema {data.get('schema')!r} in {path} (expected {SCHEMA})")
    return data


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Check the Reified RSpaces consensus-checklist map.")
    parser.add_argument("--map", type=Path, help=f"map file (default: <root>/{MAP_PATH})")
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT,
                        help="worktree root (default: two directories above the script)")
    parser.add_argument("--milestone", help="milestone to check (default: the first milestone of the map)")
    parser.add_argument("--item", type=int, nargs="+", action="extend", metavar="N",
                        help="check only these items (the numbering and count checks still run)")
    arguments = parser.parse_args(argv)
    root = arguments.root.resolve()
    map_path = arguments.map if arguments.map is not None else root / MAP_PATH
    try:
        data = load_map(map_path)
        milestones = data.get("milestones") or []
        if arguments.milestone is None and not milestones:
            raise MapInputError("the map lists no milestones")
        milestone = arguments.milestone if arguments.milestone is not None else milestones[0]
        report = check_map(root, data, milestone, arguments.item)
    except MapInputError as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 2
    for line in report.lines:
        print(line)
    print(report.summary())
    return 0 if report.failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())

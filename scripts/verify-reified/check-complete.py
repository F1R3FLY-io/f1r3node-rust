#!/usr/bin/env python3
"""Check that a Reified RSpaces phase is complete, without git.

The checker reads `complete-policies.json` and runs in one of two modes.

`--phase NAME` checks one phase and verifies that:

* every required entry point exists in its source file;
* the non-test part of each source file, comments included, has no forbidden
  token, and every `.expect(...)` call there is a lock expect;
* no source file contains a consensus-checklist token anywhere;
* the phase's test files do not use `/tmp`, contain no `#[ignore]`, and every
  property file sets `failure_persistence: None`.

`--production-scan` is the grep gate of the plan. It reads the section
`production_scan`. The scan always reads a root that names a file. Under a root
that names a directory, it reads each file with a listed suffix. Each line that
holds a listed token is a finding. A missing root is also a finding.

Each mode prints one line per finding and a summary. It exits 0 only when it
finds nothing.
"""

import argparse
import json
import os
import re
import sys
from pathlib import Path

POLICY_FILE = Path("scripts/verify-reified/complete-policies.json")


def split_regions(text, test_start):
    """Return (non_test_lines, all_lines) as lists of (line_number, text)."""
    lines = text.split("\n")
    numbered = list(enumerate(lines, start=1))
    boundary = len(lines)
    pattern = re.compile(test_start)
    for index, line in numbered:
        if pattern.match(line):
            boundary = index - 1
            break
    return numbered[:boundary], numbered


def check_phase(root, policies, name):
    phase = policies["phases"].get(name)
    if phase is None:
        raise SystemExit(f"unknown phase: {name}")
    allowed_expect = re.compile(policies["allowed_expect"])
    findings = []
    checks = 0

    for entry in phase["entry_points"]:
        checks += 1
        path = root / entry["file"]
        if not path.is_file() or entry["pattern"] not in path.read_text(encoding="utf-8"):
            findings.append(f"{entry['file']}: missing entry point {entry['pattern']!r}")

    for source in phase["sources"]:
        path = root / source
        checks += 1
        if not path.is_file():
            findings.append(f"{source}: source file is missing")
            continue
        non_test, all_lines = split_regions(path.read_text(encoding="utf-8"), policies["test_region_start"])
        for number, line in non_test:
            for rule in policies["forbidden_non_test"]:
                checks += 1
                if rule["pattern"] in line:
                    findings.append(f"{source}:{number}: forbidden {rule['pattern']!r} ({rule['reason']})")
            if ".expect(" in line:
                checks += 1
                for call in re.finditer(r"\.expect\([^)]*\)", line):
                    if not allowed_expect.fullmatch(call.group(0)):
                        findings.append(f"{source}:{number}: expect is not a lock expect: {call.group(0)}")
        for number, line in all_lines:
            for rule in policies["forbidden_anywhere"]:
                checks += 1
                if rule["pattern"] in line:
                    findings.append(f"{source}:{number}: consensus token {rule['pattern']!r} ({rule['reason']})")

    for test in phase["tests"]:
        path = root / test
        checks += 1
        if not path.is_file():
            findings.append(f"{test}: test file is missing")
            continue
        text = path.read_text(encoding="utf-8")
        for number, line in enumerate(text.split("\n"), start=1):
            if "/tmp" in line:
                findings.append(f"{test}:{number}: test refers to /tmp")
            if "#[ignore" in line:
                findings.append(f"{test}:{number}: test is ignored")

    for prop in phase["property_files"]:
        path = root / prop
        checks += 1
        if not path.is_file() or "failure_persistence: None" not in path.read_text(encoding="utf-8"):
            findings.append(f"{prop}: property file does not set failure_persistence: None")

    for finding in findings:
        print(f"complete: {finding}")
    print(f"complete phase {name}: {checks} checks, {len(findings)} findings")
    return 0 if not findings else 1


def production_files(root, relative_root, suffixes):
    """Return the sorted files that the scan reads under one root, or None when the root is missing."""
    path = root / relative_root
    if path.is_file():
        return [relative_root]
    if not path.is_dir():
        return None
    found = []
    for current, _, names in os.walk(path):
        found.extend(
            (Path(current) / name).relative_to(root).as_posix() for name in names if name.endswith(suffixes)
        )
    return sorted(found)


def production_scan(root, policies):
    """Scan the production roots for consensus tokens and return the exit code."""
    section = policies.get("production_scan")
    if section is None:
        raise SystemExit("complete-policies.json has no production_scan section")
    suffixes = tuple(section["suffixes"])
    patterns = section["patterns"]
    findings = []
    scanned = 0
    for relative_root in section["roots"]:
        files = production_files(root, relative_root, suffixes)
        if files is None:
            findings.append(f"{relative_root}: production root is missing")
            continue
        for relative in files:
            scanned += 1
            text = (root / relative).read_text(encoding="utf-8", errors="replace")
            present = [rule for rule in patterns if rule["pattern"] in text]
            if not present:
                continue
            for number, line in enumerate(text.split("\n"), start=1):
                for rule in present:
                    if rule["pattern"] in line:
                        findings.append(
                            f"{relative}:{number}: consensus token {rule['pattern']!r} ({rule['reason']})"
                        )
    for finding in findings:
        print(f"complete: {finding}")
    print(f"complete production scan: {scanned} files, {len(findings)} findings")
    return 0 if not findings else 1


def main():
    parser = argparse.ArgumentParser(
        description="Check that a Reified RSpaces phase is complete, or scan production code for consensus tokens."
    )
    parser.add_argument("--root", default=".", help="worktree root (default: current directory)")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--phase", help="phase to check, for example p1")
    mode.add_argument(
        "--production-scan", action="store_true", help="scan every production root for consensus tokens"
    )
    arguments = parser.parse_args()
    root = Path(arguments.root).resolve()
    with open(root / POLICY_FILE, encoding="utf-8") as handle:
        policies = json.load(handle)
    if arguments.production_scan:
        return production_scan(root, policies)
    return check_phase(root, policies, arguments.phase)


if __name__ == "__main__":
    sys.exit(main())

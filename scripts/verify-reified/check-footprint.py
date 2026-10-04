#!/usr/bin/env python3
"""Check the Reified RSpaces source footprint against digests pinned at the base revision.

The pgmcp verifier snapshot has no `.git`, so footprint gates cannot use `git diff`.
This checker compares files with SHA-256 digests pinned from the base revision:

* `--pin` (host only, needs git) writes the digest file named by the policy file.
* `--policy NAME` checks one policy and exits 0 only when every rule holds.

A policy lists globs whose pinned files must be byte-identical, globs in which no
new file may appear, and approved edits.  An approved edit must undo exactly: each
added line is removed, and each replaced line is restored.  The result must have the
pinned digest, so the edit is the only change in that file.
"""

import argparse
import fnmatch
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

POLICY_FILE = Path("scripts/verify-reified/footprint-policies.json")


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def load_policies(root):
    with open(root / POLICY_FILE, encoding="utf-8") as handle:
        return json.load(handle)


def resolve_policy(policies, name):
    table = policies["policies"]
    if name not in table:
        raise SystemExit(f"unknown policy: {name}")
    chain = []
    current = name
    while current is not None:
        if current in chain:
            raise SystemExit(f"policy inheritance cycle at {current}")
        chain.append(current)
        current = table[current].get("extends")
    unchanged, no_new_files, edits = [], [], []
    for policy_name in reversed(chain):
        policy = table[policy_name]
        unchanged.extend(policy.get("unchanged", []))
        no_new_files.extend(policy.get("no_new_files", []))
        edits.extend(policy.get("edits", []))
    return unchanged, no_new_files, edits


def matches_any(path, globs):
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in globs)


def pin(root, policies):
    base = policies["base_revision"]
    listing = subprocess.run(
        ["git", "-C", str(root), "ls-tree", "-r", "--name-only", base, "--", *policies["pinned_roots"]],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.splitlines()
    digests = {}
    for path in sorted(listing):
        blob = subprocess.run(
            ["git", "-C", str(root), "cat-file", "blob", f"{base}:{path}"],
            check=True,
            capture_output=True,
        ).stdout
        digests[path] = sha256_bytes(blob)
    document = {"base_revision": base, "roots": policies["pinned_roots"], "files": digests}
    target = root / policies["digests"]
    target.parent.mkdir(parents=True, exist_ok=True)
    with open(target, "w", encoding="utf-8") as handle:
        json.dump(document, handle, indent=2, sort_keys=True)
        handle.write("\n")
    print(f"pinned {len(digests)} files at {base} into {policies['digests']}")


def undo_edit(text, edit):
    lines = text.split("\n")
    problems = []
    for added in edit.get("added_lines", []):
        hits = [index for index, line in enumerate(lines) if line == added]
        if len(hits) != 1:
            problems.append(f"added line {added!r} occurs {len(hits)} times, expected 1")
            continue
        del lines[hits[0]]
    for old, new in edit.get("replaced_lines", []):
        hits = [index for index, line in enumerate(lines) if line == new]
        if len(hits) != 1:
            problems.append(f"replacement line {new!r} occurs {len(hits)} times, expected 1")
            continue
        lines[hits[0]] = old
    return "\n".join(lines), problems


def walk_files(root, directory):
    base = root / directory
    if not base.is_dir():
        return []
    found = []
    for current, _, names in os.walk(base):
        for name in names:
            found.append((Path(current) / name).relative_to(root).as_posix())
    return found


def check(root, policies, name):
    with open(root / policies["digests"], encoding="utf-8") as handle:
        pinned = json.load(handle)["files"]
    unchanged, no_new_files, edits = resolve_policy(policies, name)
    edited_paths = {edit["path"] for edit in edits}
    violations = []
    checked = 0
    for path, digest in sorted(pinned.items()):
        if path in edited_paths or not matches_any(path, unchanged):
            continue
        checked += 1
        file_path = root / path
        if not file_path.is_file():
            violations.append(f"{path}: pinned file is missing")
            continue
        if sha256_bytes(file_path.read_bytes()) != digest:
            violations.append(f"{path}: content differs from the base revision")
    directories = sorted({pattern[:-3] for pattern in no_new_files if pattern.endswith("/**")})
    for directory in directories:
        for path in walk_files(root, directory):
            if path not in pinned and matches_any(path, no_new_files) and path not in edited_paths:
                violations.append(f"{path}: new file in a protected directory")
    for edit in edits:
        path = edit["path"]
        checked += 1
        if path not in pinned:
            violations.append(f"{path}: an approved edit names a file that is not pinned")
            continue
        file_path = root / path
        if not file_path.is_file():
            violations.append(f"{path}: edited file is missing")
            continue
        restored, problems = undo_edit(file_path.read_text(encoding="utf-8"), edit)
        violations.extend(f"{path}: {problem}" for problem in problems)
        if not problems and sha256_bytes(restored.encode("utf-8")) != pinned[path]:
            violations.append(f"{path}: the file holds changes beyond the approved edit")
    for violation in violations:
        print(f"footprint: {violation}")
    print(f"footprint policy {name}: checked {checked} files, {len(violations)} violations")
    return 0 if not violations else 1


def main():
    parser = argparse.ArgumentParser(
        description="Check the Reified RSpaces source footprint against pinned digests."
    )
    parser.add_argument("--root", default=".", help="worktree root (default: current directory)")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--pin", action="store_true", help="pin digests from the base revision (needs git)")
    mode.add_argument("--policy", help="policy to check")
    arguments = parser.parse_args()
    root = Path(arguments.root).resolve()
    policies = load_policies(root)
    if arguments.pin:
        pin(root, policies)
        return 0
    return check(root, policies, arguments.policy)


if __name__ == "__main__":
    sys.exit(main())

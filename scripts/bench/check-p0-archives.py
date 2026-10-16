#!/usr/bin/env python3
"""Check the digests of the restored P0 benchmark archives (P10 step S8, design v2 section 2.8).

P10 restored the P0 benchmark archives from the backup of 2026-10-04 and replaced the workspace
path of the capture host in their logs. test-vectors/benchmarks/p0-archives-provenance.json maps
the original digest of each archive, which its summary JSON records, to the digest of the
committed archive. The checker proves:

1. each archive has its redacted digest;
2. its summary JSON records its original digest;
3. each changed member of the archive has its redacted digest;
4. no member of an archive holds a home directory path.

Usage: check-p0-archives.py [--repo-root DIR]
Exit codes: 0 every check passes, 1 a check fails, 2 the provenance file cannot be read.
Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tarfile
from pathlib import Path

DEFAULT_ROOT = Path(__file__).resolve().parents[2]
PROVENANCE = Path("test-vectors/benchmarks/p0-archives-provenance.json")
SCHEMA = "reified-rspaces-p0-archive-provenance/1"
HOME_MARKERS = (b"/home/", b"/Users/")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def strings(value) -> set[str]:
    """Every string of a JSON value, at any depth."""
    if isinstance(value, str):
        return {value}
    if isinstance(value, dict):
        return set().union(*(strings(v) for v in value.values())) if value else set()
    if isinstance(value, list):
        return set().union(*(strings(v) for v in value)) if value else set()
    return set()


def check(root: Path) -> list[str]:
    problems: list[str] = []
    data = json.loads((root / PROVENANCE).read_text(encoding="utf-8"))
    if data.get("schema") != SCHEMA:
        return [f"{PROVENANCE}: the schema is not {SCHEMA}"]
    for entry in data["archives"]:
        archive = root / entry["archive"]
        try:
            raw = archive.read_bytes()
        except OSError:
            problems.append(f"{entry['archive']} is missing")
            continue
        if sha256(raw) != entry["redacted_sha256"]:
            problems.append(f"{entry['archive']}: the digest {sha256(raw)} is not {entry['redacted_sha256']}")
        summary = json.loads((root / entry["summary"]).read_text(encoding="utf-8"))
        if entry["original_sha256"] not in strings(summary):
            problems.append(f"{entry['summary']} does not record the original digest of {entry['archive']}")
        changed = {m["member"]: m for m in entry["changed_members"]}
        with tarfile.open(archive, "r:gz") as tar:
            for member in tar.getmembers():
                if not member.isfile():
                    continue
                content = tar.extractfile(member)
                body = content.read() if content is not None else b""
                if any(marker in body for marker in HOME_MARKERS):
                    problems.append(f"{entry['archive']}: {member.name} holds a home directory path")
                if member.name in changed and sha256(body) != changed[member.name]["redacted_sha256"]:
                    problems.append(f"{entry['archive']}: {member.name} has not its redacted digest")
                changed.pop(member.name, None)
        for name in changed:
            problems.append(f"{entry['archive']}: the changed member {name} is missing")
    return problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n", 1)[0])
    parser.add_argument("--repo-root", type=Path, default=DEFAULT_ROOT)
    args = parser.parse_args(argv)
    try:
        problems = check(args.repo_root.resolve())
    except (OSError, json.JSONDecodeError, KeyError) as error:
        print(f"check-p0-archives: cannot read the provenance: {error}")
        return 2
    for problem in problems:
        print(f"FAIL {problem}")
    print(f"check-p0-archives: FAIL={len(problems)}")
    return 0 if not problems else 1


if __name__ == "__main__":
    sys.exit(main())

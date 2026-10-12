"""Check the test-port ledger of the legacy Reified RSpaces tests (plan section "Tests").

The inventory `test-vectors/legacy-test-inventory-ff479042.json` lists every test that the legacy
stack 5c0363d9..ff479042 added (`legacy-test-inventory.py` writes it on the host). The ledger
`docs/rspace/reified-rspaces/verification/test-port-ledger.json` has one row per inventory
entry. The checker needs no legacy checkout, so it runs in the verification sandbox.

Rules:

1. The ledger covers the inventory one to one: each entry has exactly one row, and each row
   names an entry.
2. Each row has a `behavior`, a `disposition` (port, retarget or drop) and at least one
   successor. A successor is `{"file": ..., "test": ...}`, and the file must define a function
   of exactly that name (`fn`, `async fn`, with `pub` or not).
3. A drop has a `reason` that names a decision or a plan section.
4. A row with `"positive_match": true` names at least one `non_match` test, which must exist
   like a successor (plan rule: every WildcardMatch-era positive test gains a non-match
   counterpart).

The report counts the rows of each disposition and lists the successors that serve the most
rows.

Usage: python3 scripts/verify-reified/check-test-port-ledger.py [--top N]
"""
import json
import re
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
INVENTORY = ROOT / "test-vectors" / "legacy-test-inventory-ff479042.json"
LEDGER = ROOT / "docs" / "rspace" / "reified-rspaces" / "verification" / "test-port-ledger.json"
DISPOSITIONS = ("port", "retarget", "drop")
REASON = re.compile(r"(decision \d+|[Dd]ecision-\d+|plan section|I-\d+|M-\d+|F\d+)")


def key(entry):
    return (entry["path"], entry["module"], entry["name"])


def defines(path, name, cache):
    if path not in cache:
        file = ROOT / path
        cache[path] = file.read_text(encoding="utf-8") if file.is_file() else None
    text = cache[path]
    if text is None:
        return False
    pattern = re.compile(r"(^|\s)(pub(\([a-z]+\))?\s+)?(async\s+)?fn\s+" + re.escape(name) + r"\s*[<(]",
                         re.MULTILINE)
    return pattern.search(text) is not None


def main():
    top = 10
    if "--top" in sys.argv:
        top = int(sys.argv[sys.argv.index("--top") + 1])
    findings = []
    inventory = json.loads(INVENTORY.read_text(encoding="utf-8"))["tests"]
    if not LEDGER.is_file():
        print(f"ledger: {LEDGER.relative_to(ROOT)} is missing")
        print("test-port ledger: FAIL")
        return 1
    rows = json.loads(LEDGER.read_text(encoding="utf-8"))["rows"]
    entries = {key(entry) for entry in inventory}
    seen = Counter(key(row) for row in rows)
    for entry_key in sorted(entries):
        if seen[entry_key] != 1:
            findings.append(f"{'::'.join(entry_key)}: {seen[entry_key]} rows, expected 1")
    for row_key in sorted(set(seen) - entries):
        findings.append(f"{'::'.join(row_key)}: row names no inventory entry")
    cache = {}
    dispositions = Counter()
    successors = Counter()
    for row in rows:
        label = "::".join(key(row))
        disposition = row.get("disposition")
        dispositions[disposition] += 1
        if disposition not in DISPOSITIONS:
            findings.append(f"{label}: disposition {disposition!r} is not one of {DISPOSITIONS}")
        if not row.get("behavior"):
            findings.append(f"{label}: no behavior")
        named = row.get("successors") or []
        if not named:
            findings.append(f"{label}: no successor")
        for successor in named:
            if not defines(successor["file"], successor["test"], cache):
                findings.append(f"{label}: successor {successor['file']}::{successor['test']} does not exist")
            successors[(successor["file"], successor["test"])] += 1
        if disposition == "drop" and not REASON.search(row.get("reason") or ""):
            findings.append(f"{label}: a drop needs a reason that names a decision or a plan section")
        if row.get("positive_match"):
            counterparts = row.get("non_match") or []
            if not counterparts:
                findings.append(f"{label}: a positive matcher test needs a non_match counterpart")
            for counterpart in counterparts:
                if not defines(counterpart["file"], counterpart["test"], cache):
                    findings.append(f"{label}: non_match {counterpart['file']}::{counterpart['test']} does not exist")
    for finding in findings:
        print(f"ledger: {finding}")
    print(f"test-port ledger: {len(inventory)} inventory entries, {len(rows)} rows, "
          f"dispositions {dict(sorted(dispositions.items(), key=lambda item: str(item[0])))}, "
          f"{len(successors)} distinct successors")
    for (file, test), count in successors.most_common(top):
        print(f"  successor of {count} rows: {file}::{test}")
    print("test-port ledger: " + ("PASS" if not findings else f"FAIL ({len(findings)} findings)"))
    return 0 if not findings else 1


if __name__ == "__main__":
    sys.exit(main())

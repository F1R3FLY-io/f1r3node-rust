"""List the tests that the legacy Reified RSpaces stack added (plan section "Tests", legacy port).

The legacy stack is the range 5c0363d9..ff479042 of the legacy f1r3node checkout. The script
reads every Rust file that the range adds or changes, at both commits, and lists each test
function of the file at ff479042 that the file at 5c0363d9 does not hold:

* a function under `#[test]` or `#[tokio::test]`, possibly after other attributes;
* a function under `#[test]` inside a `proptest!` block.

A test is identified by its file, its module path inside the file and its name. The module path
comes from the `mod name {` blocks around the function, so two tests with one name in two modules
of one file stay distinct. The output is the inventory that the test-port ledger covers one to
one (`check-test-port-ledger.py`).

Usage (host only, it needs the legacy checkout):

    python3 scripts/verify-reified/legacy-test-inventory.py LEGACY_CHECKOUT [--write]

Without `--write` it prints the counts. With `--write` it writes
`test-vectors/legacy-test-inventory-ff479042.json`.
"""
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

BASE = "5c0363d9"
HEAD = "ff479042"
ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "test-vectors" / f"legacy-test-inventory-{HEAD}.json"

TEST_ATTRIBUTE = re.compile(r"^\s*#\[(tokio::)?test(\]|\()")
OTHER_ATTRIBUTE = re.compile(r"^\s*#\[")
FUNCTION = re.compile(r"^\s*(pub(\([a-z]+\))?\s+)?(async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")
MODULE = re.compile(r"^\s*(pub(\([a-z]+\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{")
PROPTEST = re.compile(r"^\s*proptest!\s*[\{\(]")


def git(checkout, *args):
    return subprocess.run(["git", "-C", str(checkout), *args], check=True,
                          capture_output=True, text=True).stdout


def strip_strings_and_comments(line):
    """The line without string literals, char literals and line comments, for brace counting."""
    line = re.sub(r'"(\\.|[^"\\])*"', '""', line)
    line = re.sub(r"'(\\.|[^'\\])'", "''", line)
    return line.split("//", 1)[0]


def tests_of(text):
    """Return the tests of one Rust source text as (module path, name, kind, line) tuples."""
    tests = []
    modules = []  # (name, depth at which the module's block opened)
    depth = 0
    pending_attribute = None  # "test" or "tokio" when a test attribute waits for its fn
    proptest_depths = []
    in_block_comment = False
    for number, raw in enumerate(text.split("\n"), start=1):
        line = raw
        if in_block_comment:
            if "*/" in line:
                line = line.split("*/", 1)[1]
                in_block_comment = False
            else:
                continue
        if "/*" in line and "*/" not in line.split("/*", 1)[1]:
            line = line.split("/*", 1)[0]
            in_block_comment = True
        attribute = TEST_ATTRIBUTE.match(line)
        if attribute:
            pending_attribute = "tokio" if attribute.group(1) else "test"
        elif pending_attribute and not OTHER_ATTRIBUTE.match(line) and line.strip():
            function = FUNCTION.match(line)
            if function:
                kind = "proptest" if proptest_depths else pending_attribute
                path = "::".join(name for name, _ in modules)
                tests.append((path, function.group(4), kind, number))
            pending_attribute = None
        module = MODULE.match(line)
        code = strip_strings_and_comments(line)
        if PROPTEST.match(line):
            proptest_depths.append(depth)
        opens, closes = code.count("{"), code.count("}")
        if module:
            modules.append((module.group(3), depth))
        depth += opens - closes
        while modules and depth <= modules[-1][1]:
            modules.pop()
        while proptest_depths and depth <= proptest_depths[-1]:
            proptest_depths.pop()
    return tests


def inventory(checkout):
    changed = git(checkout, "diff", "--name-only", "--diff-filter=AM", BASE, HEAD, "--", "*.rs").split()
    entries = []
    for path in sorted(changed):
        head_text = git(checkout, "show", f"{HEAD}:{path}")
        try:
            base_text = git(checkout, "show", f"{BASE}:{path}")
        except subprocess.CalledProcessError:
            base_text = ""
        base_tests = {(module, name) for module, name, _, _ in tests_of(base_text)}
        digest = hashlib.sha256(head_text.encode("utf-8")).hexdigest()
        for module, name, kind, line in tests_of(head_text):
            if (module, name) in base_tests:
                continue
            entries.append({"path": path, "module": module, "name": name, "kind": kind,
                            "line": line, "file_sha256": digest})
    return entries


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    checkout = Path(sys.argv[1])
    entries = inventory(checkout)
    keys = [(e["path"], e["module"], e["name"]) for e in entries]
    duplicates = len(keys) - len(set(keys))
    kinds = {}
    for entry in entries:
        kinds[entry["kind"]] = kinds.get(entry["kind"], 0) + 1
    files = len({entry["path"] for entry in entries})
    print(f"legacy tests added by {BASE}..{HEAD}: {len(entries)} in {files} files, kinds {kinds}, "
          f"duplicate identities {duplicates}")
    if "--write" in sys.argv[2:]:
        document = {"base": BASE, "head": HEAD, "tests": entries}
        OUTPUT.write_text(json.dumps(document, indent=2, ensure_ascii=False) + "\n")
        print(f"wrote {OUTPUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()

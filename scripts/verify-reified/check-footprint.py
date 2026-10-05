#!/usr/bin/env python3
"""Check the Reified RSpaces source footprint against digests pinned at the base revision.

The pgmcp verifier snapshot has no `.git`, so footprint gates cannot use `git diff`.
This checker compares files with SHA-256 digests pinned from the base revision:

* `--pin` (host only, needs git) writes the digest file named by the policy file.
  One `git ls-tree` process lists the base tree, and one `git cat-file --batch`
  process reads every blob. Each pinned entry must be a regular file. A symbolic
  link, a submodule or any other object type stops the pin with an error.
* `--policy NAME` checks one policy and exits 0 only when every rule holds.

A policy lists globs whose pinned files must be byte-identical, globs in which no
new file may appear, and approved edits.  An approved edit must undo exactly: each
added line is removed, each replaced line is restored, and each replaced block is
restored.  The result must have the pinned digest, so the edit is the only change in
that file.

An edit takes four kinds of change:

* `added_lines`: each line occurs exactly once in the current file.
* `added_line_counts`: `[line, count]` pairs, where `count` is a positive integer.
  Each line occurs exactly `count` times in the current file, and the undo removes
  every occurrence. The digest comparison then proves that the base revision did not
  hold the line. A counted line lets one policy approve identical lines, for example
  one bound line in several `where` clauses.
* `replaced_lines`: `[old, new]` pairs. Each new line occurs exactly once in the
  current file.
* `replaced_blocks`: `[[old lines...], [new lines...]]` pairs of non-empty line
  lists. Each new block occurs exactly once in the current file, and each old block
  occurs exactly once in the restored file. A block can thus replace a multi-line
  construct, or add lines next to a unique anchor (an old block of the anchor lines
  and a new block of the anchor lines with the added lines).

The undo removes the added lines first, then removes the counted added lines, then
restores the replaced lines, and then restores the replaced blocks.

A policy can also list `allowed_new_files` as exact paths. A policy inherits this
list from the policy that it extends. When the list exists, two more rules apply.
Each file of the file list that is not pinned must be in `allowed_new_files`. Each
entry of `allowed_new_files` must be a file of the file list. An exact path of the
list also exempts that one file from the `no_new_files` globs of the chain: the
policy names the file explicitly. check-consensus-checklist.py fails a checklist
item when a protected glob of the item holds such an exception.

The file list holds the regular files of the tree. When `<root>/.git` exists, the
command `git ls-files --cached --others --exclude-standard` gives the list.
Otherwise a walk of the tree gives the list, and the walk skips each `.git` entry.
The pgmcp snapshot holds the files of the git list and no `.git`. Thus the walk of
the snapshot gives the same list as git gives on the host.
"""

import argparse
import fnmatch
import hashlib
import json
import os
import stat
import subprocess
import sys
from pathlib import Path

POLICY_FILE = Path("scripts/verify-reified/footprint-policies.json")
REGULAR_FILE_MODES = (b"100644", b"100755")
GIT_LOCATION_VARIABLES = (
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_WORK_TREE",
)


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def load_policies(root):
    with open(root / POLICY_FILE, encoding="utf-8") as handle:
        return json.load(handle)


def policy_chain(policies, name):
    """Return the names of NAME and of each policy that it extends, child first."""
    table = policies["policies"]
    chain = []
    current = name
    while current is not None:
        if current not in table:
            raise SystemExit(f"unknown policy: {current}")
        if current in chain:
            raise SystemExit(f"policy inheritance cycle at {current}")
        chain.append(current)
        current = table[current].get("extends")
    return chain


def resolve_policy(policies, name):
    table = policies["policies"]
    unchanged, no_new_files, edits = [], [], []
    for policy_name in reversed(policy_chain(policies, name)):
        policy = table[policy_name]
        unchanged.extend(policy.get("unchanged", []))
        no_new_files.extend(policy.get("no_new_files", []))
        edits.extend(policy.get("edits", []))
    return unchanged, no_new_files, edits


def resolve_allowed_new_files(policies, name):
    """Return the exact new paths that the policy chain allows.

    The result is None when no policy of the chain sets `allowed_new_files`.
    """
    table = policies["policies"]
    allowed = None
    for policy_name in reversed(policy_chain(policies, name)):
        entries = table[policy_name].get("allowed_new_files")
        if entries is None:
            continue
        if not isinstance(entries, list) or not all(isinstance(entry, str) for entry in entries):
            raise SystemExit(f"policy {policy_name}: allowed_new_files must be a list of paths")
        allowed = (allowed or []) + entries
    return allowed


def matches_any(path, globs):
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in globs)


def git_environment():
    """Return the environment for git, without the variables that move the repository."""
    environment = dict(os.environ)
    for variable in GIT_LOCATION_VARIABLES:
        environment.pop(variable, None)
    return environment


def run_git(root, arguments):
    """Run one git command in ROOT and return its standard output as bytes."""
    try:
        result = subprocess.run(
            ["git", "-C", str(root), *arguments],
            capture_output=True,
            env=git_environment(),
            check=False,
        )
    except OSError as error:
        raise SystemExit(f"cannot run git: {error}") from error
    if result.returncode != 0:
        message = result.stderr.decode("utf-8", "replace").strip()
        raise SystemExit(f"git {arguments[0]} failed with exit code {result.returncode}: {message}")
    return result.stdout


def base_tree_blobs(root, base, roots):
    """Return {path: object id} for the files under ROOTS at the revision BASE.

    Each entry must be a regular file. Any other entry stops the pin with an error.
    """
    listing = run_git(root, ["ls-tree", "-r", "-z", "--full-tree", base, "--", *roots])
    blobs = {}
    for record in listing.split(b"\0"):
        if not record:
            continue
        header, separator, raw_path = record.partition(b"\t")
        fields = header.split(b" ")
        if not separator or len(fields) != 3:
            raise SystemExit(f"--pin: git ls-tree wrote a record that the checker cannot parse: {record!r}")
        mode, kind, object_id = fields
        path = os.fsdecode(raw_path)
        if kind != b"blob" or mode not in REGULAR_FILE_MODES:
            raise SystemExit(
                f"--pin: {path} at {base} is a {kind.decode('ascii', 'replace')} with mode "
                f"{mode.decode('ascii', 'replace')}, not a regular file"
            )
        blobs[path] = object_id
    return blobs


def read_blob_digests(root, blobs):
    """Return {path: SHA-256 of its blob}. One `git cat-file --batch` process reads every blob.

    The checker sends one object id at a time and reads the reply before it sends the next.
    git flushes its output after each object, so the two pipes never block each other.
    """
    try:
        process = subprocess.Popen(
            ["git", "-C", str(root), "cat-file", "--batch"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            env=git_environment(),
        )
    except OSError as error:
        raise SystemExit(f"--pin: cannot run git: {error}") from error
    requests, replies = process.stdin, process.stdout
    digests = {}
    try:
        if requests is None or replies is None:
            raise SystemExit("--pin: git cat-file started without its input and output pipes")
        for path, object_id in sorted(blobs.items()):
            try:
                requests.write(object_id + b"\n")
                requests.flush()
            except BrokenPipeError as error:
                raise SystemExit(f"--pin: git cat-file stopped before it read {path}") from error
            header = replies.readline().split()
            if len(header) != 3 or header[0] != object_id or header[1] != b"blob":
                raise SystemExit(f"--pin: git cat-file returned {b' '.join(header)!r} for {path}, not its blob")
            size = int(header[2])
            content = replies.read(size)
            if len(content) != size or replies.read(1) != b"\n":
                raise SystemExit(f"--pin: git cat-file returned a short blob for {path}")
            digests[path] = sha256_bytes(content)
        requests.close()
        if replies.read():
            raise SystemExit("--pin: git cat-file wrote output that the checker did not request")
        if process.wait() != 0:
            raise SystemExit(f"--pin: git cat-file failed with exit code {process.returncode}")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        for stream in (requests, replies):
            if stream is not None and not stream.closed:
                stream.close()
    return digests


def pin(root, policies):
    base = policies["base_revision"]
    roots = policies["pinned_roots"]
    blobs = base_tree_blobs(root, base, roots)
    if not blobs:
        raise SystemExit(f"--pin: the roots {roots} name no file at {base}")
    digests = read_blob_digests(root, blobs)
    document = {"base_revision": base, "roots": roots, "files": digests}
    target = root / policies["digests"]
    target.parent.mkdir(parents=True, exist_ok=True)
    with open(target, "w", encoding="utf-8") as handle:
        json.dump(document, handle, indent=2, sort_keys=True)
        handle.write("\n")
    print(f"pinned {len(digests)} files at {base} into {policies['digests']}")


def is_regular_file(path):
    """Return True when PATH is a regular file. A symbolic link is not a regular file."""
    try:
        return stat.S_ISREG(os.lstat(path).st_mode)
    except OSError:
        return False


def list_tree_files(root):
    """Return the sorted relative paths of the regular files of the tree (see the module text)."""
    if (root / ".git").exists():
        output = run_git(root, ["ls-files", "--cached", "--others", "--exclude-standard", "-z"])
        candidates = {os.fsdecode(entry) for entry in output.split(b"\0") if entry}
    else:
        candidates = set()
        for current, directories, names in os.walk(root):
            directories[:] = [name for name in directories if name != ".git"]
            relative = Path(current).relative_to(root)
            candidates.update((relative / name).as_posix() for name in names if name != ".git")
    return sorted(path for path in candidates if is_regular_file(root / path))


def block_hits(lines, block):
    """Return the start index of each occurrence of the line list BLOCK in LINES."""
    width = len(block)
    return [index for index in range(len(lines) - width + 1) if lines[index:index + width] == block]


def is_line_block(block):
    return isinstance(block, list) and bool(block) and all(isinstance(line, str) for line in block)


def is_line_count(entry):
    """Return True for a `[line, count]` pair of a line and a positive integer count."""
    return (
        isinstance(entry, list)
        and len(entry) == 2
        and isinstance(entry[0], str)
        and isinstance(entry[1], int)
        and not isinstance(entry[1], bool)
        and entry[1] >= 1
    )


def undo_edit(text, edit):
    lines = text.split("\n")
    problems = []
    for added in edit.get("added_lines", []):
        hits = [index for index, line in enumerate(lines) if line == added]
        if len(hits) != 1:
            problems.append(f"added line {added!r} occurs {len(hits)} times, expected 1")
            continue
        del lines[hits[0]]
    for entry in edit.get("added_line_counts", []):
        if not is_line_count(entry):
            problems.append(f"added line count {entry!r} is not a [line, count] pair with a positive count")
            continue
        added, count = entry
        hits = sum(1 for line in lines if line == added)
        if hits != count:
            problems.append(f"added line {added!r} occurs {hits} times, expected {count}")
            continue
        lines = [line for line in lines if line != added]
    for old, new in edit.get("replaced_lines", []):
        hits = [index for index, line in enumerate(lines) if line == new]
        if len(hits) != 1:
            problems.append(f"replacement line {new!r} occurs {len(hits)} times, expected 1")
            continue
        lines[hits[0]] = old
    blocks = edit.get("replaced_blocks", [])
    restored_blocks = []
    for pair in blocks:
        if not (isinstance(pair, list) and len(pair) == 2 and all(is_line_block(block) for block in pair)):
            problems.append(f"replaced block {pair!r} is not a pair of non-empty line lists")
            continue
        old, new = pair
        hits = block_hits(lines, new)
        if len(hits) != 1:
            problems.append(f"replacement block starting {new[0]!r} occurs {len(hits)} times, expected 1")
            continue
        lines[hits[0]:hits[0] + len(new)] = old
        restored_blocks.append(old)
    for old in restored_blocks:
        count = len(block_hits(lines, old))
        if count != 1:
            problems.append(f"original block starting {old[0]!r} occurs {count} times in the restored file, expected 1")
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
    allowed_new_files = resolve_allowed_new_files(policies, name)
    exempt_new_files = set(allowed_new_files or [])
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
            if (
                path not in pinned
                and matches_any(path, no_new_files)
                and path not in edited_paths
                and path not in exempt_new_files
            ):
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
    if allowed_new_files is not None:
        listed = list_tree_files(root)
        allowed = set(allowed_new_files)
        new_files = [path for path in listed if path not in pinned]
        checked += len(new_files)
        violations.extend(f"{path}: new file outside the allowed list" for path in new_files if path not in allowed)
        present = set(listed)
        violations.extend(f"{path}: allowed new file is missing" for path in sorted(allowed) if path not in present)
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

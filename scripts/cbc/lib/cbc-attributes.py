#!/usr/bin/env python3
"""Route CbC authoring and inspect policy through Git's native resolver."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tempfile
from typing import Any

ATTRIBUTES = ("cbc", "cbc-weight", "cbc-acceptance", "cbc-evidence")
SCHEMA = "cbc-attributes/v1"


def git(root: Path, *args: str, input: bytes | None = None,
        optional: bool = False, isolated: bool = False) -> subprocess.CompletedProcess:
    env = dict(os.environ)
    if isolated:
        for key in list(env):
            if key.startswith("GIT_"):
                del env[key]
        env["GIT_ATTR_NOSYSTEM"] = "1"
        env["GIT_CONFIG_NOSYSTEM"] = "1"
        env["GIT_CONFIG_GLOBAL"] = os.devnull
    result = subprocess.run(["git", "-C", str(root), *args], input=input,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    if result.returncode and not optional:
        raise ValueError("Git attribute operation failed.")
    return result


def relative(root: Path, value: str) -> PurePosixPath:
    if not isinstance(value, str) or not value or "\0" in value:
        raise ValueError("A repository-relative path is required.")
    path = PurePosixPath(value)
    if path.is_absolute() or ".." in path.parts or ".git" in path.parts:
        raise ValueError("Paths must remain inside the repository and outside Git metadata.")
    if os.name == "nt" and "\\" in value:
        raise ValueError("Paths must remain inside the repository and outside Git metadata.")
    candidate = root / path
    current = root
    for part in path.parts:
        current /= part
        if current.is_symlink():
            raise ValueError("Attribute authoring does not follow symlinks.")
    candidate.resolve().relative_to(root)
    return path


def values(root: Path, paths: list[str], cached: bool = False,
           isolated: bool = False, attributes: tuple[str, ...] | None = ATTRIBUTES) -> dict[str, dict[str, str]]:
    if not paths:
        return {}
    stdin = b"".join(os.fsencode(path) + b"\0" for path in paths)
    args = ["check-attr", "-z", "--stdin"]
    if cached:
        args.append("--cached")
    selected = ["--all"] if attributes is None else attributes
    output = git(root, *args, *selected, input=stdin, isolated=isolated).stdout.split(b"\0")
    if output[-1] != b"" or (len(output) - 1) % 3:
        raise ValueError("Git returned an invalid attribute result.")
    result = {path: {} for path in paths}
    for offset in range(0, len(output) - 1, 3):
        path, attribute, value = map(os.fsdecode, output[offset:offset + 3])
        result.setdefault(path, {})[attribute] = value
    return result


def evidence_directory(root: Path, value: str, *, environment: bool = False) -> str:
    if (not isinstance(value, str) or not value or any(ord(c) < 32 or ord(c) == 127 for c in value)
            or "\\" in value or re.match(r"^[A-Za-z]:", value)):
        raise ValueError("Invalid cbc-evidence directory.")
    path = Path(value)
    if path.is_absolute() and not environment:
        raise ValueError("cbc-evidence must be repository-relative.")
    target = Path(os.path.abspath(root / path))
    try:
        if environment and path.is_absolute():
            target = target.resolve()
        local = target.relative_to(root)
        resolved = target.resolve()
        actual = resolved.relative_to(root)
    except (ValueError, OSError, RuntimeError) as error:
        raise ValueError("cbc-evidence must remain inside the worktree.") from error
    if any(part.casefold() == ".git" for part in (*local.parts, *actual.parts)):
        raise ValueError("cbc-evidence cannot use Git metadata.")
    if target.exists() and not target.is_dir():
        raise ValueError("cbc-evidence must name a directory.")
    return local.as_posix()


def artifact_relative(root: Path, artifact: str) -> str:
    path = Path(os.path.abspath(artifact))
    try:
        local = path.relative_to(root)
        path.resolve().relative_to(root)
    except (ValueError, OSError, RuntimeError) as error:
        raise ValueError("The artifact must remain inside the worktree.") from error
    if any(part.casefold() == ".git" for part in local.parts) or any(ord(c) < 32 or ord(c) == 127 for c in artifact):
        raise ValueError("Invalid artifact path.")
    return local.as_posix()


def evidence_slug(root: Path, artifact: str) -> str:
    return re.sub(r"[^A-Za-z0-9]+", "-", artifact_relative(root, artifact)).strip("-")


def selected_evidence_directory(root: Path, raw: str) -> str:
    if raw == "unspecified":
        return evidence_directory(root, os.environ.get("CBC_EVIDENCE_DIR") or "docs/cbc-evidence", environment=True)
    if raw in ("set", "unset"):
        raise ValueError("cbc-evidence requires a directory string.")
    return evidence_directory(root, raw)


def evidence_location(root: Path, artifact: str | None, *, ledger: bool = False) -> str:
    local = artifact_relative(root, artifact) if artifact else None
    raw = values(root, [local], attributes=("cbc-evidence",))[local]["cbc-evidence"] if local else "unspecified"
    directory = selected_evidence_directory(root, raw)
    if not ledger:
        return directory
    if not local or not artifact:
        raise ValueError("An artifact is required for an evidence ledger.")
    slug = evidence_slug(root, artifact)
    target = root / directory / (slug + ".md")
    for candidate in (target, target.with_suffix(".acceptance.json"), target.parent / "history",
                      target.with_suffix(".claims.json"), target.with_suffix(".claims"),
                      target.with_suffix(".claims.lock")):
        try:
            actual = candidate.resolve().relative_to(root)
        except (ValueError, OSError, RuntimeError) as error:
            raise ValueError("Evidence files must remain inside the worktree.") from error
        if any(part.casefold() == ".git" for part in actual.parts) or (candidate.is_file() and candidate.stat().st_nlink > 1):
            raise ValueError("Unsafe evidence file destination.")
    return (Path(directory) / target.name).as_posix()


def literal_pattern(path: str) -> str:
    pattern = "/" + re.sub(r"([\\*?\[\]])", r"\\\1", path)
    if re.fullmatch(r"[A-Za-z0-9_./-]+", pattern):
        return pattern
    quoted = []
    for byte in os.fsencode(pattern):
        if byte in (34, 92):
            quoted.append("\\" + chr(byte))
        elif 32 <= byte < 127:
            quoted.append(chr(byte))
        else:
            quoted.append("\\%03o" % byte)
    return '"' + "".join(quoted) + '"'


def rule_for(path: str, weight: str, acceptance: bool) -> str:
    if weight not in ("low", "medium", "high") or not isinstance(acceptance, bool):
        raise ValueError("A valid proposal weight and acceptance flag are required.")
    rule = literal_pattern(path) + " cbc=mandatory cbc-weight=" + weight
    if acceptance:
        rule += " cbc-acceptance=required"
    return rule


def route(root: Path, request: dict[str, Any]) -> list[dict[str, Any]]:
    if not isinstance(request, dict):
        raise ValueError("The routing request must be an object.")
    for flag in ("root_only", "acceptance_required"):
        if not isinstance(request.get(flag, False), bool):
            raise ValueError("Routing options must be Boolean values.")
    owners = request.get("owners", [])
    if not isinstance(owners, list):
        raise ValueError("attribute_owners must be an array of directories.")
    owners = [relative(root, owner) for owner in owners]
    if any(not (root / owner).is_dir() for owner in owners):
        raise ValueError("Every attribute owner must be an existing directory.")
    result = []
    candidates = request.get("candidates", [])
    if not isinstance(candidates, list):
        raise ValueError("Candidates must be an array.")
    paths = [str(relative(root, candidate["path"])) for candidate in candidates]
    available = policy_files(root, paths, False)
    for candidate in candidates:
        artifact = relative(root, candidate["path"])
        if not (root / artifact).is_file():
            raise ValueError("A proposed artifact must exist inside the repository.")
        ancestors = list(artifact.parents)
        declared = [owner for owner in owners if owner in ancestors]
        owner = PurePosixPath(".")
        if not request.get("root_only", False):
            if declared:
                owner = max(declared, key=lambda path: len(path.parts))
            else:
                for parent in ancestors:
                    policy = root / parent / ".gitattributes"
                    if policy.exists() or policy.is_symlink() or str(parent / ".gitattributes") in available:
                        owner = parent
                        break
        policy = owner / ".gitattributes"
        relative(root, str(policy))
        acceptance_value = values(root, [str(artifact)])[str(artifact)]["cbc-acceptance"]
        if acceptance_value not in ("required", "unspecified"):
            raise ValueError("The artifact has an invalid acceptance policy.")
        acceptance = bool(request.get("acceptance_required", False)) or acceptance_value == "required"
        local = artifact.relative_to(owner).as_posix()
        result.append({**candidate, "path": artifact.as_posix(), "attributes_file": policy.as_posix(),
                       "pattern": literal_pattern(local),
                       "rule": rule_for(local, candidate["weight"], acceptance),
                       "acceptance_required": acceptance})
    return result


def apply(root: Path, proposals: list[dict[str, Any]]) -> dict[str, list[str]]:
    if not isinstance(proposals, list):
        raise ValueError("Proposals must be an array.")
    changes = {}
    paths = [str(relative(root, proposal["path"])) for proposal in proposals]
    policies = policy_files(root, paths, False)
    # Validate the entire batch before any file is created or changed.
    for proposal in proposals:
        artifact = relative(root, proposal["path"])
        policy = relative(root, proposal["attributes_file"])
        if policy.name != ".gitattributes" or not (root / artifact).is_file():
            raise ValueError("Invalid attribute proposal destination or artifact.")
        local = artifact.relative_to(policy.parent).as_posix()
        expected = rule_for(local, proposal["weight"], proposal["acceptance_required"])
        if proposal["pattern"] != literal_pattern(local) or proposal["rule"] != expected:
            raise ValueError("The proposal must contain one anchored exact-file rule.")
        target = root / policy
        if not target.parent.is_dir() or (target.exists() and not target.is_file()):
            raise ValueError("The policy destination must be a regular file in an existing directory.")
        original = changes.setdefault(policy, policies.get(str(policy), b""))
        line = os.fsencode(expected)
        if line not in original.splitlines():
            changes[policy] = original + (b"\n" if original and not original.endswith(b"\n") else b"") + line + b"\n"
    planned = {**policies, **{str(path): data for path, data in changes.items()}}
    effective = repository_values(planned, paths)
    for proposal in proposals:
        attrs = effective[str(relative(root, proposal["path"]))]
        if (attrs["cbc"] != "mandatory"
                or attrs["cbc-acceptance"] not in ("required", "unspecified")
                or (proposal["acceptance_required"] and attrs["cbc-acceptance"] != "required")):
            raise ValueError("A deeper policy overrides the proposed mandatory or acceptance rule.")
    for policy, content in changes.items():
        target = root / policy
        if not target.exists() or target.read_bytes() != content:
            target.write_bytes(content)
    return {"attributes_files": sorted(str(path) for path in changes)}


def policy_files(root: Path, paths: list[str], cached: bool) -> dict[str, bytes]:
    candidates = {".gitattributes"}
    for path in paths:
        candidates.update(str(parent / ".gitattributes") for parent in PurePosixPath(path).parents)
    indexed = {}
    for entry in git(root, "--literal-pathspecs", "ls-files", "--stage", "-z", "--", *sorted(candidates)).stdout.split(b"\0"):
        if entry:
            metadata, path = entry.split(b"\t", 1)
            mode, oid, stage = metadata.split()
            if stage == b"0" and mode in (b"100644", b"100755"):
                indexed[os.fsdecode(path)] = oid.decode()
    result = {}
    for name in sorted(candidates):
        if cached:
            if name in indexed:
                result[name] = git(root, "cat-file", "blob", indexed[name]).stdout
            continue
        path = root / name
        if path.is_symlink():
            continue
        try:
            path.resolve().relative_to(root)
        except ValueError:
            continue
        if not cached and path.is_file():
            result[name] = path.read_bytes()
        elif name in indexed:
            result[name] = git(root, "cat-file", "blob", indexed[name]).stdout
    return result


def diagnostics(policies: dict[str, bytes]) -> list[dict[str, Any]]:
    result = []
    layers = {}
    for name, content in policies.items():
        seen = set()
        seen_patterns = {}
        attributes = set()
        for number, raw in enumerate(os.fsdecode(content).splitlines(), 1):
            line = raw.strip()
            if not line or line.startswith("#"):
                continue
            fields = re.match(r'("(?:\\.|[^"\\])*"|\S+)\s+(.*)', line)
            if not fields:
                continue
            pattern, tokens = fields.groups()
            policy = {token.lstrip("-!").split("=", 1)[0] for token in tokens.split()} & set(ATTRIBUTES)
            if not policy:
                continue
            attributes.update(policy)
            location = {"path": name, "line": number}
            if line in seen:
                result.append({"code": "duplicate_rule", **location})
            seen.add(line)
            if pattern in seen_patterns:
                result.append({"code": "duplicate_pattern", **location,
                               "previous_line": seen_patterns[pattern]})
            seen_patterns[pattern] = number
            if pattern.startswith("[attr]") and name != ".gitattributes":
                result.append({"code": "nested_macro", **location})
            elif not pattern.startswith(('"', '/', '[attr]')) and not any(char in pattern for char in "/*?[]\\"):
                result.append({"code": "unanchored_file_rule", **location})
        if attributes:
            layers[name] = attributes
    for child, child_attrs in layers.items():
        directory = PurePosixPath(child).parent
        for parent in directory.parents:
            name = str(parent / ".gitattributes")
            common = child_attrs & layers.get(name, set())
            if common:
                result.append({"code": "potential_override", "path": child,
                               "parent": name, "attributes": sorted(common)})
    return result


def repository_values(policies: dict[str, bytes], paths: list[str]) -> dict[str, dict[str, str]]:
    # An isolated repository delegates precedence to Git, without ambient sources.
    with tempfile.TemporaryDirectory(prefix="cbc-attributes-") as tmp:
        clean = Path(tmp)
        git(clean, "-c", "core.attributesFile=/dev/null", "init", "-q", "--template=", isolated=True)
        for name, content in policies.items():
            destination = clean / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(content)
        controlled = git(clean, "-c", "core.attributesFile=/dev/null", "check-attr", "-z", "--stdin",
                         *ATTRIBUTES, input=b"".join(os.fsencode(p) + b"\0" for p in paths), isolated=True).stdout
        fields = controlled.split(b"\0")
        repository_only = {}
        for offset in range(0, len(fields) - 1, 3):
            path, attr, value = map(os.fsdecode, fields[offset:offset + 3])
            repository_only.setdefault(path, {})[attr] = value

    return repository_only


def snapshot(root: Path, cached: bool = False) -> dict[str, Any]:
    paths = sorted(set(map(os.fsdecode, filter(None, git(root, "ls-files", "-z").stdout.split(b"\0")))))
    effective = values(root, paths, cached)
    all_attributes = values(root, paths, cached, attributes=None)
    policies = policy_files(root, paths, cached)
    warnings = diagnostics(policies)
    repository_only = repository_values(policies, paths)
    for path in paths:
        if effective[path] != repository_only[path]:
            warnings.append({"code": "ambient_difference", "path": path,
                             "effective": effective[path], "repository_only": repository_only[path]})
    info = os.fsdecode(git(root, "rev-parse", "--git-path", "info/attributes").stdout).strip()
    configured = bool(git(root, "config", "--get", "core.attributesFile", optional=True).stdout.strip())
    errors = []
    for path, attrs in effective.items():
        acceptance = attrs["cbc-acceptance"]
        if acceptance not in ("unspecified", "required") or (acceptance == "required" and attrs["cbc"] != "mandatory"):
            errors.append({"code": "policy_error", "path": path})
        if attrs["cbc-evidence"] != "unspecified":
            try:
                selected_evidence_directory(root, attrs["cbc-evidence"])
            except ValueError:
                errors.append({"code": "evidence_policy_error", "path": path})
    return {"schema": SCHEMA, "attributes": list(ATTRIBUTES), "cached": cached,
            "files": effective, "all_attributes": all_attributes,
            "diagnostics": warnings, "errors": errors,
            "ambient_sources": {"info_attributes": (root / info).is_file(),
                                "configured_attributes": configured}}


def compare(before: dict[str, Any], after: dict[str, Any]) -> dict[str, Any]:
    if not isinstance(before, dict) or before.get("schema") != SCHEMA or before.get("attributes") != list(ATTRIBUTES):
        raise ValueError("The baseline must be a cbc-attributes/v1 snapshot.")
    if not isinstance(before.get("files"), dict) or not isinstance(before.get("all_attributes"), dict):
        raise ValueError("The baseline file maps are invalid.")
    if before.get("cached") != after["cached"]:
        raise ValueError("The baseline and current snapshot must use the same index mode.")
    changes = [{"path": path, "before": before["files"].get(path), "after": after["files"].get(path)}
               for path in sorted(set(before["files"]) | set(after["files"]))
               if before["files"].get(path) != after["files"].get(path)]
    all_changes = [{"path": path, "before": before["all_attributes"].get(path),
                    "after": after["all_attributes"].get(path)}
                   for path in sorted(set(before["all_attributes"]) | set(after["all_attributes"]))
                   if before["all_attributes"].get(path) != after["all_attributes"].get(path)]
    return {"schema": SCHEMA, "changes": changes, "all_attribute_changes": all_changes}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("route", "apply", "snapshot", "lint", "compare", "evidence-dir", "evidence-path", "evidence-slug", "evidence-check", "evidence-dirs", "evidence-formal"))
    parser.add_argument("baseline", nargs="?")
    parser.add_argument("--cached", action="store_true")
    args = parser.parse_args()
    try:
        root = Path(os.fsdecode(git(Path.cwd(), "rev-parse", "--show-toplevel").stdout[:-1])).resolve()
        rc = 0
        if args.command == "evidence-slug":
            if not args.baseline:
                raise ValueError("An artifact is required for an evidence slug.")
            print(evidence_slug(root, args.baseline))
            return 0
        if args.command in ("evidence-dir", "evidence-path", "evidence-formal"):
            location = evidence_location(root, args.baseline, ledger=args.command == "evidence-path")
            if args.command == "evidence-formal":
                location = evidence_directory(root, str(Path(location) / "formal"))
            print(location)
            return 0
        if args.command == "evidence-check":
            paths = [os.fsdecode(p) for p in sys.stdin.buffer.read().split(b"\0") if p]
            attrs = values(root, paths, attributes=("cbc-evidence",))
            for path in paths:
                artifact_relative(root, path)
                selected_evidence_directory(root, attrs[path]["cbc-evidence"])
            return 0
        if args.command == "evidence-dirs":
            paths = [os.fsdecode(p) for p in git(root, "ls-files", "--cached", "--others", "--exclude-standard", "-z").stdout.split(b"\0") if p]
            attrs = values(root, paths, attributes=("cbc-evidence",))
            directories = {selected_evidence_directory(root, attrs[p]["cbc-evidence"]) for p in paths}
            if not paths:
                directories.add(selected_evidence_directory(root, "unspecified"))
            print(json.dumps(sorted(directories)))
            return 0
        if args.command == "route":
            output = route(root, json.load(sys.stdin))
        elif args.command == "apply":
            output = apply(root, json.load(sys.stdin))
        else:
            output = snapshot(root, args.cached)
            if args.command == "compare":
                if not args.baseline:
                    raise ValueError("A baseline snapshot file is required.")
                output = compare(json.loads(Path(args.baseline).read_text()), output)
                rc = 4 if output["changes"] or output["all_attribute_changes"] else 0
            elif args.command == "lint":
                rc = 3 if output["errors"] else 0
        print(json.dumps(output, sort_keys=True, indent=2))
        return rc
    except (ValueError, KeyError, TypeError, OSError) as error:
        # OS exceptions can contain local paths. Keep those paths out of diagnostics.
        message = "Attribute file operation failed." if isinstance(error, OSError) else str(error)
        print("cbc attributes: " + message, file=sys.stderr)
        return 3


if __name__ == "__main__":
    raise SystemExit(main())

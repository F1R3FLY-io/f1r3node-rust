import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath
import re
import stat
import sys
import tempfile


MAX_BYTES = 1024 * 1024
REQUIRED_FIELDS = (
    "schema_version", "run_id", "phase", "candidate_id", "node_revision",
    "node_binary_digest", "image_digest", "harness_revision", "external_harness_revision",
    "source_digests", "configuration_digest", "profile_id", "profile_digest",
    "fixture_digest", "expectation_digest", "seed", "provider", "policy_variant",
    "evidence_kind", "capabilities", "tool_versions", "bounds", "assumptions",
    "resource_limits", "required_scenarios", "deadline", "merge_gate",
)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "A JSON object has duplicate fields.")
        result[key] = value
    return result


def invalid_constant(value):
    raise ValueError("Nonfinite JSON numbers are not permitted.")


def finite_number(value):
    number = float(value)
    require(math.isfinite(number), "Nonfinite JSON numbers are not permitted.")
    return number


def read_regular(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        require(stat.S_ISREG(metadata.st_mode), "A manifest record is not a regular file.")
        require(metadata.st_size <= MAX_BYTES, "A manifest record exceeds the size limit.")
        data = stream.read(MAX_BYTES + 1)
    require(len(data) <= MAX_BYTES, "A manifest record exceeds the size limit.")
    return data


def parse(data):
    value = json.loads(data.decode("utf-8"), object_pairs_hook=unique_object, parse_constant=invalid_constant, parse_float=finite_number)
    require(isinstance(value, dict), "A manifest record must be a JSON object.")
    return value


def validate(manifest):
    require(all(key in manifest for key in REQUIRED_FIELDS), "The manifest lacks required fields.")
    require(type(manifest["schema_version"]) is int and manifest["schema_version"] == 1, "The manifest schema version is unsupported.")
    for key in ("run_id", "candidate_id", "profile_id", "policy_variant"):
        require(isinstance(manifest[key], str) and bool(manifest[key].strip()), "A manifest identity is missing.")
    for key in ("node_revision", "harness_revision", "external_harness_revision"):
        require(isinstance(manifest[key], str) and re.fullmatch(r"[0-9a-f]{40}", manifest[key]), "A repository revision is invalid.")
    for key in ("node_binary_digest", "configuration_digest", "profile_digest", "fixture_digest", "expectation_digest"):
        require(isinstance(manifest[key], str) and re.fullmatch(r"[0-9a-f]{64}", manifest[key]), "A manifest digest is invalid.")
    require(manifest["phase"] in ("pre_pr216_merge", "post_pr216_merge"), "The manifest phase is invalid.")
    require(manifest["provider"] in ("docker", "subprocess"), "The manifest provider is invalid.")
    require(manifest["evidence_kind"] in ("synthetic_fixture", "node_observation"), "The evidence kind is invalid.")
    if manifest["provider"] == "docker":
        require(isinstance(manifest["image_digest"], str) and re.fullmatch(r"[0-9a-f]{64}", manifest["image_digest"]), "The Docker image digest is invalid.")
    else:
        require(manifest["image_digest"] is None and isinstance(manifest.get("image_digest_reason"), str) and bool(manifest["image_digest_reason"].strip()), "A subprocess manifest needs an explicit image reason.")
    require(isinstance(manifest["seed"], str) and re.fullmatch(r"0|[1-9][0-9]*", manifest["seed"]), "The manifest seed is invalid.")
    for key in ("source_digests", "capabilities", "tool_versions", "bounds", "resource_limits", "deadline"):
        require(isinstance(manifest[key], dict) and bool(manifest[key]), "A required manifest record is empty or invalid.")
    for path, digest in manifest["source_digests"].items():
        relative = PurePosixPath(path)
        require(not relative.is_absolute() and ".." not in relative.parts and str(relative) == path and path != "." and "\\" not in path, "A source path is invalid.")
        require(isinstance(digest, str) and re.fullmatch(r"[0-9a-f]{64}", digest), "A source digest is invalid.")
    for key in ("assumptions", "required_scenarios"):
        values = manifest[key]
        require(isinstance(values, list) and all(isinstance(value, str) and bool(value.strip()) for value in values), "A required manifest list is invalid.")
    scenarios = manifest["required_scenarios"]
    require(bool(scenarios) and len(set(scenarios)) == len(scenarios), "Required scenarios must be nonempty and unique.")
    if manifest["phase"] == "pre_pr216_merge":
        require(manifest["merge_gate"] is None, "A pre-merge manifest cannot assert a post-merge gate.")
    else:
        require(isinstance(manifest["merge_gate"], dict), "The post-merge gate is missing.")


def bind(manifest_path, output):
    require(bool(manifest_path), "A bound run requires its original manifest.")
    data = read_regular(manifest_path)
    manifest = parse(data)
    validate(manifest)
    output = Path(output)
    require(not output.is_symlink(), "The run directory cannot be a symbolic link.")
    bound = output / ".casper-manifest.json"
    digest = hashlib.sha256(data).hexdigest()
    if bound.exists() or bound.is_symlink():
        require(read_regular(bound) == data, "The immutable manifest changed on resume.")
        checkpoint = output / ".soak-checkpoint-state.json"
        state = output / ".soak-state"
        has_state = state.exists() or state.is_symlink()
        has_checkpoint = checkpoint.exists() or checkpoint.is_symlink()
        require(has_state == has_checkpoint, "The saved state and checkpoint must both exist.")
        if has_checkpoint:
            require(parse(read_regular(checkpoint)).get("manifest_digest") == digest, "The checkpoint manifest identity differs.")
        else:
            require(all(path.name == bound.name for path in output.iterdir()), "An incomplete run cannot discard its existing artifacts.")
        return digest
    require(not output.exists() or not any(output.iterdir()), "An existing run cannot acquire a new manifest identity.")
    output.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(prefix=".casper-manifest-", dir=output)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(name, 0o444)
        try:
            os.link(name, bound)
        except FileExistsError:
            require(read_regular(bound) == data, "Another manifest already owns the run directory.")
        directory = os.open(output, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        os.unlink(name)
    return digest


def main():
    try:
        require(len(sys.argv) == 3, "Usage: casper_soak_manifest.py MANIFEST OUTPUT_DIRECTORY")
        print(bind(sys.argv[1], sys.argv[2]))
        return 0
    except (OSError, ValueError, RecursionError):
        print("The manifest is invalid or differs from the retained run identity.", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())

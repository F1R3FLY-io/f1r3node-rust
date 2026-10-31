import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = Path("formal/pathmap_obligations.generated.json")
RUNNER = Path("scripts/check-stack-safe-pda-proofs.sh")
FAMILIES = (
    "accepted_match",
    "affine_import",
    "atomic_checkpoint",
    "bag",
    "epathmap_core",
    "epm1_positional",
    "free_map_journal",
    "ordered_receive_view",
    "parbag_egress",
    "rci2_channel_image",
    "retained_set_automaton",
    "rho_path_v2",
    "rmp2",
    "spatial_matcher",
    "split_index_authority",
    "workspace_regression",
)
MODES = {"proof_only", "concrete_witness", "executable_refinement", "executable_only"}
EXECUTABLE_MODES = MODES - {"proof_only"}
WITNESS_FIELDS = (
    "target",
    "target_name",
    "test_target",
    "test_name",
    "test",
    "normal_suite_witnesses",
    "assertion",
)


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def checker(family):
    return Path(f"scripts/check-{family.replace('_', '-')}-obligations.py")


def manifest(family):
    return Path(f"formal/{family}_obligations.generated.json")


def validate_sources(checkers, manifests, runner):
    expected_checkers = {checker(family) for family in FAMILIES}
    expected_manifests = {manifest(family) for family in FAMILIES}
    require(checkers == expected_checkers, "missing or extra family checker")
    require(manifests == expected_manifests, "missing or extra family manifest")
    for path in sorted(expected_checkers):
        invocation = f'python3 "$repo_root/{path}" --check'
        require(
            runner.count(invocation) == 1,
            f"family checker is absent or duplicated in proof runner: {path}",
        )


def inspect_obligations(value, location, seen_ids, counts):
    if isinstance(value, list):
        for index, child in enumerate(value):
            inspect_obligations(child, f"{location}/{index}", seen_ids, counts)
        return
    if not isinstance(value, dict):
        return
    identifier = value.get("id")
    if identifier is not None:
        require(
            isinstance(identifier, str) and bool(identifier),
            f"invalid obligation id: {location}",
        )
        require(identifier not in seen_ids, f"duplicate obligation id: {identifier}")
        seen_ids[identifier] = location
    mode = value.get("mode", value.get("classification"))
    if mode is not None:
        require(mode in MODES, f"unknown obligation mode: {location}")
        counts[mode] += 1
        if mode in EXECUTABLE_MODES:
            require(
                any(value.get(field) for field in WITNESS_FIELDS),
                f"executable obligation without witness: {location}",
            )
    for key, child in value.items():
        inspect_obligations(child, f"{location}/{key}", seen_ids, counts)


def aggregate(payloads):
    require(set(payloads) == set(FAMILIES), "missing or extra registry family")
    seen_ids = {}
    counts = {mode: 0 for mode in sorted(MODES)}
    families = []
    for family in FAMILIES:
        path = manifest(family)
        payload = payloads[family]
        inspect_obligations(payload, family, seen_ids, counts)
        encoded = json.dumps(payload, indent=2, sort_keys=True) + "\n"
        families.append(
            {
                "family": family,
                "checker": str(checker(family)),
                "manifest": str(path),
                "manifest_sha256": hashlib.sha256(encoded.encode()).hexdigest(),
                "obligations": payload,
            }
        )
    return {
        "families": families,
        "global_obligation_ids": len(seen_ids),
        "classified_obligations": counts,
        "scope": "PathMap integration formal obligations and normal-suite refinement bindings",
    }


def self_test(payloads, checkers, manifests, runner):
    def rejects(call):
        try:
            call()
        except RegistryError:
            return
        raise RegistryError("mutated aggregate registry input was accepted")

    first = json.loads(json.dumps(payloads))
    first["bag"]["rocq_obligations"][0]["id"] = first["accepted_match"][
        "rocq_theorems"
    ][0]["id"]
    rejects(lambda: aggregate(first))
    second = json.loads(json.dumps(payloads))
    second["bag"]["rocq_obligations"][0]["mode"] = "executable_refinement"
    second["bag"]["rocq_obligations"][0]["target_name"] = None
    rejects(lambda: aggregate(second))
    rejects(lambda: validate_sources(checkers - {checker("bag")}, manifests, runner))
    rejects(
        lambda: validate_sources(
            checkers,
            manifests | {Path("formal/extra_obligations.generated.json")},
            runner,
        )
    )
    rejects(
        lambda: validate_sources(
            checkers,
            manifests,
            runner.replace(
                'python3 "$repo_root/scripts/check-bag-obligations.py" --check',
                "",
                1,
            ),
        )
    )


def main():
    parser = argparse.ArgumentParser()
    choice = parser.add_mutually_exclusive_group(required=True)
    choice.add_argument("--write", action="store_true")
    choice.add_argument("--check", action="store_true")
    choice.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    checkers = {
        path.relative_to(ROOT)
        for path in (ROOT / "scripts").glob("check-*-obligations.py")
    }
    manifests = {
        path.relative_to(ROOT)
        for path in (ROOT / "formal").glob("*_obligations.generated.json")
        if path.relative_to(ROOT) != OUTPUT
    }
    runner = (ROOT / RUNNER).read_text(encoding="utf-8")
    validate_sources(checkers, manifests, runner)
    payloads = {}
    for family in FAMILIES:
        result = subprocess.run(
            [sys.executable, str(ROOT / checker(family)), "--check"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        require(
            result.returncode == 0,
            f"stale or invalid {family} registry: {result.stdout}{result.stderr}",
        )
        payloads[family] = json.loads(
            (ROOT / manifest(family)).read_text(encoding="utf-8")
        )
    if args.self_test:
        self_test(payloads, checkers, manifests, runner)
        print("PathMap aggregate registry mutation controls passed")
        return
    content = json.dumps(aggregate(payloads), indent=2, sort_keys=True) + "\n"
    if args.write:
        (ROOT / OUTPUT).write_text(content, encoding="utf-8")
        print("PathMap aggregate registry written")
    else:
        require(
            (ROOT / OUTPUT).read_text(encoding="utf-8") == content,
            "stale aggregate registry",
        )
        print("PathMap aggregate registry current")


if __name__ == "__main__":
    main()

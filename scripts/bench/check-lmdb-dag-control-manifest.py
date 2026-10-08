"""Check the frozen LMDB/DAG comparison plan and refuse stale run instances."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "docs/design/pathmap/lmdb-dag-equal-contract-control-manifest.json"
FIXTURES = ROOT / "docs/design/pathmap/lmdb-dag-equal-contract-negative-fixtures.json"
CONTROL_PATHS = [
    "rspace++/libs/rspace_rhotypes/src/lib.rs",
    "rspace++/src/rspace/state/rspace_state_authority.rs",
    "rspace++/src/rspace/shared/env_cache.rs",
    "rspace++/src/rspace/shared/rspace_store_manager.rs",
    "shared/src/rust/store/lmdb_key_value_store.rs",
]
LANES = {
    "component_admission_persistence": "component_only",
    "owner_primary_unchanged_state": "real_owner",
    "delta_sharing_retention": "real_owner",
    "concurrency_pinned_readers": "real_owner",
    "publication_lifecycle": "real_owner",
    "transfer_comm": "real_owner",
}
MATRIX_LEVELS = {
    "component_admission_persistence": {
        "objects": [4096, 65536],
        "fresh_or_reused_digest": ["fresh", "reused"],
    },
    "owner_primary_unchanged_state": {
        "objects": [4096, 65536, 1048576],
        "changed_paths_per_checkpoint": [64],
    },
    "delta_sharing_retention": {
        "delta": [1, 64, 4096],
        "retained_roots": [1, 16, 128],
        "sharing_percent": [0, 50, 95],
        "reference_case": [
            "repeated_update",
            "full_removal",
            "same_digest_reuse",
            "retention_eviction",
        ],
    },
    "concurrency_pinned_readers": {
        "workers": [1, 8],
        "long_lived_old_root_pin": [False, True],
    },
    "publication_lifecycle": {
        "checkpoint_every_mutations": [1, 64, 1024],
        "pack_rotation": [False, True],
        "wal_rotation": [False, True],
        "restart": ["clean", "torn"],
        "index_repair": [False, True],
        "gc_pin": [False, True],
    },
    "transfer_comm": {
        "channel_distribution": ["shared", "random", "adversarial"],
        "comm_order": ["datum_first", "receive_first"],
        "transfer": ["cold_export_import", "state_sync"],
    },
}
MATRIX_OUTPUTS = {
    "component_admission_persistence": [
        "admission_copy_commit_latency",
        "allocated_bytes",
        "peak_rss",
        "bytes_read_written",
        "sync_count",
        "installed_physical_bytes",
    ],
    "owner_primary_unchanged_state": [
        "durable_batches_per_second",
        "operation_p50_p95_p99",
        "checkpoint_p50_p95_p99",
        "semantic_witness",
        "work_per_changed_path",
    ],
    "delta_sharing_retention": [
        "reference_activations_deactivations",
        "new_bytes",
        "old_closure_work",
        "metadata_work",
        "dedup",
        "latency",
    ],
    "concurrency_pinned_readers": [
        "durable_batches_per_second",
        "p99_latency",
        "lock_contention",
        "gc_deferral",
        "wal_replay_tail",
        "recovery_point",
    ],
    "publication_lifecycle": [
        "checkpoint_reopen_recovery_latency",
        "gc_work",
        "write_amplification",
        "retained_and_garbage_bytes",
        "whole_tree_allocated_bytes",
        "inode_count",
    ],
    "transfer_comm": [
        "transfer_bytes_latency",
        "imported_root_witness",
        "produce_consume_comm_throughput",
        "p99_latency",
        "candidate_and_node_visits",
    ],
}
HEX40 = re.compile(r"[0-9a-f]{40}\Z")
HEX64 = re.compile(r"[0-9a-f]{64}\Z")


class CheckFailure(Exception):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code


def require(condition: bool, code: str, message: str) -> None:
    if not condition:
        raise CheckFailure(code, message)


def no_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate_json_key", f"duplicate JSON key: {key}")
        result[key] = value
    return result


def read_json(path: Path) -> dict:
    try:
        value = json.loads(path.read_text(), object_pairs_hook=no_duplicate_keys)
    except (OSError, ValueError) as error:
        raise CheckFailure("invalid_json", f"{path}: {error}") from error
    require(isinstance(value, dict), "invalid_json", f"{path}: root must be an object")
    return value


def relative_file(name: str) -> Path:
    require(isinstance(name, str), "invalid_path", "artifact path must be text")
    path = Path(name)
    require(not path.is_absolute() and ".." not in path.parts, "invalid_path", name)
    resolved = ROOT / path
    require(resolved.is_file(), "missing_artifact", name)
    return resolved


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git(*arguments: str) -> str:
    result = subprocess.run(
        ["git", *arguments], cwd=ROOT, text=True, capture_output=True, check=False
    )
    require(result.returncode == 0, "git_preflight_failed", result.stderr.strip())
    return result.stdout.strip()


def exact(actual: object, expected: object, code: str) -> None:
    require(
        type(actual) is type(expected) and actual == expected,
        code,
        f"expected {expected!r}, got {actual!r}",
    )


def validate_static(manifest: dict) -> None:
    exact(manifest.get("schema_version"), 1, "schema_version_drift")
    exact(
        manifest.get("task_public_id"),
        "rspace-dag-required-performance-control",
        "task_identity_drift",
    )
    exact(manifest.get("experiment_id"), 377, "experiment_identity_drift")
    exact(manifest.get("state"), "preregistered_without_samples", "premature_result")

    protocol = manifest.get("protocol", {})
    exact(
        protocol.get("path"),
        "docs/design/pathmap/lmdb-dag-equal-contract-comparison-protocol.md",
        "protocol_path_drift",
    )
    require(
        digest(relative_file(protocol["path"])) == protocol.get("sha256"),
        "protocol_stale",
        "protocol bytes differ from the frozen manifest digest",
    )

    historical = manifest.get("historical_evidence", {})
    exact(
        historical.get("role"),
        "historical_component_context_only",
        "historical_promoted_to_control",
    )
    exact(
        historical.get("path"),
        "docs/design/stack-safety/measurements/rspace-act-lmdb-experiment-327-transcript-2026-09-29.json",
        "historical_path_drift",
    )
    require(
        digest(relative_file(historical["path"])) == historical.get("sha256"),
        "historical_artifact_stale",
        "retained experiment 327 transcript hash changed",
    )
    exact(
        historical.get("component_harness"),
        "rspace++/benches/rspace_act_lmdb.rs",
        "historical_harness_drift",
    )
    exact(
        git("log", "-1", "--format=%H", "--", historical["component_harness"]),
        historical.get("harness_last_edit"),
        "historical_harness_stale",
    )

    control = manifest.get("control", {})
    pin = control.get("source_pin")
    require(
        isinstance(pin, str) and HEX40.fullmatch(pin), "invalid_source_pin", str(pin)
    )
    exact(git("rev-parse", "--verify", f"{pin}^{{commit}}"), pin, "missing_source_pin")
    exact(control.get("source_paths"), CONTROL_PATHS, "control_source_scope_drift")
    exact(control.get("owner_entrypoint"), "space_new", "control_owner_drift")
    for field, expected in {
        "map_size_bytes": 1073741824,
        "max_databases": 20,
        "max_readers": 2048,
        "relaxed_sync": False,
    }.items():
        exact(control.get(field), expected, "control_setting_drift")

    treatment = manifest.get("treatment", {})
    exact(
        treatment.get("owner_entrypoint"),
        "DurableAuthorityViewV1::into_rspace_authority",
        "treatment_owner_drift",
    )
    exact(
        treatment.get("ffi_selected_at_preregistration"), False, "premature_owner_claim"
    )
    exact(
        treatment.get("readiness"),
        "requires_non_default_real_owner_adapter",
        "premature_owner_claim",
    )

    contract = manifest.get("paired_contract", {})
    exact(
        contract.get("acknowledgement"),
        "successful_durable_checkpoint_after_required_sync",
        "durability_contract_drift",
    )
    for field in (
        "identical_command_trace",
        "identical_canonical_object_stream_and_batch_size",
        "identical_sync_and_barrier_sequence",
        "identical_retention_and_cache_policy",
        "identical_worker_and_reader_pin_policy",
        "durability_kill_reopen_witness_required",
    ):
        exact(contract.get(field), True, "paired_contract_drift")
    exact(
        contract.get("semantic_witnesses"),
        [
            "current_and_known_roots",
            "data_bag_contents_and_order",
            "continuation_map_contents_and_order",
            "join_bag_contents_and_order",
            "produce_consume_comm_diagnostics",
            "reset_replay_and_restart_selection",
            "cold_export_import_and_state_sync_bytes",
        ],
        "semantic_witness_gap",
    )

    sampling = manifest.get("sampling", {})
    frozen_sampling = {
        "primary_metric": "median_within_pair_treatment_control_durable_batch_throughput_ratio",
        "primary_pairs": 51,
        "secondary_pairs": 30,
        "bootstrap_replicates": 100000,
        "bootstrap_seed": 20261001,
        "confidence_percent": 99,
        "primary_improvement_lower_bound_exclusive": 1.05,
        "critical_throughput_lower_bound_minimum": 0.95,
        "critical_p99_latency_upper_bound_maximum": 1.05,
        "critical_physical_footprint_ratio_maximum": 1.05,
        "critical_peak_rss_ratio_maximum": 1.05,
        "randomized_balanced_pair_order": True,
        "one_pair_one_retained_run": True,
        "timed_runs_unprofiled": True,
    }
    for field, expected in frozen_sampling.items():
        exact(sampling.get(field), expected, "sampling_contract_drift")

    primary = manifest.get("primary_cell", {})
    for field, expected in {
        "cell_id": "owner-primary-65536",
        "lane": "owner_primary_unchanged_state",
        "scope": "real_owner",
        "mutations": 4096,
        "checkpoints": 64,
        "objects": 65536,
        "changed_paths_per_checkpoint": 64,
        "retained_roots": 16,
        "sharing_percent": 95,
        "payload_bytes": 128,
        "workers": 1,
        "pairs": 51,
    }.items():
        exact(primary.get(field), expected, "primary_cell_drift")

    matrix = manifest.get("matrix", [])
    require(isinstance(matrix, list), "matrix_gap", "matrix must be a list")
    exact(len(matrix), len(LANES), "matrix_gap")
    lanes = [row.get("lane") for row in matrix if isinstance(row, dict)]
    exact(set(lanes), set(LANES), "matrix_gap")
    exact(len(lanes), len(LANES), "matrix_duplicate_lane")
    for row in matrix:
        exact(row.get("scope"), LANES[row["lane"]], "matrix_scope_drift")
        exact(row.get("levels"), MATRIX_LEVELS[row["lane"]], "matrix_levels_drift")
        exact(row.get("outputs"), MATRIX_OUTPUTS[row["lane"]], "matrix_outputs_drift")

    template = manifest.get("run_instance_template", {})
    exact(template.get("status"), "unfilled", "template_prematurely_ready")
    for field in (
        "source_revision",
        "control_binary_sha256",
        "treatment_binary_sha256",
        "host_snapshot_sha256",
        "real_owner_adapter_commit",
        "semantic_witness_sha256",
        "kill_reopen_witness_sha256",
        "source_tree_clean",
        "same_durability_contract",
        "pair_plan_sha256",
    ):
        exact(template.get(field), None, "template_prematurely_ready")
    exact(
        template.get("historical_control_samples"),
        False,
        "historical_promoted_to_control",
    )
    exact(template.get("cell_ids"), [], "template_prematurely_ready")


def check_staleness(
    revision: str, head: str, changed_control: list[str], dirty_paths: list[str]
) -> None:
    exact(revision, head, "revision_mismatch")
    require(
        not changed_control,
        "control_source_stale",
        f"{len(changed_control)} pinned control paths changed: {changed_control[:3]!r}",
    )
    require(
        not dirty_paths,
        "dirty_source_tree",
        f"{len(dirty_paths)} dirty paths: {dirty_paths[:3]!r}",
    )


def validate_preflight(manifest: dict, instance: dict) -> None:
    exact(instance.get("status"), "ready", "run_instance_incomplete")
    revision = instance.get("source_revision")
    require(
        isinstance(revision, str) and HEX40.fullmatch(revision),
        "run_instance_incomplete",
        "source_revision must be a full commit SHA",
    )
    for field in (
        "control_binary_sha256",
        "treatment_binary_sha256",
        "host_snapshot_sha256",
        "semantic_witness_sha256",
        "kill_reopen_witness_sha256",
        "pair_plan_sha256",
    ):
        value = instance.get(field)
        require(
            isinstance(value, str) and HEX64.fullmatch(value),
            "run_instance_incomplete",
            f"{field} must be a SHA-256 digest",
        )
    adapter = instance.get("real_owner_adapter_commit")
    require(
        isinstance(adapter, str) and HEX40.fullmatch(adapter),
        "owner_adapter_missing",
        "real-owner adapter commit is required",
    )
    exact(instance.get("source_tree_clean"), True, "dirty_source_tree")
    exact(
        instance.get("same_durability_contract"), True, "durability_contract_mismatch"
    )
    exact(
        instance.get("historical_control_samples"),
        False,
        "historical_promoted_to_control",
    )
    cell_ids = instance.get("cell_ids", [])
    require(
        isinstance(cell_ids, list)
        and "owner-primary-65536" in cell_ids
        and len(cell_ids) == len(set(cell_ids)),
        "pair_plan_incomplete",
        "pair plan must include the unique primary cell",
    )
    pin = manifest["control"]["source_pin"]
    git("rev-parse", "--verify", f"{revision}^{{commit}}")
    git("rev-parse", "--verify", f"{adapter}^{{commit}}")
    git("merge-base", "--is-ancestor", pin, revision)
    git("merge-base", "--is-ancestor", adapter, revision)
    changed = git("diff", "--name-only", pin, revision, "--", *CONTROL_PATHS)
    dirty = git("status", "--porcelain=v1")
    check_staleness(
        revision, git("rev-parse", "HEAD"), changed.splitlines(), dirty.splitlines()
    )


def set_pointer(value: dict, pointer: str, replacement: object) -> None:
    parts = pointer.strip("/").split("/")
    current = value
    for part in parts[:-1]:
        current = current[int(part)] if isinstance(current, list) else current[part]
    last = parts[-1]
    if isinstance(current, list):
        current[int(last)] = replacement
    else:
        current[last] = replacement


def self_test(manifest: dict) -> None:
    fixtures = read_json(FIXTURES)
    exact(fixtures.get("schema_version"), 1, "negative_fixture_schema_drift")
    cases = fixtures.get("manifest_mutations", [])
    require(
        isinstance(cases, list), "negative_fixture_gap", "manifest mutations missing"
    )
    exact(len(cases), 12, "negative_fixture_gap")
    probes = fixtures.get("staleness_probes", [])
    require(
        isinstance(probes, list), "negative_fixture_gap", "staleness probes missing"
    )
    exact(len(probes), 3, "negative_fixture_gap")
    for case in cases:
        mutated = copy.deepcopy(manifest)
        set_pointer(mutated, case["pointer"], case["value"])
        try:
            validate_static(mutated)
        except CheckFailure as error:
            exact(error.code, case["expected_code"], "negative_fixture_mismatch")
        else:
            raise CheckFailure("negative_fixture_survived", case["name"])
    for case in probes:
        try:
            check_staleness(**case["inputs"])
        except CheckFailure as error:
            exact(error.code, case["expected_code"], "negative_fixture_mismatch")
        else:
            raise CheckFailure("negative_fixture_survived", case["name"])
    print(
        f"lmdb-dag-control: {len(cases)} manifest mutants and "
        f"{len(probes)} staleness probes rejected"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("preregistration", "preflight", "self-test"))
    parser.add_argument("--manifest", type=Path, default=MANIFEST)
    parser.add_argument("--instance", type=Path)
    arguments = parser.parse_args()
    try:
        manifest = read_json(arguments.manifest)
        validate_static(manifest)
        if arguments.mode == "self-test":
            self_test(manifest)
        elif arguments.mode == "preflight":
            require(
                arguments.instance is not None,
                "run_instance_missing",
                "--instance is required",
            )
            validate_preflight(manifest, read_json(arguments.instance))
            print("lmdb-dag-control: paired run instance eligible for witness checks")
        else:
            print(
                "lmdb-dag-control: preregistration and frozen evidence valid; no samples claimed"
            )
    except CheckFailure as error:
        print(f"lmdb-dag-control: {error.code}: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

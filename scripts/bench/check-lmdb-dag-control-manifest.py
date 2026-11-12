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
    "history_scaling": "real_owner",
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
    "history_scaling": {"retained_states": [16, 256, 4096]},
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
    "history_scaling": ["reopen_latency", "peak_rss", "physical_bytes"],
}
FORBIDDEN_CROSS_ARM_SUBJECTS = ("roots", "orders", "chosen_comms", "state_sync_bytes")
COMPARISON_SUBJECTS = {
    "operations": "operation_list",
    "payloads": "operation_list",
    "checkpoint_positions": "operation_list",
    "replay_results": "results",
    "replay_contents": "contents",
    "replay_root": "roots",
    "export_import_root": "roots",
    "contents_digest_at_every_checkpoint": "contents_digest",
    "roots": "roots",
    "orders": "orders",
    "chosen_comms": "chosen_comms",
    "state_sync_bytes": "state_sync_bytes",
    "current_and_known_roots": "roots",
    "data_bag_contents_and_order": "orders",
    "continuation_map_contents_and_order": "orders",
    "join_bag_contents_and_order": "orders",
    "produce_consume_comm_diagnostics": "chosen_comms",
    "reset_replay_and_restart_selection": "roots",
    "cold_export_import_and_state_sync_bytes": "state_sync_bytes",
}
WORK_EQUIVALENCE_WITNESSES = {
    "W1": {
        "name": "same_operation_list",
        "scope": "across_arms",
        "compares": ["operations", "payloads", "checkpoint_positions"],
        "reset_names": "checkpoint_position",
    },
    "W2": {
        "name": "own_validity",
        "scope": "within_each_arm",
        "compares": [
            "replay_results",
            "replay_contents",
            "replay_root",
            "export_import_root",
        ],
    },
    "W3": {
        "name": "choice_free_contents_digest",
        "scope": "across_arms",
        "applies_to": "timed_cells",
        "compares": ["contents_digest_at_every_checkpoint"],
        "contents_digest": "canonical_multiset_digest_per_channel_and_ordered_channel_list",
        "computed_outside_both_backends": True,
    },
}
CHOICE_BEARING_CHECKS = ["W1", "W2", "comm_membership_in_reference_enabled_set"]
EXCLUDED_TRACES = ["two_orders_of_one_channel_set"]
MULTI_WORKER_TIMED_CELLS = {
    "concurrency_pinned_readers": {
        "workers": 8,
        "choice_freedom_method": "channel_disjoint_parts",
        "parts_checked_on": "generated_operation_lists_outside_both_backends",
        "per_worker_verification": "one_reference_run_per_worker",
        "barriers": "all_workers_joined_after_prescribed_prefixes",
    },
}
ADDED_CELLS = {
    "publication_lifecycle": [
        {
            "cell_id": "publication-lifecycle-reset-then-checkpoint",
            "timed": True,
            "choice_free": True,
            "operation_pattern": "reset_to_an_earlier_checkpoint_position_then_checkpoint",
            "outputs": ["reset_latency", "following_checkpoint_latency"],
        }
    ],
}
NOT_APPLICABLE = [
    {
        "item": "component_lane",
        "targets": [{"lane": "component_admission_persistence"}],
    },
    {
        "item": "paired_retained_root_gc",
        "targets": [
            {
                "lane": "publication_lifecycle",
                "axis": "gc_pin",
                "levels": [False, True],
            },
            {"lane": "publication_lifecycle", "output": "gc_work"},
            {"lane": "concurrency_pinned_readers", "output": "gc_deferral"},
        ],
    },
    {
        "item": "retention_eviction",
        "targets": [
            {
                "lane": "delta_sharing_retention",
                "axis": "reference_case",
                "levels": ["retention_eviction"],
            }
        ],
    },
]
WHOLLY_NOT_APPLICABLE_LANES = frozenset(
    target["lane"]
    for entry in NOT_APPLICABLE
    for target in entry["targets"]
    if set(target) == {"lane"}
)
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


def git_is_ancestor(ancestor: str, descendant: str) -> bool:
    result = subprocess.run(
        ["git", "merge-base", "--is-ancestor", ancestor, descendant],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    require(result.returncode in (0, 1), "git_preflight_failed", result.stderr.strip())
    return result.returncode == 0


def check_harness_pin(
    pin: str, pin_is_ancestor_of_head: bool, last_touch_as_of_pin: str
) -> None:
    require(
        pin_is_ancestor_of_head,
        "historical_harness_not_ancestor",
        f"harness pin {pin} is not an ancestor of HEAD",
    )
    require(
        last_touch_as_of_pin == pin,
        "historical_harness_stale",
        f"harness pin {pin} is not the last commit that touched the harness as of "
        f"itself ({last_touch_as_of_pin or 'none'} is)",
    )


def exact(actual: object, expected: object, code: str) -> None:
    require(
        type(actual) is type(expected) and actual == expected,
        code,
        f"expected {expected!r}, got {actual!r}",
    )


def validate_work_equivalence(contract: dict) -> None:
    require(
        "semantic_witnesses" not in contract,
        "cross_arm_comparison_forbidden",
        "the superseded witness list compares roots, orders, chosen COMMs and "
        "state-sync bytes across arms",
    )
    witnesses = contract.get("work_equivalence_witnesses")
    if not (
        isinstance(witnesses, dict)
        and set(witnesses) == set(WORK_EQUIVALENCE_WITNESSES)
    ):
        raise CheckFailure(
            "work_witness_gap", "work-equivalence witnesses W1, W2 and W3 are required"
        )
    for label, witness in witnesses.items():
        require(
            isinstance(witness, dict) and isinstance(witness.get("compares"), list),
            "work_witness_drift",
            f"{label} must be an object with a list of compared items",
        )
        if witness.get("scope") == "across_arms":
            for item in witness["compares"]:
                subject = (
                    COMPARISON_SUBJECTS.get(item) if isinstance(item, str) else None
                )
                require(
                    subject not in FORBIDDEN_CROSS_ARM_SUBJECTS,
                    "cross_arm_comparison_forbidden",
                    f"{label} compares {item!r} ({subject}) across arms",
                )
    require(
        witnesses["W1"].get("reset_names") != "root_value",
        "cross_arm_comparison_forbidden",
        "a reset in the operation list names a root value, which differs across arms",
    )
    exact(witnesses, WORK_EQUIVALENCE_WITNESSES, "work_witness_drift")
    exact(
        contract.get("never_compared_across_arms"),
        list(FORBIDDEN_CROSS_ARM_SUBJECTS),
        "work_witness_drift",
    )


def validate_timed_cell_policy(policy: object) -> None:
    if not isinstance(policy, dict):
        raise CheckFailure(
            "timed_cell_not_choice_free", "the timed-cell policy is missing"
        )
    for field, expected in {
        "choice_free_required": True,
        "required_outcome_set": "singleton",
    }.items():
        exact(policy.get(field), expected, "timed_cell_not_choice_free")
    for field, expected in {
        "choice_freedom_verifier": "executable_reference_tuple_space",
        "verifier_delivered_by_leaf": "rspace-executable-reference-tuplespace",
        "verified_before_sampling": True,
    }.items():
        exact(policy.get(field), expected, "choice_freedom_verifier_drift")
    exact(
        policy.get("verifier_available_at_preregistration"),
        False,
        "premature_verifier_claim",
    )
    exact(policy.get("excluded_traces"), EXCLUDED_TRACES, "trace_exclusion_gap")


def validate_lane_cells(row: dict) -> None:
    lane = row["lane"]
    if lane in WHOLLY_NOT_APPLICABLE_LANES:
        for field in (
            "timed_cells_choice_free",
            "added_cells",
            "multi_worker_timed_cells",
            "untimed_choice_bearing_cell",
        ):
            require(
                field not in row,
                "not_applicable_drift",
                f"{lane} is not applicable before sampling but declares {field}",
            )
        return
    require(
        row.get("timed_cells_choice_free") is True,
        "timed_cell_not_choice_free",
        f"the timed cells of {lane} are not marked choice-free",
    )
    added = row.get("added_cells", [])
    require(
        isinstance(added, list) and all(isinstance(cell, dict) for cell in added),
        "matrix_cells_drift",
        f"the added cells of {lane} must be a list of objects",
    )
    for cell in added:
        require(
            cell.get("timed") is False or cell.get("choice_free") is True,
            "timed_cell_not_choice_free",
            f"timed cell {cell.get('cell_id')!r} is not marked choice-free",
        )
    exact(added, ADDED_CELLS.get(lane, []), "matrix_cells_drift")
    validate_multi_worker_cells(row)
    choice_bearing = row.get("untimed_choice_bearing_cell")
    if not isinstance(choice_bearing, dict):
        raise CheckFailure(
            "choice_bearing_cell_gap", f"{lane} has no untimed choice-bearing cell"
        )
    require(
        choice_bearing.get("timed") is False,
        "timed_cell_not_choice_free",
        f"the choice-bearing cell of {lane} is timed but not choice-free",
    )
    require(
        choice_bearing.get("excluded_traces") == EXCLUDED_TRACES,
        "trace_exclusion_gap",
        f"the choice-bearing cell of {lane} admits two orders of one channel set",
    )
    exact(
        choice_bearing,
        {
            "cell_id": lane.replace("_", "-") + "-choice-bearing",
            "timed": False,
            "choice_free": False,
            "checks": CHOICE_BEARING_CHECKS,
            "arms_may_end_in_different_outcome_members": True,
            "excluded_traces": EXCLUDED_TRACES,
        },
        "choice_bearing_cell_gap",
    )


def validate_multi_worker_cells(row: dict) -> None:
    lane = row["lane"]
    expected = MULTI_WORKER_TIMED_CELLS.get(lane)
    marker = row.get("multi_worker_timed_cells")
    if expected is None:
        require(
            marker is None,
            "multi_worker_method_drift",
            f"{lane} has no multi-worker level but declares multi-worker timed cells",
        )
        return
    fields = marker if isinstance(marker, dict) else {}
    require(
        fields.get("choice_freedom_method") == expected["choice_freedom_method"],
        "multi_worker_disjointness_gap",
        f"the {expected['workers']}-worker timed cells of {lane} lack the "
        "channel-disjoint parts marker",
    )
    require(
        fields.get("barriers") == expected["barriers"],
        "multi_worker_barrier_not_joined",
        f"the {expected['workers']}-worker timed cells of {lane} do not take every "
        "checkpoint and reset with all workers joined",
    )
    exact(marker, expected, "multi_worker_method_drift")


def validate_not_applicable(entries: object) -> None:
    if not (
        isinstance(entries, list) and all(isinstance(entry, dict) for entry in entries)
    ):
        raise CheckFailure(
            "not_applicable_drift", "not-applicable markings must be a list of objects"
        )
    for entry in entries:
        reason = entry.get("reason")
        require(
            isinstance(reason, str) and reason.strip() != "",
            "not_applicable_reason_missing",
            f"{entry.get('item')!r} is marked not applicable without a reason",
        )
    exact(
        [
            {key: value for key, value in entry.items() if key != "reason"}
            for entry in entries
        ],
        NOT_APPLICABLE,
        "not_applicable_drift",
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
    harness_pin = historical.get("harness_last_edit")
    if not (isinstance(harness_pin, str) and HEX40.fullmatch(harness_pin) is not None):
        raise CheckFailure(
            "historical_harness_stale",
            f"harness pin {harness_pin!r} is not a full commit SHA",
        )
    check_harness_pin(
        harness_pin,
        git_is_ancestor(harness_pin, "HEAD"),
        git(
            "log",
            "-1",
            "--format=%H",
            harness_pin,
            "--",
            historical["component_harness"],
        ),
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
    for field, expected in {
        "owner_entrypoint": "RSpace with the durable backend selected",
        "selection_delivered_by_leaf": "rspace-select-durable-backend",
    }.items():
        exact(treatment.get(field), expected, "treatment_owner_drift")
    exact(
        treatment.get("ffi_selected_at_preregistration"), False, "premature_owner_claim"
    )
    exact(
        treatment.get("readiness"),
        "requires_durable_backend_selection",
        "premature_owner_claim",
    )

    contract = manifest.get("paired_contract", {})
    exact(
        contract.get("acknowledgement"),
        "successful_durable_checkpoint_after_required_sync",
        "durability_contract_drift",
    )
    for field in (
        "identical_sync_and_barrier_sequence",
        "identical_retention_and_cache_policy",
        "identical_worker_and_reader_pin_policy",
        "durability_kill_reopen_witness_required",
    ):
        exact(contract.get(field), True, "paired_contract_drift")
    for field, expected in {
        "retention_policy": "keep_all_in_both_arms",
        "pair_validity": "same_work_modulo_nondeterminism",
    }.items():
        exact(contract.get(field), expected, "paired_contract_drift")
    validate_work_equivalence(contract)
    validate_timed_cell_policy(manifest.get("timed_cell_policy"))

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
        validate_lane_cells(row)
    validate_not_applicable(manifest.get("not_applicable_before_sampling"))

    template = manifest.get("run_instance_template", {})
    exact(template.get("status"), "unfilled", "template_prematurely_ready")
    for field in (
        "source_revision",
        "control_binary_sha256",
        "treatment_binary_sha256",
        "host_snapshot_sha256",
        "real_owner_adapter_commit",
        "work_equivalence_witness_sha256",
        "choice_freedom_receipt_sha256",
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
        "work_equivalence_witness_sha256",
        "kill_reopen_witness_sha256",
        "pair_plan_sha256",
    ):
        value = instance.get(field)
        require(
            isinstance(value, str) and HEX64.fullmatch(value),
            "run_instance_incomplete",
            f"{field} must be a SHA-256 digest",
        )
    receipt = instance.get("choice_freedom_receipt_sha256")
    require(
        isinstance(receipt, str) and HEX64.fullmatch(receipt) is not None,
        "choice_freedom_unverified",
        "timed cells need the executable reference's choice-freedom receipt",
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
    exact(len(cases), 32, "negative_fixture_gap")
    probes = fixtures.get("staleness_probes", [])
    require(
        isinstance(probes, list), "negative_fixture_gap", "staleness probes missing"
    )
    exact(len(probes), 3, "negative_fixture_gap")
    harness_probes = fixtures.get("harness_pin_probes", [])
    require(
        isinstance(harness_probes, list),
        "negative_fixture_gap",
        "harness-pin probes missing",
    )
    exact(len(harness_probes), 2, "negative_fixture_gap")
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
    for case in harness_probes:
        try:
            check_harness_pin(**case["inputs"])
        except CheckFailure as error:
            exact(error.code, case["expected_code"], "negative_fixture_mismatch")
        else:
            raise CheckFailure("negative_fixture_survived", case["name"])
    print(
        f"lmdb-dag-control: {len(cases)} manifest mutants, "
        f"{len(probes)} staleness probes and "
        f"{len(harness_probes)} harness-pin probes rejected"
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

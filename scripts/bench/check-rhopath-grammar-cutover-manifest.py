"""Check the pre-registered benchmark plan that compares the two RhoPath key grammars.

The plan is the manifest docs/design/pathmap/rhopath-grammar-cutover-control-manifest.json,
described by docs/design/pathmap/rhopath-grammar-cutover-comparison-protocol.md. This script
measures nothing. It checks that the manifest still holds the plan as it was registered:

  * both arms and their profile IDs, and that the treatment profile is still marked as
    proposed and not buildable;
  * the workloads W0 to W4, with W0 as the only deciding one;
  * the eight primary and the three secondary metrics;
  * the correctness gate that precedes any comparison;
  * the decision rule, its margin and its outcomes, evaluated on measured figures only;
  * the labelled diagnostic for the cost of encode_trie_path, which enters no rule;
  * the sample plan, which must not be smaller than the plan of the parent protocol;
  * the registration: a positive experiment ID, a well-formed UTC time, and every recorded
    sample batch later than the registration;
  * the import by reference: the SHA-256 of the three files of the parent protocol is
    recomputed from this tree, and every imported heading must exist in the parent protocol;
  * the protocol document records the same experiment ID and registration time.

It then replays the negative fixtures of
docs/design/pathmap/rhopath-grammar-cutover-negative-fixtures.json (exactly eight manifest
mutations, each of which must be refused with its own error code) and a list of built-in
probes for the checks that have no fixture. The last line of a successful run states both
counts.

Run it from any directory, without arguments:

    python3 scripts/bench/check-rhopath-grammar-cutover-manifest.py

The options --manifest and --fixtures name other files to check. The script uses the Python
standard library only, reads no clock, opens no network connection and writes no file. It
exits with status 0 on success. On the first failure it prints one line of the form
"rhopath-grammar-cutover: <error code>: <message>" on standard error and exits with status 1.
"""

from __future__ import annotations

import argparse
import copy
import datetime
import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "docs/design/pathmap/rhopath-grammar-cutover-control-manifest.json"
FIXTURES = ROOT / "docs/design/pathmap/rhopath-grammar-cutover-negative-fixtures.json"
PREFIX = "rhopath-grammar-cutover"
TASK = "rspace-key-grammar-cutover-benchmark-protocol"
PROTOCOL = {
    "path": "docs/design/pathmap/rhopath-grammar-cutover-comparison-protocol.md",
    "records_experiment_id_and_registration_time": True,
}
TOP_LEVEL_KEYS = [
    "schema_version",
    "task_public_id",
    "state",
    "registration",
    "sample_batches",
    "protocol",
    "parent_protocol",
    "arms",
    "workloads",
    "primary_metrics",
    "secondary_metrics",
    "correctness_gate",
    "sample_plan",
    "decision_rule",
    "encode_trie_path_diagnostic",
]
PARENT_FILES = {
    "protocol": "docs/design/pathmap/lmdb-dag-equal-contract-comparison-protocol.md",
    "manifest": "docs/design/pathmap/lmdb-dag-equal-contract-control-manifest.json",
    "negative_fixtures": "docs/design/pathmap/lmdb-dag-equal-contract-negative-fixtures.json",
}
PARENT_FROZEN = {
    "task_public_id": "rspace-dag-required-performance-control",
    "experiment_id": 377,
    "import": "by_reference_pinned_by_sha256",
    "imported_sections": [
        "Equal contract and observations",
        "Required matrix and measured quantities",
        "Paired sampling and decision rule",
    ],
    "imported_items": [
        "harness",
        "operation_lists",
        "durability_contract",
        "retention_contract",
        "cache_contract",
        "concurrency_contract",
        "host_rules",
        "paired_sampling",
        "witnesses",
    ],
    "amendment_policy": "pinned_content_stays_imported_until_this_protocol_is_repinned_by_a_recorded_amendment",
    "repin_without_further_ceremony_only_while": "no_sample_batch_is_recorded",
}
PROFILE_IDS = {"control": "RSpace.Rust.V1", "treatment": "RSpace.Rust.V2"}
ARMS = {
    "control": {
        "profile_id": "RSpace.Rust.V1",
        "key_grammar": "RhoPathV1",
        "backend": "durable_pathmap",
        "profile_status": "in_use",
    },
    "treatment": {
        "profile_id": "RSpace.Rust.V2",
        "key_grammar": "RhoPathV2",
        "backend": "durable_pathmap",
        "profile_status": "proposed_awaiting_owner_approval",
        "owner_question": "Q2",
        "buildable_at_preregistration": False,
        "requires_leaves": [
            "rspace-key-grammar-profile-binding",
            "rspace-key-grammar-lifecycle-differential",
            "rspace-key-grammar-durable-lifecycle-differential",
        ],
    },
}
WORKLOADS = [
    {
        "id": "W0",
        "role": "deciding",
        "source": "parent_protocol_operation_lists",
        "cells": "every_timed_cell_of_the_parent_matrix",
    },
    {
        "id": "W1",
        "role": "diagnostic",
        "source": "experiment_487",
        "channel_kind": "unforgeable_name",
        "receive_channels": [1],
    },
    {
        "id": "W2",
        "role": "diagnostic",
        "source": "experiment_487",
        "channel_kind": "tuple_of_unforgeable_name_and_short_string",
        "receive_channels": [1],
    },
    {
        "id": "W3",
        "role": "diagnostic",
        "source": "experiment_487",
        "channel_kind": "large_compound_list_of_16_triples",
        "receive_channels": [1],
    },
    {
        "id": "W4",
        "role": "diagnostic",
        "source": "experiment_487",
        "channel_kind": "unforgeable_name",
        "receive_channels": [2, 3],
    },
]
PRIMARY_METRICS = [
    {
        "id": "produce_wall_time",
        "unit": "seconds_per_call",
        "event": "produce_call_that_completes_no_comm",
        "per_run": "arithmetic_mean",
        "lower_is_better": True,
    },
    {
        "id": "consume_wall_time",
        "unit": "seconds_per_call",
        "event": "consume_call_that_completes_no_comm",
        "per_run": "arithmetic_mean",
        "lower_is_better": True,
    },
    {
        "id": "comm_wall_time",
        "unit": "seconds_per_call",
        "event": "produce_or_consume_call_that_completes_a_comm",
        "per_run": "arithmetic_mean",
        "lower_is_better": True,
    },
    {
        "id": "checkpoint_publication_time",
        "unit": "seconds_per_checkpoint",
        "event": "acknowledged_checkpoint_call",
        "per_run": "arithmetic_mean",
        "lower_is_better": True,
    },
    {
        "id": "durable_bytes_written_per_checkpoint",
        "unit": "bytes_per_checkpoint",
        "event": "acknowledged_checkpoint_call",
        "per_run": "total_physical_write_bytes_divided_by_checkpoints",
        "lower_is_better": True,
    },
    {
        "id": "hot_store_resident_bytes",
        "unit": "bytes",
        "event": "checkpoint_position",
        "per_run": "maximum_live_heap_growth_since_before_open",
        "lower_is_better": True,
    },
    {
        "id": "open_time",
        "unit": "seconds_per_open",
        "event": "open_after_orderly_shutdown",
        "per_run": "arithmetic_mean",
        "lower_is_better": True,
    },
    {
        "id": "recovery_time",
        "unit": "seconds_per_recovery",
        "event": "open_after_kill_during_a_write",
        "per_run": "arithmetic_mean",
        "lower_is_better": True,
    },
]
SECONDARY_METRICS = [
    {
        "id": "key_bytes",
        "unit": "bytes",
        "method": "experiment_487_part_1",
        "enters_decision_rule": False,
    },
    {
        "id": "distinct_trie_edges",
        "unit": "edges",
        "method": "experiment_487_part_2",
        "enters_decision_rule": False,
    },
    {
        "id": "instructions_per_key_construction",
        "unit": "instructions",
        "method": "experiment_487_part_3_cachegrind_slope",
        "enters_decision_rule": False,
    },
]
CORRECTNESS_GATE = {
    "required_before_any_comparison": True,
    "tests": [
        "rspace-key-grammar-lifecycle-differential",
        "rspace-key-grammar-durable-lifecycle-differential",
    ],
    "on_recorded_v1_divergence": "report_to_the_campaign_owner_before_any_comparison",
}
SAMPLE_PLAN_SECTION = "Paired sampling and decision rule"
SAMPLE_PLAN_KEYS = [
    "imported_from_section",
    "primary_cell_id",
    "primary_pairs",
    "secondary_pairs",
    "diagnostic_stratum_pairs",
    "bootstrap_replicates",
    "bootstrap_seed",
    "confidence_percent",
    "randomized_balanced_pair_order",
    "one_pair_one_retained_run",
    "timed_runs_unprofiled",
]
SAMPLE_PLAN_SIZES = {
    "primary_pairs": "primary_pairs",
    "secondary_pairs": "secondary_pairs",
    "diagnostic_stratum_pairs": "secondary_pairs",
    "bootstrap_replicates": "bootstrap_replicates",
}
SAMPLE_PLAN_EQUAL = [
    "bootstrap_seed",
    "confidence_percent",
    "randomized_balanced_pair_order",
    "one_pair_one_retained_run",
    "timed_runs_unprofiled",
]
PARENT_MARGIN_FIELD = "critical_p99_latency_upper_bound_maximum"
DECISION_RULE = {
    "evaluated_on_workload": "W0",
    "evaluated_on": "measured_figures_only",
    "scope": "every_primary_metric_in_every_eligible_timed_cell_of_W0_in_which_every_run_performs_its_event",
    "statistic": "median_of_within_pair_ratios_treatment_over_control",
    "interval": "two_sided_pair_preserving_bootstrap_interval",
    "equivalence_margin_upper_bound": 1.05,
    "worse_beyond_margin_when": "upper_bound_greater_than_margin",
    "better_when": "upper_bound_less_than_one",
    "equivalent_when": "upper_bound_between_one_and_margin_inclusive",
    "outcomes": [
        {
            "id": "recommend_v1",
            "when": "any_comparison_worse_beyond_margin",
            "recommends": "RSpace.Rust.V1",
        },
        {
            "id": "recommend_v2_on_measurement",
            "when": "no_comparison_worse_beyond_margin_and_at_least_one_better",
            "recommends": "RSpace.Rust.V2",
        },
        {
            "id": "recommend_v2_by_tie_break",
            "when": "all_comparisons_equivalent",
            "recommends": "RSpace.Rust.V2",
            "tie_break": "CBR-053_channel_identity_condition",
        },
    ],
    "no_recommendation_when": [
        "correctness_gate_not_passed",
        "a_primary_metric_is_defined_in_no_eligible_timed_cell_of_W0",
        "a_pair_of_runs_fails_a_witness",
    ],
    "recommendation_goes_to": "campaign_owner",
    "stop_points": [6021, 7936],
    "protocol_decides": False,
}
DIAGNOSTIC = {
    "owner_question": "Q5",
    "owner_question_answered_at_preregistration": False,
    "fix_landed_at_preregistration": False,
    "kind": "model_not_measurement",
    "enters_decision_rule": False,
    "source_experiment": 487,
    "source_reading": "cold_production_arms_larger_iteration_pair",
    "share_definition": "instructions_of_arm_v1structure_divided_by_instructions_of_arm_v1",
    "shares": {
        "W1": {
            "encode_trie_path_instructions": 6274.0,
            "v1_datum_prefix_instructions": 17100.2,
        },
        "W2": {
            "encode_trie_path_instructions": 1306000.9,
            "v1_datum_prefix_instructions": 1320425.6,
        },
        "W3": {
            "encode_trie_path_instructions": 63238710.5,
            "v1_datum_prefix_instructions": 63410521.5,
        },
    },
    "no_share_measured_for": ["W0", "W4"],
    "applies_to_arm": "control",
    "applies_to_metric": "instructions_per_key_construction",
    "formula": "modelled_control_instructions = measured_control_instructions * (1 - share)",
    "omitted_when": "the_encode_trie_path_fix_of_owner_question_Q5_has_landed",
}
REQUIRED_FIXTURE_CODES = [
    "arm_dropped",
    "workload_dropped",
    "deciding_workload_drift",
    "primary_metric_dropped",
    "correctness_gate_missing",
    "rule_registered_after_sample",
    "profile_id_not_an_arm",
    "sample_plan_smaller_than_parent",
]
STATES = {False: "preregistered_without_samples", True: "sampling_after_registration"}
HEX40 = re.compile(r"[0-9a-f]{40}\Z")
HEX64 = re.compile(r"[0-9a-f]{64}\Z")
UTC_FORM = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z\Z")
UTC_FORMAT = "%Y-%m-%dT%H:%M:%SZ"
FENCE = re.compile(r" {0,3}(`{3,}|~{3,})")
HEADING = re.compile(r" {0,3}#{1,6}[ \t]+(.*?)[ \t]*#*[ \t]*\Z")


class CheckFailure(Exception):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code


def require(condition: object, code: str, message: str) -> None:
    if not condition:
        raise CheckFailure(code, message)


def same(actual: object, expected: object) -> bool:
    if type(actual) is not type(expected):
        return False
    if isinstance(expected, dict):
        return list(actual) == list(expected) and all(
            same(actual[key], expected[key]) for key in expected
        )
    if isinstance(expected, list):
        return len(actual) == len(expected) and all(
            same(left, right) for left, right in zip(actual, expected)
        )
    return actual == expected


def exact(actual: object, expected: object, code: str, subject: str) -> None:
    require(
        same(actual, expected),
        code,
        f"{subject}: expected {expected!r}, got {actual!r}",
    )


def no_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        require(key not in result, "duplicate_json_key", f"duplicate JSON key: {key}")
        result[key] = value
    return result


def relative_name(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.name


def read_json(path: Path) -> dict:
    try:
        value = json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=no_duplicate_keys
        )
    except (OSError, ValueError) as error:
        raise CheckFailure("invalid_json", f"{relative_name(path)}: {error}") from error
    require(
        isinstance(value, dict),
        "invalid_json",
        f"{relative_name(path)}: the root must be an object",
    )
    return value


def tree_file(name: object) -> Path:
    require(isinstance(name, str), "invalid_path", "an artifact path must be text")
    path = Path(name)
    require(
        not path.is_absolute() and ".." not in path.parts,
        "invalid_path",
        f"{name}: an artifact path must be relative to the repository root",
    )
    resolved = ROOT / path
    require(resolved.is_file(), "missing_artifact", f"{name} does not exist")
    return resolved


def object_at(container: dict, key: str, code: str) -> dict:
    value = container.get(key)
    require(isinstance(value, dict), code, f"{key} must be an object")
    return value


def parse_utc(text: object, code: str, subject: str) -> datetime.datetime:
    require(
        isinstance(text, str) and UTC_FORM.fullmatch(text) is not None,
        code,
        f"{subject} {text!r} is not a UTC time of the form YYYY-MM-DDTHH:MM:SSZ",
    )
    try:
        return datetime.datetime.strptime(text, UTC_FORMAT)
    except ValueError as error:
        raise CheckFailure(code, f"{subject} {text!r}: {error}") from error


def markdown_headings(text: str) -> set[str]:
    headings = set()
    fence = None
    for line in text.split("\n"):
        marker = FENCE.match(line)
        if fence is None:
            if marker is not None:
                fence = marker.group(1)
                continue
            heading = HEADING.fullmatch(line)
            if heading is not None:
                headings.add(heading.group(1))
        elif (
            marker is not None
            and marker.group(1)[0] == fence[0]
            and len(marker.group(1)) >= len(fence)
            and line.strip() == marker.group(1)
        ):
            fence = None
    return headings


def require_headings(wanted: list[str], parent_text: str) -> None:
    present = markdown_headings(parent_text)
    for heading in wanted:
        require(
            heading in present,
            "parent_heading_missing",
            f"the parent protocol has no heading {heading!r}",
        )


def require_protocol_records(text: str, experiment_id: int, registered_at: str) -> None:
    number = re.compile(rf"(?<![0-9]){experiment_id}(?![0-9])")
    require(
        number.search(text) is not None and registered_at in text,
        "protocol_registration_mismatch",
        f"the protocol does not record experiment {experiment_id} registered at "
        f"{registered_at}",
    )


def validate_parent(manifest: dict) -> dict:
    parent = object_at(manifest, "parent_protocol", "parent_import_drift")
    files = object_at(parent, "files", "parent_import_drift")
    exact(list(files), list(PARENT_FILES), "parent_import_drift", "pinned parent files")
    for role, path in PARENT_FILES.items():
        entry = files[role]
        require(
            isinstance(entry, dict) and list(entry) == ["path", "sha256"],
            "parent_import_drift",
            f"the pin of the parent {role} must hold a path and a sha256",
        )
        exact(entry["path"], path, "parent_import_drift", f"path of the parent {role}")
        recorded = entry["sha256"]
        require(
            isinstance(recorded, str) and HEX64.fullmatch(recorded) is not None,
            "parent_pin_mismatch",
            f"{path}: the recorded SHA-256 {recorded!r} is not 64 hexadecimal digits",
        )
        actual = hashlib.sha256(tree_file(path).read_bytes()).hexdigest()
        require(
            actual == recorded,
            "parent_pin_mismatch",
            f"{path}: the manifest pins SHA-256 {recorded}, this tree has {actual}",
        )
    commit = parent.get("pinned_at_commit")
    require(
        isinstance(commit, str) and HEX40.fullmatch(commit) is not None,
        "parent_import_drift",
        f"pinned_at_commit {commit!r} is not a full commit SHA",
    )
    exact(
        {key: value for key, value in parent.items() if key not in ("files", "pinned_at_commit")},
        PARENT_FROZEN,
        "parent_import_drift",
        "import of the parent protocol",
    )
    parent_text = tree_file(PARENT_FILES["protocol"]).read_text(encoding="utf-8")
    require_headings(parent["imported_sections"], parent_text)
    parent_manifest = read_json(tree_file(PARENT_FILES["manifest"]))
    sampling = parent_manifest.get("sampling")
    primary_cell = parent_manifest.get("primary_cell")
    require(
        isinstance(sampling, dict) and isinstance(primary_cell, dict),
        "parent_import_gap",
        "the pinned parent manifest has no sampling plan or no primary cell",
    )
    return {"sampling": sampling, "primary_cell_id": primary_cell.get("cell_id")}


def validate_arms(manifest: dict) -> None:
    arms = manifest.get("arms")
    for name in PROFILE_IDS:
        require(
            isinstance(arms, dict) and isinstance(arms.get(name), dict),
            "arm_dropped",
            f"the {name} arm is missing",
        )
    for name, profile_id in PROFILE_IDS.items():
        found = arms[name].get("profile_id")
        require(
            type(found) is str and found == profile_id,
            "profile_id_not_an_arm",
            f"the {name} arm names profile ID {found!r}; the two arms are "
            f"{PROFILE_IDS['control']} (control) and {PROFILE_IDS['treatment']} (treatment)",
        )
    treatment = arms["treatment"]
    require(
        treatment.get("profile_status") == ARMS["treatment"]["profile_status"]
        and treatment.get("buildable_at_preregistration") is False,
        "premature_approval_claim",
        "the treatment profile must stay marked as proposed, awaiting the owner's "
        "approval, and not buildable",
    )
    exact(arms, ARMS, "arm_drift", "arms")


def validate_workloads(manifest: dict) -> None:
    workloads = manifest.get("workloads")
    require(
        isinstance(workloads, list)
        and all(isinstance(workload, dict) for workload in workloads),
        "workload_dropped",
        "the list of workloads is missing",
    )
    identifiers = [workload.get("id") for workload in workloads]
    for expected in WORKLOADS:
        require(
            expected["id"] in identifiers,
            "workload_dropped",
            f"workload {expected['id']} is missing",
        )
    deciding = [
        workload.get("id") for workload in workloads if workload.get("role") == "deciding"
    ]
    require(
        deciding == ["W0"],
        "deciding_workload_drift",
        f"W0 must be the only deciding workload; the deciding workloads are {deciding!r}",
    )
    exact(workloads, WORKLOADS, "workload_drift", "workloads")


def validate_metrics(manifest: dict) -> None:
    primary = manifest.get("primary_metrics")
    require(
        isinstance(primary, list) and all(isinstance(metric, dict) for metric in primary),
        "primary_metric_dropped",
        "the list of primary metrics is missing",
    )
    identifiers = [metric.get("id") for metric in primary]
    for expected in PRIMARY_METRICS:
        require(
            expected["id"] in identifiers,
            "primary_metric_dropped",
            f"primary metric {expected['id']} is missing",
        )
    exact(primary, PRIMARY_METRICS, "primary_metric_drift", "primary metrics")
    exact(
        manifest.get("secondary_metrics"),
        SECONDARY_METRICS,
        "secondary_metric_drift",
        "secondary metrics",
    )


def validate_gate(manifest: dict) -> None:
    gate = manifest.get("correctness_gate")
    require(
        isinstance(gate, dict)
        and gate.get("required_before_any_comparison") is True
        and isinstance(gate.get("tests"), list)
        and len(gate["tests"]) > 0,
        "correctness_gate_missing",
        "no correctness gate is required before the comparison",
    )
    exact(gate, CORRECTNESS_GATE, "correctness_gate_drift", "correctness gate")


def validate_sample_plan(manifest: dict, parent: dict) -> None:
    plan = object_at(manifest, "sample_plan", "sample_plan_drift")
    sampling = parent["sampling"]
    for field, parent_field in SAMPLE_PLAN_SIZES.items():
        require(
            type(sampling.get(parent_field)) is int,
            "parent_import_gap",
            f"the pinned parent manifest has no whole number sampling.{parent_field}",
        )
        value = plan.get(field)
        require(
            type(value) is int,
            "sample_plan_drift",
            f"{field} must be a whole number, got {value!r}",
        )
        require(
            value >= sampling[parent_field],
            "sample_plan_smaller_than_parent",
            f"{field} is {value}; the parent protocol's {parent_field} is "
            f"{sampling[parent_field]}",
        )
        exact(value, sampling[parent_field], "sample_plan_drift", field)
    for field in SAMPLE_PLAN_EQUAL:
        require(
            field in sampling,
            "parent_import_gap",
            f"the pinned parent manifest has no sampling.{field}",
        )
        exact(plan.get(field), sampling[field], "sample_plan_drift", field)
    exact(
        plan.get("primary_cell_id"),
        parent["primary_cell_id"],
        "sample_plan_drift",
        "primary_cell_id",
    )
    exact(
        plan.get("imported_from_section"),
        SAMPLE_PLAN_SECTION,
        "sample_plan_drift",
        "imported_from_section",
    )
    exact(list(plan), SAMPLE_PLAN_KEYS, "sample_plan_drift", "fields of the sample plan")


def validate_rule(manifest: dict, parent: dict) -> None:
    rule = object_at(manifest, "decision_rule", "decision_rule_drift")
    require(
        rule.get("evaluated_on_workload") == "W0",
        "deciding_workload_drift",
        f"the rule is evaluated on {rule.get('evaluated_on_workload')!r}, not on W0",
    )
    sampling = parent["sampling"]
    require(
        type(sampling.get(PARENT_MARGIN_FIELD)) is float,
        "parent_import_gap",
        f"the pinned parent manifest has no sampling.{PARENT_MARGIN_FIELD}",
    )
    exact(
        rule.get("equivalence_margin_upper_bound"),
        sampling[PARENT_MARGIN_FIELD],
        "margin_drift",
        "equivalence margin",
    )
    exact(rule, DECISION_RULE, "decision_rule_drift", "decision rule")
    diagnostic = object_at(manifest, "encode_trie_path_diagnostic", "diagnostic_drift")
    require(
        diagnostic.get("enters_decision_rule") is False,
        "diagnostic_enters_rule",
        "the modelled figure for encode_trie_path must not enter the decision rule",
    )
    exact(diagnostic, DIAGNOSTIC, "diagnostic_drift", "encode_trie_path diagnostic")


def validate_registration(manifest: dict) -> tuple[int, str]:
    registration = object_at(manifest, "registration", "registration_drift")
    experiment_id = registration.get("experiment_id")
    require(
        type(experiment_id) is int and experiment_id > 0,
        "registration_id_invalid",
        f"the experiment ID {experiment_id!r} is not a positive whole number",
    )
    registered_text = registration.get("registered_at_utc")
    registered = parse_utc(registered_text, "registration_time_invalid", "registration time")
    exact(
        {
            key: value
            for key, value in registration.items()
            if key not in ("experiment_id", "registered_at_utc")
        },
        {"registry": "pgmcp", "registered_before_any_sample": True},
        "registration_drift",
        "registration",
    )
    exact(
        list(registration),
        ["registry", "experiment_id", "registered_at_utc", "registered_before_any_sample"],
        "registration_drift",
        "fields of the registration",
    )
    batches = manifest.get("sample_batches")
    require(
        isinstance(batches, list),
        "sample_batch_invalid",
        "sample_batches must be a list",
    )
    seen = set()
    for batch in batches:
        require(
            isinstance(batch, dict)
            and list(batch) == ["batch_id", "recorded_at_utc"]
            and type(batch["batch_id"]) is str
            and batch["batch_id"] != ""
            and batch["batch_id"] not in seen,
            "sample_batch_invalid",
            f"a sample batch must hold a unique batch_id and a recorded_at_utc: {batch!r}",
        )
        seen.add(batch["batch_id"])
        recorded = parse_utc(
            batch["recorded_at_utc"],
            "sample_batch_invalid",
            f"recording time of sample batch {batch['batch_id']!r}",
        )
        require(
            recorded > registered,
            "rule_registered_after_sample",
            f"sample batch {batch['batch_id']!r} was recorded at "
            f"{batch['recorded_at_utc']}, which is not later than the registration of "
            f"the rule at {registered_text}",
        )
    exact(
        manifest.get("state"),
        STATES[len(batches) > 0],
        "state_drift",
        f"state with {len(batches)} sample batches",
    )
    return experiment_id, registered_text


def validate_manifest(manifest: dict) -> dict:
    exact(manifest.get("schema_version"), 1, "schema_version_drift", "schema_version")
    exact(manifest.get("task_public_id"), TASK, "task_identity_drift", "task_public_id")
    parent = validate_parent(manifest)
    validate_arms(manifest)
    validate_workloads(manifest)
    validate_metrics(manifest)
    validate_gate(manifest)
    validate_sample_plan(manifest, parent)
    validate_rule(manifest, parent)
    experiment_id, registered_text = validate_registration(manifest)
    exact(manifest.get("protocol"), PROTOCOL, "protocol_path_drift", "protocol")
    require_protocol_records(
        tree_file(PROTOCOL["path"]).read_text(encoding="utf-8"),
        experiment_id,
        registered_text,
    )
    exact(list(manifest), TOP_LEVEL_KEYS, "manifest_keys_drift", "fields of the manifest")
    return {
        "arms": len(manifest["arms"]),
        "workloads": len(manifest["workloads"]),
        "primary_metrics": len(manifest["primary_metrics"]),
        "parent_files": len(PARENT_FILES),
        "parent_headings": len(manifest["parent_protocol"]["imported_sections"]),
        "sample_batches": len(manifest["sample_batches"]),
    }


def apply_mutation(manifest: dict, case: dict) -> None:
    parts = case["pointer"].strip("/").split("/")
    current: object = manifest
    for part in parts[:-1]:
        current = current[int(part)] if isinstance(current, list) else current[part]
    last = int(parts[-1]) if isinstance(current, list) else parts[-1]
    if case["op"] == "remove":
        del current[last]
    else:
        current[last] = case["value"]


def expect_refusal(name: str, expected_code: str, action) -> None:
    try:
        action()
    except CheckFailure as error:
        require(
            error.code == expected_code,
            "negative_fixture_mismatch",
            f"{name!r} must be refused with {expected_code}, but was refused with "
            f"{error.code}: {error}",
        )
    else:
        raise CheckFailure(
            "negative_fixture_survived", f"{name!r} was accepted; expected {expected_code}"
        )


def replay_fixtures(manifest: dict, fixtures: dict) -> int:
    exact(fixtures.get("schema_version"), 1, "negative_fixture_schema_drift", "schema_version")
    exact(
        list(fixtures),
        ["schema_version", "manifest_mutations"],
        "negative_fixture_schema_drift",
        "fields of the fixtures file",
    )
    cases = fixtures["manifest_mutations"]
    require(
        isinstance(cases, list) and len(cases) == len(REQUIRED_FIXTURE_CODES),
        "negative_fixture_gap",
        f"exactly {len(REQUIRED_FIXTURE_CODES)} manifest mutations are required",
    )
    for case in cases:
        require(
            isinstance(case, dict)
            and type(case.get("name")) is str
            and type(case.get("pointer")) is str
            and type(case.get("expected_code")) is str
            and case.get("op") in ("remove", "replace")
            and list(case)
            == ["name", "op", "pointer"]
            + (["value"] if case.get("op") == "replace" else [])
            + ["expected_code"],
            "negative_fixture_invalid",
            f"a fixture must hold name, op, pointer, a value when op is replace, and "
            f"expected_code: {case!r}",
        )
    names = [case["name"] for case in cases]
    codes = [case["expected_code"] for case in cases]
    require(
        len(set(names)) == len(names) and sorted(codes) == sorted(REQUIRED_FIXTURE_CODES),
        "negative_fixture_gap",
        f"the fixtures must have distinct names and exactly the codes "
        f"{sorted(REQUIRED_FIXTURE_CODES)!r}; found {sorted(codes)!r}",
    )
    for case in cases:
        mutated = copy.deepcopy(manifest)
        try:
            apply_mutation(mutated, case)
        except (KeyError, IndexError, ValueError, TypeError) as error:
            raise CheckFailure(
                "negative_fixture_invalid",
                f"{case['name']!r}: the pointer {case['pointer']} does not apply: {error!r}",
            ) from error
        expect_refusal(case["name"], case["expected_code"], lambda: validate_manifest(mutated))
    return len(cases)


def later_batch(manifest: dict, seconds: int) -> list[dict]:
    registered = datetime.datetime.strptime(
        manifest["registration"]["registered_at_utc"], UTC_FORMAT
    )
    moment = registered + datetime.timedelta(seconds=seconds)
    return [{"batch_id": "probe", "recorded_at_utc": moment.strftime(UTC_FORMAT)}]


def manifest_probe(manifest: dict, change) -> object:
    def action() -> None:
        mutated = copy.deepcopy(manifest)
        change(mutated)
        validate_manifest(mutated)

    return action


def set_at(pointer: str, value: object):
    return lambda manifest: apply_mutation(
        manifest, {"op": "replace", "pointer": pointer, "value": value}
    )


def swapped_codes(fixtures: dict) -> dict:
    swapped = copy.deepcopy(fixtures)
    first, second = swapped["manifest_mutations"][0], swapped["manifest_mutations"][1]
    first["expected_code"], second["expected_code"] = (
        second["expected_code"],
        first["expected_code"],
    )
    return swapped


def shortened(fixtures: dict) -> dict:
    shorter = copy.deepcopy(fixtures)
    del shorter["manifest_mutations"][-1]
    return shorter


def built_in_probes(manifest: dict, fixtures: dict) -> int:
    zeros = "0" * 64
    probes = [
        (
            "a changed parent protocol breaks its pin",
            "parent_pin_mismatch",
            manifest_probe(manifest, set_at("/parent_protocol/files/protocol/sha256", zeros)),
        ),
        (
            "a changed parent manifest breaks its pin",
            "parent_pin_mismatch",
            manifest_probe(manifest, set_at("/parent_protocol/files/manifest/sha256", zeros)),
        ),
        (
            "changed parent fixtures break their pin",
            "parent_pin_mismatch",
            manifest_probe(
                manifest, set_at("/parent_protocol/files/negative_fixtures/sha256", zeros)
            ),
        ),
        (
            "an imported heading that the parent protocol lacks",
            "parent_heading_missing",
            lambda: require_headings(
                PARENT_FROZEN["imported_sections"],
                "# Title\n\n## Equal contract and observations\n\n"
                "## Required matrix and measured quantities\n",
            ),
        ),
        (
            "an imported heading that occurs only inside a fenced block",
            "parent_heading_missing",
            lambda: require_headings(
                ["Paired sampling and decision rule"],
                "# Title\n\n```text\n## Paired sampling and decision rule\n```\n",
            ),
        ),
        (
            "an import of a section other than the three registered ones",
            "parent_import_drift",
            manifest_probe(
                manifest, set_at("/parent_protocol/imported_sections/2", "References")
            ),
        ),
        (
            "an experiment ID of zero",
            "registration_id_invalid",
            manifest_probe(manifest, set_at("/registration/experiment_id", 0)),
        ),
        (
            "an experiment ID that is a truth value",
            "registration_id_invalid",
            manifest_probe(manifest, set_at("/registration/experiment_id", True)),
        ),
        (
            "a registration time without the UTC form",
            "registration_time_invalid",
            manifest_probe(
                manifest, set_at("/registration/registered_at_utc", "2026-11-12 18:00:00")
            ),
        ),
        (
            "a registration time on a day that does not exist",
            "registration_time_invalid",
            manifest_probe(
                manifest, set_at("/registration/registered_at_utc", "2026-02-30T00:00:00Z")
            ),
        ),
        (
            "a sample batch recorded in the same second as the registration",
            "rule_registered_after_sample",
            manifest_probe(
                manifest,
                lambda mutated: mutated.__setitem__("sample_batches", later_batch(mutated, 0)),
            ),
        ),
        (
            "a later sample batch while the state still claims no samples",
            "state_drift",
            manifest_probe(
                manifest,
                lambda mutated: mutated.__setitem__("sample_batches", later_batch(mutated, 1)),
            ),
        ),
        (
            "a treatment profile marked as approved",
            "premature_approval_claim",
            manifest_probe(manifest, set_at("/arms/treatment/profile_status", "approved")),
        ),
        (
            "a second deciding workload",
            "deciding_workload_drift",
            manifest_probe(manifest, set_at("/workloads/4/role", "deciding")),
        ),
        (
            "a rule evaluated on a diagnostic stratum",
            "deciding_workload_drift",
            manifest_probe(manifest, set_at("/decision_rule/evaluated_on_workload", "W1")),
        ),
        (
            "a margin other than the parent protocol's",
            "margin_drift",
            manifest_probe(
                manifest, set_at("/decision_rule/equivalence_margin_upper_bound", 1.1)
            ),
        ),
        (
            "a rule that also uses modelled figures",
            "decision_rule_drift",
            manifest_probe(
                manifest, set_at("/decision_rule/evaluated_on", "measured_and_modelled_figures")
            ),
        ),
        (
            "a modelled figure that enters the rule",
            "diagnostic_enters_rule",
            manifest_probe(
                manifest, set_at("/encode_trie_path_diagnostic/enters_decision_rule", True)
            ),
        ),
        (
            "a correctness gate with one differential only",
            "correctness_gate_drift",
            manifest_probe(
                manifest,
                set_at("/correctness_gate/tests", ["rspace-key-grammar-lifecycle-differential"]),
            ),
        ),
        (
            "a bootstrap seed other than the parent protocol's",
            "sample_plan_drift",
            manifest_probe(manifest, set_at("/sample_plan/bootstrap_seed", 20261002)),
        ),
        (
            "a protocol that does not record the registration",
            "protocol_registration_mismatch",
            lambda: require_protocol_records(
                "experiment 1234 registered at an unknown time", 123, "2026-01-01T00:00:00Z"
            ),
        ),
        (
            "a fixtures file with seven mutations",
            "negative_fixture_gap",
            lambda: replay_fixtures(manifest, shortened(fixtures)),
        ),
        (
            "two fixtures with exchanged codes",
            "negative_fixture_mismatch",
            lambda: replay_fixtures(manifest, swapped_codes(fixtures)),
        ),
    ]
    for name, expected_code, action in probes:
        try:
            action()
        except CheckFailure as error:
            require(
                error.code == expected_code,
                "built_in_probe_mismatch",
                f"{name!r} must be refused with {expected_code}, but was refused with "
                f"{error.code}: {error}",
            )
        else:
            raise CheckFailure(
                "built_in_probe_survived", f"{name!r} was accepted; expected {expected_code}"
            )
    return len(probes)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Check the pre-registered RhoPath key-grammar cutover benchmark plan."
    )
    parser.add_argument("--manifest", type=Path, default=MANIFEST)
    parser.add_argument("--fixtures", type=Path, default=FIXTURES)
    arguments = parser.parse_args()
    try:
        manifest = read_json(arguments.manifest)
        counts = validate_manifest(manifest)
        fixtures = read_json(arguments.fixtures)
        replayed = replay_fixtures(manifest, fixtures)
        probed = built_in_probes(manifest, fixtures)
    except CheckFailure as error:
        print(f"{PREFIX}: {error.code}: {error}", file=sys.stderr)
        return 1
    print(
        f"{PREFIX}: manifest valid: {counts['arms']} arms, {counts['workloads']} workloads, "
        f"{counts['primary_metrics']} primary metrics, {counts['parent_files']} parent files "
        f"and {counts['parent_headings']} parent headings pinned, "
        f"{counts['sample_batches']} sample batches; nothing is measured by this check"
    )
    print(
        f"{PREFIX}: {replayed} negative fixtures refused, each with its own error code, "
        f"and {probed} built-in probes refused, each with its expected error code"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

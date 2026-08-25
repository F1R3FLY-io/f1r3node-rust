#!/usr/bin/env python3
"""Validate and analyze pgmcp experiment 314's identity-order matrix.

The benchmark emits one CHECK row per case/operation and 82 counterbalanced
AB/BA pairs. This analyzer rejects protocol drift or semantic disagreement
before computing timing, allocation, robust, and lifecycle-frontier evidence.
Bootstrap resampling retains complete arm pairs.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import math
import statistics
from collections import defaultdict
from pathlib import Path
from typing import Any

import numpy as np
from scipy import __version__ as scipy_version
from scipy.stats import mannwhitneyu, shapiro, ttest_ind

SCHEMAS = {
    "rspace-identity-order-index-v1",
    "rspace-identity-order-index-v2-final-persisted-value",
}
NO_DIGEST = "-"
SEED = 0x4131_6028_2026_0825
RETAINED_PAIRS = 82
OPERATIONS = ("egress", "insert_remove", "clone_update", "rebuild_egress")
ARMS = ("arena_sort", "identity_pathmap")
EXPECTED_CASES = {
    ("shared", 1, 1),
    ("shared", 16, 1),
    ("shared", 256, 1),
    ("shared", 4_096, 1),
    ("unshared", 256, 1),
    ("unshared", 4_096, 1),
    ("shared", 16, 2),
    ("shared", 16, 16),
    ("opposed", 256, 1),
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--summary-tsv", type=Path)
    parser.add_argument("--retained-dir", type=Path)
    parser.add_argument("--retained-summary-tsv", type=Path)
    parser.add_argument("--bootstrap-replicates", type=int, default=20_000)
    return parser.parse_args()


def parse_int(value: str, label: str) -> int:
    try:
        parsed = int(value)
    except ValueError as error:
        raise ValueError(f"{label} is not an integer: {value!r}") from error
    if parsed < 0:
        raise ValueError(f"{label} must be nonnegative: {parsed}")
    return parsed


def validate_digest(value: str, output_len: int, line_number: int) -> str:
    if output_len == 0:
        if value != NO_DIGEST:
            raise ValueError(
                f"line {line_number}: an absent digest must be {NO_DIGEST!r}"
            )
        return value
    if len(value) != 64:
        raise ValueError(f"line {line_number}: digest must contain 64 hex digits")
    try:
        bytes.fromhex(value)
    except ValueError as error:
        raise ValueError(f"line {line_number}: digest is not hexadecimal") from error
    return value


def read_and_validate(
    path: Path,
) -> tuple[
    bytes,
    str,
    dict[tuple[str, int, int, int, str], dict[str, Any]],
    dict[tuple[str, int, int, int, str, int], dict[str, dict[str, Any]]],
]:
    raw = path.read_bytes()
    schema: str | None = None
    checks: dict[tuple[str, int, int, int, str], dict[str, Any]] = {}
    pairs: dict[tuple[str, int, int, int, str, int], dict[str, dict[str, Any]]] = (
        defaultdict(dict)
    )

    for line_number, raw_line in enumerate(raw.decode("utf-8").splitlines(), 1):
        fields = raw_line.split("\t")
        if not fields or fields[0] not in {"CHECK", "OBS"}:
            raise ValueError(f"line {line_number}: unknown row kind")
        expected_width = 10 if fields[0] == "CHECK" else 17
        if len(fields) != expected_width:
            raise ValueError(
                f"line {line_number}: expected {expected_width} fields, got {len(fields)}"
            )
        if fields[1] not in SCHEMAS:
            raise ValueError(f"line {line_number}: unexpected schema {fields[1]!r}")
        if schema is None:
            schema = fields[1]
        elif fields[1] != schema:
            raise ValueError(
                f"line {line_number}: mixed schemas {schema!r} and {fields[1]!r}"
            )
        shape = fields[2]
        distinct = parse_int(fields[3], "distinct")
        multiplicity = parse_int(fields[4], "multiplicity")
        logical_cardinality = parse_int(fields[5], "logical cardinality")
        case = (shape, distinct, multiplicity)
        if case not in EXPECTED_CASES:
            raise ValueError(f"line {line_number}: unregistered case {case!r}")
        if logical_cardinality != distinct * multiplicity:
            raise ValueError(f"line {line_number}: incorrect logical cardinality")
        operation = fields[6]
        if operation not in OPERATIONS:
            raise ValueError(f"line {line_number}: unknown operation {operation!r}")
        cell = (shape, distinct, multiplicity, logical_cardinality, operation)

        if fields[0] == "CHECK":
            if cell in checks:
                raise ValueError(
                    f"line {line_number}: duplicate CHECK row for {cell!r}"
                )
            output_len = parse_int(fields[8], "CHECK output length")
            checks[cell] = {
                "logical": parse_int(fields[7], "CHECK logical witness"),
                "output_len": output_len,
                "digest": validate_digest(fields[9], output_len, line_number),
            }
            continue

        pair = parse_int(fields[7], "pair")
        if pair >= RETAINED_PAIRS:
            raise ValueError(f"line {line_number}: pair outside locked range")
        order = fields[8]
        if order not in {"AB", "BA"}:
            raise ValueError(f"line {line_number}: invalid arm order {order!r}")
        arm = fields[9]
        if arm not in ARMS:
            raise ValueError(f"line {line_number}: unknown arm {arm!r}")
        key = (*cell, pair)
        if arm in pairs[key]:
            raise ValueError(f"line {line_number}: duplicate {arm} row for {key!r}")
        elapsed_ns = parse_int(fields[10], "elapsed_ns")
        if elapsed_ns == 0:
            raise ValueError(f"line {line_number}: elapsed_ns must be positive")
        output_len = parse_int(fields[15], "output length")
        pairs[key][arm] = {
            "order": order,
            "elapsed_ns": elapsed_ns,
            "allocations": parse_int(fields[11], "allocations"),
            "allocation_bytes": parse_int(fields[12], "allocation_bytes"),
            "deallocations": parse_int(fields[13], "deallocations"),
            "deallocation_bytes": parse_int(fields[14], "deallocation_bytes"),
            "logical": logical_cardinality
            if operation != "clone_update"
            else logical_cardinality + 1,
            "output_len": output_len,
            "digest": validate_digest(fields[16], output_len, line_number),
        }

    expected_cells = {
        (shape, distinct, multiplicity, distinct * multiplicity, operation)
        for shape, distinct, multiplicity in EXPECTED_CASES
        for operation in OPERATIONS
    }
    if set(checks) != expected_cells:
        raise ValueError(
            f"CHECK cell set differs: missing={expected_cells - set(checks)!r}, "
            f"extra={set(checks) - expected_cells!r}"
        )
    expected_pair_keys = {
        (*cell, pair) for cell in expected_cells for pair in range(RETAINED_PAIRS)
    }
    if set(pairs) != expected_pair_keys:
        raise ValueError(
            f"OBS pair set differs: missing={len(expected_pair_keys - set(pairs))}, "
            f"extra={len(set(pairs) - expected_pair_keys)}"
        )
    for key, arms in pairs.items():
        if set(arms) != set(ARMS):
            raise ValueError(f"pair {key!r} does not contain both arms")
        if arms[ARMS[0]]["order"] != arms[ARMS[1]]["order"]:
            raise ValueError(f"pair {key!r} has inconsistent arm order")
        cell = key[:-1]
        check = checks[cell]
        for arm in ARMS:
            observation = arms[arm]
            for witness in ("logical", "output_len", "digest"):
                if observation[witness] != check[witness]:
                    raise ValueError(
                        f"pair {key!r} arm {arm} disagrees on {witness}: "
                        f"{observation[witness]!r} != {check[witness]!r}"
                    )
    if len(checks) != 36 or len(pairs) != 36 * RETAINED_PAIRS:
        raise AssertionError("locked matrix cardinality changed")
    if schema is None:
        raise ValueError("input contains no protocol rows")
    return raw, schema, checks, pairs


def read_retained_profiles(
    directory: Path,
    schema: str,
) -> tuple[
    dict[tuple[str, int, int], dict[str, dict[str, int | str]]],
    list[dict[str, str]],
]:
    retained: dict[tuple[str, int, int], dict[str, dict[str, int | str]]] = defaultdict(
        dict
    )
    artifacts = []
    paths = sorted(directory.glob("rspace-index-retained-*.tsv"))
    if not paths:
        raise ValueError(f"no retained profiles found in {directory}")
    for path in paths:
        raw = path.read_bytes()
        lines = raw.decode("utf-8").splitlines()
        if len(lines) != 1:
            raise ValueError(f"{path}: expected exactly one RETAINED row")
        fields = lines[0].split("\t")
        if len(fields) != 14 or fields[0] != "RETAINED" or fields[1] != schema:
            raise ValueError(f"{path}: malformed retained-profile row")
        shape = fields[2]
        distinct = parse_int(fields[3], "retained distinct")
        multiplicity = parse_int(fields[4], "retained multiplicity")
        logical_cardinality = parse_int(fields[5], "retained logical cardinality")
        case = (shape, distinct, multiplicity)
        if case not in EXPECTED_CASES:
            raise ValueError(f"{path}: unregistered retained-profile case {case!r}")
        if logical_cardinality != distinct * multiplicity:
            raise ValueError(f"{path}: incorrect retained logical cardinality")
        arm = fields[6]
        if arm not in ARMS:
            raise ValueError(f"{path}: unknown retained-profile arm {arm!r}")
        if arm in retained[case]:
            raise ValueError(f"{path}: duplicate retained profile for {case!r}/{arm}")
        allocation_bytes = parse_int(fields[8], "retained allocation bytes")
        deallocation_bytes = parse_int(fields[10], "retained deallocation bytes")
        retained_bytes = parse_int(fields[11], "retained bytes")
        if allocation_bytes < deallocation_bytes:
            raise ValueError(f"{path}: deallocation bytes exceed allocation bytes")
        if retained_bytes != allocation_bytes - deallocation_bytes:
            raise ValueError(f"{path}: retained-byte witness is inconsistent")
        retained[case][arm] = {
            "allocation_calls": parse_int(fields[7], "retained allocation calls"),
            "allocation_bytes": allocation_bytes,
            "deallocation_calls": parse_int(fields[9], "retained deallocation calls"),
            "deallocation_bytes": deallocation_bytes,
            "retained_bytes": retained_bytes,
            "inline_bytes": parse_int(fields[12], "retained inline bytes"),
            "fixture_type": fields[13],
        }
        artifacts.append(
            {
                "path": str(path),
                "sha256": hashlib.sha256(raw).hexdigest(),
            }
        )

    if set(retained) != EXPECTED_CASES:
        raise ValueError(
            "retained-profile cases differ: "
            f"missing={EXPECTED_CASES - set(retained)!r}, "
            f"extra={set(retained) - EXPECTED_CASES!r}"
        )
    for case, arms in retained.items():
        if set(arms) != set(ARMS):
            raise ValueError(
                f"retained-profile case {case!r} does not contain both arms"
            )
    return retained, artifacts


def sample_summary(values: list[float]) -> dict[str, float | int]:
    return {
        "n": len(values),
        "min": min(values),
        "median": statistics.median(values),
        "mean": statistics.mean(values),
        "sample_stddev": statistics.stdev(values),
        "max": max(values),
    }


def cohen_d(treatment: list[float], control: list[float]) -> float:
    pooled_variance = (
        (len(treatment) - 1) * statistics.variance(treatment)
        + (len(control) - 1) * statistics.variance(control)
    ) / (len(treatment) + len(control) - 2)
    if pooled_variance == 0:
        return (
            0.0 if statistics.mean(treatment) == statistics.mean(control) else math.inf
        )
    return (statistics.mean(treatment) - statistics.mean(control)) / math.sqrt(
        pooled_variance
    )


def cliffs_delta(treatment: list[float], control: list[float]) -> float:
    greater = sum(left > right for left in treatment for right in control)
    less = sum(left < right for left in treatment for right in control)
    return (greater - less) / (len(treatment) * len(control))


def paired_bootstrap_ratio(
    controls: list[float], treatments: list[float], replicates: int, seed_offset: int
) -> dict[str, float]:
    if replicates < 10_000:
        raise ValueError("at least 10,000 bootstrap replicates are required")
    generator = np.random.default_rng(SEED + seed_offset)
    indices = generator.integers(0, len(controls), size=(replicates, len(controls)))
    control_array = np.asarray(controls, dtype=np.float64)
    treatment_array = np.asarray(treatments, dtype=np.float64)
    ratios = np.median(treatment_array[indices], axis=1) / np.median(
        control_array[indices], axis=1
    )
    low, median, high = np.quantile(ratios, (0.005, 0.5, 0.995))
    return {
        "lower_0_5_percent": float(low),
        "median": float(median),
        "upper_99_5_percent": float(high),
    }


def benjamini_hochberg(pvalues: dict[str, float]) -> dict[str, float]:
    ordered = sorted(pvalues.items(), key=lambda item: (item[1], item[0]))
    adjusted: dict[str, float] = {}
    running_minimum = 1.0
    count = len(ordered)
    for reverse_index in range(count - 1, -1, -1):
        key, pvalue = ordered[reverse_index]
        rank = reverse_index + 1
        running_minimum = min(running_minimum, pvalue * count / rank)
        adjusted[key] = min(1.0, running_minimum)
    return adjusted


def analyze_cell(
    cell: tuple[str, int, int, int, str],
    pairs: dict[tuple[str, int, int, int, str, int], dict[str, dict[str, Any]]],
    bootstrap_replicates: int,
    seed_offset: int,
) -> dict[str, Any]:
    observations = [pairs[(*cell, pair)] for pair in range(RETAINED_PAIRS)]
    controls = [float(item[ARMS[0]]["elapsed_ns"]) for item in observations]
    treatments = [float(item[ARMS[1]]["elapsed_ns"]) for item in observations]
    welch = ttest_ind(treatments, controls, equal_var=False, alternative="less")
    robust = mannwhitneyu(treatments, controls, alternative="less")
    control_shapiro = shapiro(controls)
    treatment_shapiro = shapiro(treatments)
    control_mean = statistics.mean(controls)
    treatment_mean = statistics.mean(treatments)
    result: dict[str, Any] = {
        "control_elapsed_ns": sample_summary(controls),
        "treatment_elapsed_ns": sample_summary(treatments),
        "mean_treatment_control_ratio": treatment_mean / control_mean,
        "median_treatment_control_ratio": statistics.median(treatments)
        / statistics.median(controls),
        "mean_treatment_minus_control_ns": treatment_mean - control_mean,
        "one_sided_welch_treatment_less": {
            "statistic": float(welch.statistic),
            "pvalue": float(welch.pvalue),
        },
        "one_sided_mann_whitney_treatment_less": {
            "statistic": float(robust.statistic),
            "pvalue": float(robust.pvalue),
        },
        "cohens_d_treatment_minus_control": cohen_d(treatments, controls),
        "cliffs_delta_treatment_minus_control": cliffs_delta(treatments, controls),
        "shapiro_pvalue": {
            "control": float(control_shapiro.pvalue),
            "treatment": float(treatment_shapiro.pvalue),
        },
        "paired_bootstrap_median_ratio_99_percent": paired_bootstrap_ratio(
            controls, treatments, bootstrap_replicates, seed_offset
        ),
        "order_effect": {},
        "resources": {},
    }
    for order in ("AB", "BA"):
        subset = [item for item in observations if item[ARMS[0]]["order"] == order]
        order_controls = [float(item[ARMS[0]]["elapsed_ns"]) for item in subset]
        order_treatments = [float(item[ARMS[1]]["elapsed_ns"]) for item in subset]
        result["order_effect"][order] = {
            "n_pairs": len(subset),
            "median_treatment_control_ratio": statistics.median(order_treatments)
            / statistics.median(order_controls),
        }
    for metric in (
        "allocations",
        "allocation_bytes",
        "deallocations",
        "deallocation_bytes",
    ):
        control_values = [float(item[ARMS[0]][metric]) for item in observations]
        treatment_values = [float(item[ARMS[1]][metric]) for item in observations]
        result["resources"][metric] = {
            "control_mean": statistics.mean(control_values),
            "treatment_mean": statistics.mean(treatment_values),
            "treatment_minus_control_mean": statistics.mean(treatment_values)
            - statistics.mean(control_values),
        }
    return result


def analyze(
    path: Path,
    raw: bytes,
    schema: str,
    checks: dict[tuple[str, int, int, int, str], dict[str, Any]],
    pairs: dict[tuple[str, int, int, int, str, int], dict[str, dict[str, Any]]],
    bootstrap_replicates: int,
    retained: dict[tuple[str, int, int], dict[str, dict[str, int | str]]] | None,
    retained_artifacts: list[dict[str, str]],
) -> dict[str, Any]:
    cells: dict[str, Any] = {}
    by_case: dict[tuple[str, int, int], dict[str, dict[str, Any]]] = defaultdict(dict)
    for seed_offset, cell in enumerate(sorted(checks)):
        result = analyze_cell(cell, pairs, bootstrap_replicates, seed_offset)
        key = f"{cell[0]}/distinct={cell[1]}/multiplicity={cell[2]}/{cell[4]}"
        cells[key] = result
        by_case[(cell[0], cell[1], cell[2])][cell[4]] = result

    welch_qvalues = benjamini_hochberg(
        {
            key: float(value["one_sided_welch_treatment_less"]["pvalue"])
            for key, value in cells.items()
        }
    )
    robust_qvalues = benjamini_hochberg(
        {
            key: float(value["one_sided_mann_whitney_treatment_less"]["pvalue"])
            for key, value in cells.items()
        }
    )
    for key, value in cells.items():
        value["one_sided_welch_treatment_less"]["benjamini_hochberg_qvalue"] = (
            welch_qvalues[key]
        )
        value["one_sided_mann_whitney_treatment_less"]["benjamini_hochberg_qvalue"] = (
            robust_qvalues[key]
        )

    frontiers: dict[str, Any] = {}
    for case, operations in sorted(by_case.items()):
        egress = operations["egress"]
        mutation = operations["insert_remove"]
        clone = operations["clone_update"]
        rebuild = operations["rebuild_egress"]
        egress_saving = -float(egress["mean_treatment_minus_control_ns"])
        mutation_cycle_overhead = float(mutation["mean_treatment_minus_control_ns"])
        mutation_overhead = mutation_cycle_overhead / 2.0
        clone_overhead = float(clone["mean_treatment_minus_control_ns"])
        break_even = (
            max(0.0, mutation_overhead) / egress_saving if egress_saving > 0 else None
        )
        clone_break_even = (
            max(0.0, clone_overhead) / egress_saving if egress_saving > 0 else None
        )
        lifecycle_control = float(egress["control_elapsed_ns"]["mean"]) + float(
            mutation["control_elapsed_ns"]["mean"]
        )
        lifecycle_treatment = float(egress["treatment_elapsed_ns"]["mean"]) + float(
            mutation["treatment_elapsed_ns"]["mean"]
        )
        key = f"{case[0]}/distinct={case[1]}/multiplicity={case[2]}"
        retained_ratio = None
        if retained is not None:
            control_retained = int(retained[case][ARMS[0]]["retained_bytes"])
            treatment_retained = int(retained[case][ARMS[1]]["retained_bytes"])
            if control_retained == 0:
                raise ValueError(f"control retained bytes are zero for {case!r}")
            retained_ratio = treatment_retained / control_retained
        frontiers[key] = {
            "ready_egress_saving_mean_ns": egress_saving,
            "insert_remove_cycle_overhead_mean_ns": mutation_cycle_overhead,
            "maintenance_overhead_per_mutation_mean_ns": mutation_overhead,
            "break_even_checkpoints_per_mutation": break_even,
            "clone_update_overhead_mean_ns": clone_overhead,
            "clone_break_even_checkpoints_per_update": clone_break_even,
            "one_checkpoint_plus_insert_remove_treatment_control_ratio": lifecycle_treatment
            / lifecycle_control,
            "cold_rebuild_egress_treatment_control_ratio": float(
                rebuild["mean_treatment_control_ratio"]
            ),
            "retained_memory_treatment_control_ratio": retained_ratio,
        }

    ready_ratios = [
        float(operations["egress"]["mean_treatment_control_ratio"])
        for operations in by_case.values()
    ]
    finite_break_even = [
        float(frontier["break_even_checkpoints_per_mutation"])
        for frontier in frontiers.values()
        if frontier["break_even_checkpoints_per_mutation"] is not None
    ]
    retained_ratios = [
        float(frontier["retained_memory_treatment_control_ratio"])
        for frontier in frontiers.values()
        if frontier["retained_memory_treatment_control_ratio"] is not None
    ]
    acceptance_gates = {
        "every_ready_egress_mean_ratio_lte_1_05": max(ready_ratios) <= 1.05,
        "every_cell_has_positive_ready_egress_saving": len(finite_break_even)
        == len(frontiers),
        "every_break_even_checkpoints_per_mutation_lte_1": len(finite_break_even)
        == len(frontiers)
        and max(finite_break_even) <= 1.0,
        "every_ready_egress_welch_q_lte_0_01_and_cohens_d_lte_minus_0_5": all(
            float(
                operations["egress"]["one_sided_welch_treatment_less"][
                    "benjamini_hochberg_qvalue"
                ]
            )
            <= 0.01
            and float(operations["egress"]["cohens_d_treatment_minus_control"]) <= -0.5
            for operations in by_case.values()
        ),
        "retained_memory_ratio_lte_1_10": None
        if not retained_ratios
        else max(retained_ratios) <= 1.10,
    }
    return {
        "experiment_id": 314,
        "schema": schema,
        "input": str(path),
        "input_sha256": hashlib.sha256(raw).hexdigest(),
        "protocol_valid": True,
        "check_rows": len(checks),
        "observation_rows": len(pairs) * len(ARMS),
        "retained_pairs_per_cell": RETAINED_PAIRS,
        "bootstrap_replicates": bootstrap_replicates,
        "bootstrap_seed": hex(SEED),
        "multiple_testing_correction": "benjamini_hochberg_over_all_36_cells",
        "scipy_version": scipy_version,
        "numpy_version": np.__version__,
        "cells": cells,
        "lifecycle_frontiers": frontiers,
        "retained_profiles": None
        if retained is None
        else {
            f"{case[0]}/distinct={case[1]}/multiplicity={case[2]}": arms
            for case, arms in sorted(retained.items())
        },
        "retained_profile_artifacts": retained_artifacts,
        "acceptance_gates": acceptance_gates,
        "all_acceptance_gates_pass": all(
            value is True for value in acceptance_gates.values()
        ),
    }


def render_summary_tsv(result: dict[str, Any]) -> str:
    fields = (
        "case",
        "ready_egress_saving_mean_ns",
        "maintenance_overhead_per_mutation_mean_ns",
        "break_even_checkpoints_per_mutation",
        "clone_update_overhead_mean_ns",
        "clone_break_even_checkpoints_per_update",
        "one_checkpoint_plus_insert_remove_treatment_control_ratio",
        "cold_rebuild_egress_treatment_control_ratio",
        "retained_memory_treatment_control_ratio",
    )
    stream = io.StringIO(newline="")
    writer = csv.DictWriter(
        stream, fieldnames=fields, delimiter="\t", lineterminator="\n"
    )
    writer.writeheader()
    for case, frontier in sorted(result["lifecycle_frontiers"].items()):
        row = {
            field: "NA" if frontier[field] is None else frontier[field]
            for field in fields
            if field != "case"
        }
        writer.writerow(
            {
                "case": case,
                **row,
            }
        )
    return stream.getvalue()


def render_retained_summary_tsv(result: dict[str, Any]) -> str:
    profiles = result["retained_profiles"]
    if profiles is None:
        raise ValueError("--retained-summary-tsv requires --retained-dir")
    fields = (
        "case",
        "arm",
        "allocation_calls",
        "allocation_bytes",
        "deallocation_calls",
        "deallocation_bytes",
        "retained_bytes",
        "inline_bytes",
        "fixture_type",
    )
    stream = io.StringIO(newline="")
    writer = csv.DictWriter(
        stream, fieldnames=fields, delimiter="\t", lineterminator="\n"
    )
    writer.writeheader()
    for case, arms in sorted(profiles.items()):
        for arm, profile in sorted(arms.items()):
            writer.writerow({"case": case, "arm": arm, **profile})
    return stream.getvalue()


def write_exclusive(path: Path, contents: str) -> None:
    with path.open("x", encoding="utf-8", newline="") as stream:
        stream.write(contents)


def main() -> None:
    args = parse_args()
    output_paths = [
        path
        for path in (args.output, args.summary_tsv, args.retained_summary_tsv)
        if path is not None
    ]
    resolved_outputs = [path.resolve() for path in output_paths]
    if len(set(resolved_outputs)) != len(resolved_outputs):
        raise ValueError("analysis output paths must be distinct")
    if args.input.resolve() in resolved_outputs:
        raise ValueError("analysis output must not overwrite raw measurements")
    existing_outputs = [str(path) for path in output_paths if path.exists()]
    if existing_outputs:
        raise FileExistsError(f"analysis outputs already exist: {existing_outputs!r}")
    raw, schema, checks, pairs = read_and_validate(args.input)
    retained, retained_artifacts = (
        (None, [])
        if args.retained_dir is None
        else read_retained_profiles(args.retained_dir, schema)
    )
    result = analyze(
        args.input,
        raw,
        schema,
        checks,
        pairs,
        args.bootstrap_replicates,
        retained,
        retained_artifacts,
    )
    rendered = json.dumps(result, indent=2, sort_keys=True, allow_nan=False) + "\n"
    summary_tsv = None if args.summary_tsv is None else render_summary_tsv(result)
    retained_summary_tsv = (
        None
        if args.retained_summary_tsv is None
        else render_retained_summary_tsv(result)
    )
    if args.output is not None:
        write_exclusive(args.output, rendered)
    if args.summary_tsv is not None and summary_tsv is not None:
        write_exclusive(args.summary_tsv, summary_tsv)
    if args.retained_summary_tsv is not None and retained_summary_tsv is not None:
        write_exclusive(args.retained_summary_tsv, retained_summary_tsv)
    print(rendered, end="")


if __name__ == "__main__":
    main()

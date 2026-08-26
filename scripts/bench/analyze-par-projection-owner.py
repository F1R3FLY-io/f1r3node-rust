#!/usr/bin/env python3
"""Validate and analyze complete-owner ParBag protobuf projection samples.

The parser fails closed on protocol drift, incomplete balanced batches,
counter disagreement, cap violations, and ordering imbalance before computing
paired statistics. The six-arm experiment measures policy diagnostics; the
compile-time two-arm protocol gates the exact production binary without a
statistics encoder monomorphization. Bootstrap resampling retains complete
within-sample pairs.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import statistics
from collections import defaultdict
from pathlib import Path
from typing import Any

import numpy as np
from paired_benchmark_statistics import benjamini_hochberg, paired_statistics
from scipy import __version__ as scipy_version

SCHEMA = "par-projection-owner-v5"
SEED = 0x4131_6027_2026_0826
SIX_ARMS = (
    "adaptive",
    "in_frame_production",
    "in_frame_memo",
    "selective",
    "blanket",
    "selective_too_small",
)
PRODUCTION_ARMS = ("adaptive", "in_frame_production")
CONTROL = "adaptive"
HEADER = (
    "HEADER",
    "width",
    "body_bytes",
    "reuse",
    "output_bytes",
    "projected_bytes",
    "arm",
    "sample",
    "position",
    "elapsed_ns",
    "allocations",
    "allocated_bytes",
    "pathmap_traversals",
    "attempts",
    "prepared",
    "already_prepared",
    "refused_too_large",
    "newly_retained_bytes",
    "remaining_budget",
    "par_occurrences",
    "component_empty_occurrences",
    "eligible_occurrences",
    "unique_owners",
    "repeated_occurrences",
    "repeated_owners",
    "unique_component_bytes",
    "reusable_component_bytes",
    "repeated_component_bytes",
    "census_owner_spilled",
    "transcript_events",
    "transcript_spilled",
    "direct_only_occurrences",
    "range_recorded_owners",
    "range_copied_occurrences",
    "memo_owner_spilled",
    "direct_only_component_bytes",
    "range_recorded_component_bytes",
    "range_copied_component_bytes",
)

Cell = tuple[int, int, int, int, int]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--summary-tsv", type=Path, required=True)
    parser.add_argument("--expected-reps", type=int, default=31)
    parser.add_argument("--bootstrap-replicates", type=int, default=100_000)
    return parser.parse_args()


def parse_nonnegative(value: str, label: str, line: int) -> int:
    try:
        result = int(value)
    except ValueError as error:
        raise ValueError(
            f"line {line}: {label} is not an integer: {value!r}"
        ) from error
    if result < 0:
        raise ValueError(f"line {line}: {label} must be nonnegative")
    return result


def parse_bool(value: str, label: str, line: int) -> bool:
    if value == "true":
        return True
    if value == "false":
        return False
    raise ValueError(f"line {line}: {label} is not true or false: {value!r}")


def parse_meta(fields: list[str], line: int) -> dict[str, str]:
    result: dict[str, str] = {}
    for field in fields[1:]:
        key, separator, value = field.partition("=")
        if not separator or not key or key in result:
            raise ValueError(
                f"line {line}: malformed or duplicate META field {field!r}"
            )
        result[key] = value
    required = {
        "format",
        "reps",
        "warmup",
        "ordering",
        "timed",
        "fixture",
        "traversal_counter",
    }
    actual = set(result)
    if actual != required:
        raise ValueError(
            f"line {line}: META fields differ; missing={required - actual!r}, "
            f"unexpected={actual - required!r}"
        )
    if result["format"] != SCHEMA:
        raise ValueError(f"line {line}: unexpected format {result['format']!r}")
    if result["ordering"] not in {
        "balanced-six-arm-rotation",
        "balanced-two-arm-production-rotation",
    }:
        raise ValueError(f"line {line}: unexpected ordering protocol")
    if result["timed"] != "policy-plus-complete-frame":
        raise ValueError(f"line {line}: incomplete timed region")
    if result["fixture"] != "soft-checkpoint-shared-aux":
        raise ValueError(f"line {line}: unexpected owner fixture")
    if result["traversal_counter"] != "emission-only":
        raise ValueError(f"line {line}: unexpected traversal-counter scope")
    return result


def expected_resources(
    arm: str, reuse: int, projected_bytes: int
) -> dict[str, int | bool]:
    no_frame_memo = {
        "par_occurrences": 0,
        "component_empty_occurrences": 0,
        "eligible_occurrences": 0,
        "unique_owners": 0,
        "repeated_occurrences": 0,
        "repeated_owners": 0,
        "unique_component_bytes": 0,
        "reusable_component_bytes": 0,
        "repeated_component_bytes": 0,
        "census_owner_spilled": False,
        "transcript_events": 0,
        "transcript_spilled": False,
        "direct_only_occurrences": 0,
        "range_recorded_owners": 0,
        "range_copied_occurrences": 0,
        "memo_owner_spilled": False,
        "direct_only_component_bytes": 0,
        "range_recorded_component_bytes": 0,
        "range_copied_component_bytes": 0,
    }
    if arm == "adaptive":
        return {
            "pathmap_traversals": reuse,
            "attempts": 0,
            "prepared": 0,
            "already_prepared": 0,
            "refused_too_large": 0,
            "newly_retained_bytes": 0,
            "remaining_budget": 0,
            **no_frame_memo,
        }
    if arm == "in_frame_production":
        return {
            "pathmap_traversals": 1,
            "attempts": 0,
            "prepared": 0,
            "already_prepared": 0,
            "refused_too_large": 0,
            "newly_retained_bytes": 0,
            "remaining_budget": 0,
            **no_frame_memo,
        }
    if arm == "in_frame_memo":
        return {
            "pathmap_traversals": 1,
            "attempts": 0,
            "prepared": 0,
            "already_prepared": 0,
            "refused_too_large": 0,
            "newly_retained_bytes": 0,
            "remaining_budget": 0,
            "par_occurrences": reuse,
            "component_empty_occurrences": 0,
            "eligible_occurrences": reuse,
            "unique_owners": 1,
            "repeated_occurrences": reuse - 1,
            "repeated_owners": int(reuse > 1),
            "unique_component_bytes": projected_bytes,
            "reusable_component_bytes": projected_bytes * int(reuse > 1),
            "repeated_component_bytes": projected_bytes * (reuse - 1),
            "census_owner_spilled": False,
            "transcript_events": reuse + 9,
            "transcript_spilled": reuse + 9 > 16,
            "direct_only_occurrences": int(reuse == 1),
            "range_recorded_owners": int(reuse > 1),
            "range_copied_occurrences": reuse - 1,
            "memo_owner_spilled": False,
            "direct_only_component_bytes": projected_bytes * int(reuse == 1),
            "range_recorded_component_bytes": projected_bytes * int(reuse > 1),
            "range_copied_component_bytes": projected_bytes * (reuse - 1),
        }
    if arm == "selective":
        return {
            "pathmap_traversals": 1,
            "attempts": 1,
            "prepared": 1,
            "already_prepared": 0,
            "refused_too_large": 0,
            "newly_retained_bytes": projected_bytes,
            "remaining_budget": 0,
            **no_frame_memo,
        }
    if arm == "blanket":
        return {
            "pathmap_traversals": 1,
            "attempts": reuse,
            "prepared": 1,
            "already_prepared": reuse - 1,
            "refused_too_large": 0,
            "newly_retained_bytes": projected_bytes,
            "remaining_budget": 0,
            **no_frame_memo,
        }
    if arm == "selective_too_small":
        return {
            "pathmap_traversals": reuse,
            "attempts": 1,
            "prepared": 0,
            "already_prepared": 0,
            "refused_too_large": 1,
            "newly_retained_bytes": 0,
            "remaining_budget": projected_bytes,
            **no_frame_memo,
        }
    raise ValueError(f"unknown arm {arm!r}")


def read_protocol(
    path: Path, expected_reps: int
) -> tuple[
    bytes,
    dict[str, str],
    tuple[str, ...],
    list[str],
    dict[Cell, dict[int, dict[str, Any]]],
]:
    raw = path.read_bytes()
    meta: dict[str, str] | None = None
    arms: tuple[str, ...] | None = None
    loadavg: list[str] = []
    saw_header = False
    cells: dict[Cell, dict[int, dict[str, Any]]] = defaultdict(
        lambda: defaultdict(dict)
    )

    for line_number, line in enumerate(raw.decode("utf-8").splitlines(), 1):
        if not line or line.startswith("Running as unit:"):
            continue
        fields = line.split("\t")
        if fields[0] == "META":
            if meta is not None:
                raise ValueError(f"line {line_number}: duplicate META row")
            meta = parse_meta(fields, line_number)
            arms = (
                PRODUCTION_ARMS
                if meta["ordering"] == "balanced-two-arm-production-rotation"
                else SIX_ARMS
            )
            continue
        if fields[0] == "HEADER":
            if saw_header or tuple(fields) != HEADER:
                raise ValueError(f"line {line_number}: unexpected or duplicate HEADER")
            saw_header = True
            continue
        if fields[0] == "LOADAVG":
            loadavg.append("\t".join(fields[1:]))
            continue
        if fields[0] != "OBS":
            raise ValueError(f"line {line_number}: unknown row kind {fields[0]!r}")
        if arms is None:
            raise ValueError(f"line {line_number}: OBS row precedes META")
        if len(fields) != len(HEADER):
            raise ValueError(
                f"line {line_number}: expected {len(HEADER)} fields, got {len(fields)}"
            )
        width = parse_nonnegative(fields[1], "width", line_number)
        body_bytes = parse_nonnegative(fields[2], "body bytes", line_number)
        reuse = parse_nonnegative(fields[3], "reuse", line_number)
        output_bytes = parse_nonnegative(fields[4], "output bytes", line_number)
        projected_bytes = parse_nonnegative(fields[5], "projected bytes", line_number)
        arm = fields[6]
        sample = parse_nonnegative(fields[7], "sample", line_number)
        position = parse_nonnegative(fields[8], "position", line_number)
        if width == 0 or body_bytes == 0 or reuse == 0:
            raise ValueError(
                f"line {line_number}: benchmark dimensions must be positive"
            )
        if output_bytes == 0 or projected_bytes == 0:
            raise ValueError(f"line {line_number}: byte witnesses must be positive")
        if arm not in arms:
            raise ValueError(f"line {line_number}: unknown arm {arm!r}")
        if sample >= expected_reps:
            raise ValueError(f"line {line_number}: sample is outside the locked range")
        if position >= len(arms):
            raise ValueError(f"line {line_number}: position is outside the arm batch")
        cell = (width, body_bytes, reuse, output_bytes, projected_bytes)
        if arm in cells[cell][sample]:
            raise ValueError(
                f"line {line_number}: duplicate {arm} observation for {cell!r}, sample {sample}"
            )
        observation = {
            "position": position,
            "elapsed_ns": parse_nonnegative(fields[9], "elapsed ns", line_number),
            "allocations": parse_nonnegative(fields[10], "allocations", line_number),
            "allocated_bytes": parse_nonnegative(
                fields[11], "allocated bytes", line_number
            ),
            "pathmap_traversals": parse_nonnegative(
                fields[12], "PathMap traversals", line_number
            ),
            "attempts": parse_nonnegative(fields[13], "attempts", line_number),
            "prepared": parse_nonnegative(fields[14], "prepared", line_number),
            "already_prepared": parse_nonnegative(
                fields[15], "already prepared", line_number
            ),
            "refused_too_large": parse_nonnegative(
                fields[16], "refused too large", line_number
            ),
            "newly_retained_bytes": parse_nonnegative(
                fields[17], "newly retained bytes", line_number
            ),
            "remaining_budget": parse_nonnegative(
                fields[18], "remaining budget", line_number
            ),
            "par_occurrences": parse_nonnegative(
                fields[19], "Par occurrences", line_number
            ),
            "component_empty_occurrences": parse_nonnegative(
                fields[20], "component-empty occurrences", line_number
            ),
            "eligible_occurrences": parse_nonnegative(
                fields[21], "eligible occurrences", line_number
            ),
            "unique_owners": parse_nonnegative(
                fields[22], "unique owners", line_number
            ),
            "repeated_occurrences": parse_nonnegative(
                fields[23], "repeated occurrences", line_number
            ),
            "repeated_owners": parse_nonnegative(
                fields[24], "repeated owners", line_number
            ),
            "unique_component_bytes": parse_nonnegative(
                fields[25], "unique component bytes", line_number
            ),
            "reusable_component_bytes": parse_nonnegative(
                fields[26], "reusable component bytes", line_number
            ),
            "repeated_component_bytes": parse_nonnegative(
                fields[27], "repeated component bytes", line_number
            ),
            "census_owner_spilled": parse_bool(
                fields[28], "census owner spill", line_number
            ),
            "transcript_events": parse_nonnegative(
                fields[29], "transcript events", line_number
            ),
            "transcript_spilled": parse_bool(
                fields[30], "transcript spill", line_number
            ),
            "direct_only_occurrences": parse_nonnegative(
                fields[31], "direct-only occurrences", line_number
            ),
            "range_recorded_owners": parse_nonnegative(
                fields[32], "range-recorded owners", line_number
            ),
            "range_copied_occurrences": parse_nonnegative(
                fields[33], "range-copied occurrences", line_number
            ),
            "memo_owner_spilled": parse_bool(
                fields[34], "memo owner spill", line_number
            ),
            "direct_only_component_bytes": parse_nonnegative(
                fields[35], "direct-only component bytes", line_number
            ),
            "range_recorded_component_bytes": parse_nonnegative(
                fields[36], "range-recorded component bytes", line_number
            ),
            "range_copied_component_bytes": parse_nonnegative(
                fields[37], "range-copied component bytes", line_number
            ),
        }
        if observation["elapsed_ns"] == 0:
            raise ValueError(f"line {line_number}: elapsed time must be positive")
        expected = expected_resources(arm, reuse, projected_bytes)
        for metric, value in expected.items():
            if observation[metric] != value:
                raise ValueError(
                    f"line {line_number}: {arm} {metric} is "
                    f"{observation[metric]}, expected {value}"
                )
        cells[cell][sample][arm] = observation

    if meta is None or arms is None or not saw_header:
        raise ValueError("input is missing META or HEADER")
    if parse_nonnegative(meta["reps"], "META reps", 0) != expected_reps:
        raise ValueError("META reps differs from --expected-reps")
    warmup = parse_nonnegative(meta["warmup"], "META warmup", 0)
    if not cells:
        raise ValueError("input contains no observations")
    for cell, samples in cells.items():
        if set(samples) != set(range(expected_reps)):
            raise ValueError(f"cell {cell!r} has an incomplete sample range")
        for sample, observations in samples.items():
            if set(observations) != set(arms):
                raise ValueError(f"cell {cell!r}, sample {sample} lacks all arms")
            positions = {item["position"] for item in observations.values()}
            if positions != set(range(len(arms))):
                raise ValueError(f"cell {cell!r}, sample {sample} has bad positions")
            for arm, observation in observations.items():
                expected_arm = arms[
                    (sample + warmup + observation["position"]) % len(arms)
                ]
                if arm != expected_arm:
                    raise ValueError(
                        f"cell {cell!r}, sample {sample} violates balanced rotation"
                    )
    return raw, meta, arms, loadavg, cells


def cell_name(cell: Cell) -> str:
    width, body, reuse, output, projected = cell
    return (
        f"width={width}/body={body}/reuse={reuse}/output={output}/projected={projected}"
    )


def main() -> None:
    args = parse_args()
    if args.expected_reps < 3:
        raise ValueError("at least three paired observations are required")
    raw, meta, arms, loadavg, cells = read_protocol(args.input, args.expected_reps)
    results: dict[str, Any] = {}
    tsv_rows: list[list[str]] = []
    comparisons_by_key: dict[str, dict[str, Any]] = {}
    eligible_control: list[float] = []
    eligible_in_frame_production: list[float] = []
    eligible_unit_keys: list[str] = []
    reuse_one_control: list[float] = []
    reuse_one_in_frame_production: list[float] = []
    reuse_one_unit_keys: list[str] = []

    for cell_index, cell in enumerate(sorted(cells)):
        samples = cells[cell]
        name = cell_name(cell)
        control = [
            float(samples[index][CONTROL]["elapsed_ns"])
            for index in range(args.expected_reps)
        ]
        comparisons: dict[str, Any] = {}
        for arm_index, arm in enumerate(arms[1:], 1):
            treatment = [
                float(samples[index][arm]["elapsed_ns"])
                for index in range(args.expected_reps)
            ]
            comparison = paired_statistics(
                control,
                treatment,
                args.bootstrap_replicates,
                SEED + cell_index * len(arms) + arm_index,
            )
            comparison["paired_difference_ns"] = comparison.pop("paired_difference")
            comparison["position_median_ns"] = {
                str(position): statistics.median(
                    float(observations[arm]["elapsed_ns"])
                    for observations in samples.values()
                    if observations[arm]["position"] == position
                )
                for position in range(len(arms))
                if any(
                    observations[arm]["position"] == position
                    for observations in samples.values()
                )
            }
            comparisons[arm] = comparison
            comparisons_by_key[f"{name}|{arm}"] = comparison
            interval = comparison["paired_bootstrap_median_of_ratios_99_percent"]
            tsv_rows.append(
                [
                    name,
                    arm,
                    str(comparison["paired_ratio"]["median"]),
                    str(interval["lower_0_5_percent"]),
                    str(interval["upper_99_5_percent"]),
                    str(comparison["paired_wilcoxon_two_sided_pvalue"]),
                ]
            )
        resources = {
            arm: {
                metric: sorted(
                    {samples[index][arm][metric] for index in range(args.expected_reps)}
                )
                for metric in (
                    "allocations",
                    "allocated_bytes",
                    "pathmap_traversals",
                    "attempts",
                    "prepared",
                    "already_prepared",
                    "refused_too_large",
                    "newly_retained_bytes",
                    "remaining_budget",
                    "par_occurrences",
                    "component_empty_occurrences",
                    "eligible_occurrences",
                    "unique_owners",
                    "repeated_occurrences",
                    "repeated_owners",
                    "unique_component_bytes",
                    "reusable_component_bytes",
                    "repeated_component_bytes",
                    "census_owner_spilled",
                    "transcript_events",
                    "transcript_spilled",
                    "direct_only_occurrences",
                    "range_recorded_owners",
                    "range_copied_occurrences",
                    "memo_owner_spilled",
                    "direct_only_component_bytes",
                    "range_recorded_component_bytes",
                    "range_copied_component_bytes",
                )
            }
            for arm in arms
        }
        results[name] = {
            "comparisons_against_adaptive": comparisons,
            "resources": resources,
        }

        if cell[2] >= 2:
            for sample in range(args.expected_reps):
                eligible_control.append(float(samples[sample][CONTROL]["elapsed_ns"]))
                eligible_in_frame_production.append(
                    float(samples[sample]["in_frame_production"]["elapsed_ns"])
                )
                eligible_unit_keys.append(f"{name}/sample={sample}")
        else:
            for sample in range(args.expected_reps):
                reuse_one_control.append(float(samples[sample][CONTROL]["elapsed_ns"]))
                reuse_one_in_frame_production.append(
                    float(samples[sample]["in_frame_production"]["elapsed_ns"])
                )
                reuse_one_unit_keys.append(f"{name}/sample={sample}")

    paired_t_qvalues = benjamini_hochberg(
        {
            key: comparison["paired_t_two_sided_pvalue"]
            for key, comparison in comparisons_by_key.items()
        }
    )
    signed_rank_qvalues = benjamini_hochberg(
        {
            key: comparison["paired_wilcoxon_two_sided_pvalue"]
            for key, comparison in comparisons_by_key.items()
        }
    )
    for key, comparison in comparisons_by_key.items():
        comparison["paired_t_benjamini_hochberg_qvalue"] = paired_t_qvalues[key]
        comparison["paired_wilcoxon_benjamini_hochberg_qvalue"] = signed_rank_qvalues[
            key
        ]

    aggregate: dict[str, Any] | None = None
    if eligible_control:
        aggregate = paired_statistics(
            eligible_control,
            eligible_in_frame_production,
            args.bootstrap_replicates,
            SEED ^ 0xA661_E6A7,
        )
        aggregate["paired_difference_ns"] = aggregate.pop("paired_difference")
        aggregate["treatment_arm"] = "in_frame_production"
        aggregate["eligibility"] = "reuse >= 2"
        aggregate["decision_role"] = "primary"
        aggregate["unit_keys"] = eligible_unit_keys

    reuse_one_non_regression: dict[str, Any] | None = None
    if reuse_one_control:
        reuse_one_non_regression = paired_statistics(
            reuse_one_control,
            reuse_one_in_frame_production,
            args.bootstrap_replicates,
            SEED ^ 0x1F4A_4D31,
        )
        reuse_one_non_regression["paired_difference_ns"] = reuse_one_non_regression.pop(
            "paired_difference"
        )
        reuse_one_non_regression["treatment_arm"] = "in_frame_production"
        reuse_one_non_regression["eligibility"] = "reuse == 1"
        reuse_one_non_regression["decision_role"] = "non-regression"
        reuse_one_non_regression["unit_keys"] = reuse_one_unit_keys

    gate_failures: list[str] = []
    for cell in sorted(cells):
        name = cell_name(cell)
        reuse = cell[2]
        comparison = results[name]["comparisons_against_adaptive"][
            "in_frame_production"
        ]
        upper = comparison["paired_bootstrap_median_of_ratios_99_percent"][
            "upper_99_5_percent"
        ]
        if reuse == 1:
            if upper > 1.05:
                gate_failures.append(
                    f"{name}: no-reuse 99% upper ratio {upper:.6f} exceeds 1.05"
                )
            for sample, observations in cells[cell].items():
                certified = observations["in_frame_production"]["allocations"]
                control = observations[CONTROL]["allocations"]
                if certified > control:
                    gate_failures.append(
                        f"{name}/sample={sample}: singleton certified allocations "
                        f"{certified} exceed control {control}"
                    )
        elif upper >= 1.0:
            gate_failures.append(
                f"{name}: reusable-owner 99% upper ratio {upper:.6f} is not below 1.0"
            )

    if aggregate is None:
        gate_failures.append("primary reuse>=2 aggregate is absent")
    else:
        aggregate_upper = aggregate["paired_bootstrap_median_of_ratios_99_percent"][
            "upper_99_5_percent"
        ]
        if aggregate_upper > 0.90:
            gate_failures.append(
                f"primary reuse>=2 aggregate 99% upper ratio {aggregate_upper:.6f} "
                "exceeds 0.90"
            )

    report = {
        "schema": SCHEMA,
        "input": str(args.input),
        "input_sha256": hashlib.sha256(raw).hexdigest(),
        "meta": meta,
        "arms": arms,
        "loadavg": loadavg,
        "paired_samples_per_cell": args.expected_reps,
        "bootstrap_replicates": args.bootstrap_replicates,
        "bootstrap_seed": hex(SEED),
        "python": platform.python_version(),
        "numpy": np.__version__,
        "scipy": scipy_version,
        "protocol_validation": "passed",
        "primary_eligible_in_frame_production_aggregate": aggregate,
        "reuse_one_in_frame_production_non_regression": reuse_one_non_regression,
        "decision_gate": {
            "passed": not gate_failures,
            "rules": {
                "reuse_one_99_percent_upper_max": 1.05,
                "reuse_one_certified_allocations_lte_control": True,
                "each_reusable_cell_99_percent_upper_strict_max": 1.0,
                "aggregate_reusable_99_percent_upper_max": 0.90,
            },
            "failures": gate_failures,
        },
        "cells": results,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.summary_tsv.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    with args.summary_tsv.open("w", encoding="utf-8", newline="") as output:
        output.write(
            "cell\ttreatment\tpaired_median_ratio\tbootstrap_99_low\t"
            "bootstrap_99_high\twilcoxon_pvalue\n"
        )
        for row in tsv_rows:
            output.write("\t".join(row) + "\n")
    if gate_failures:
        raise SystemExit("benchmark decision gate failed: " + "; ".join(gate_failures))


if __name__ == "__main__":
    main()

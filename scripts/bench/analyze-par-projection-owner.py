#!/usr/bin/env python3
"""Validate and analyze complete-owner ParBag protobuf projection samples.

The parser fails closed on protocol drift, incomplete four-arm batches,
counter disagreement, cap violations, and ordering imbalance before computing
paired statistics. Bootstrap resampling retains complete within-sample pairs.
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

SCHEMA = "par-projection-owner-v1"
SEED = 0x4131_6027_2026_0826
ARMS = ("adaptive", "selective", "blanket", "selective_too_small")
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


def parse_meta(fields: list[str], line: int) -> dict[str, str]:
    result: dict[str, str] = {}
    for field in fields[1:]:
        key, separator, value = field.partition("=")
        if not separator or not key or key in result:
            raise ValueError(
                f"line {line}: malformed or duplicate META field {field!r}"
            )
        result[key] = value
    required = {"format", "reps", "warmup", "ordering", "timed", "fixture"}
    if not required <= result.keys():
        raise ValueError(f"line {line}: META is missing {required - result.keys()!r}")
    if result["format"] != SCHEMA:
        raise ValueError(f"line {line}: unexpected format {result['format']!r}")
    if result["ordering"] != "balanced-four-arm-rotation":
        raise ValueError(f"line {line}: unexpected ordering protocol")
    if result["timed"] != "prepare-plus-complete-frame":
        raise ValueError(f"line {line}: incomplete timed region")
    if result["fixture"] != "soft-checkpoint-shared-aux":
        raise ValueError(f"line {line}: unexpected owner fixture")
    return result


def expected_resources(arm: str, reuse: int, projected_bytes: int) -> dict[str, int]:
    if arm == "adaptive":
        return {
            "pathmap_traversals": reuse,
            "attempts": 0,
            "prepared": 0,
            "already_prepared": 0,
            "refused_too_large": 0,
            "newly_retained_bytes": 0,
            "remaining_budget": 0,
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
        }
    raise ValueError(f"unknown arm {arm!r}")


def read_protocol(
    path: Path, expected_reps: int
) -> tuple[bytes, dict[str, str], list[str], dict[Cell, dict[int, dict[str, Any]]]]:
    raw = path.read_bytes()
    meta: dict[str, str] | None = None
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
        if arm not in ARMS:
            raise ValueError(f"line {line_number}: unknown arm {arm!r}")
        if sample >= expected_reps:
            raise ValueError(f"line {line_number}: sample is outside the locked range")
        if position >= len(ARMS):
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

    if meta is None or not saw_header:
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
            if set(observations) != set(ARMS):
                raise ValueError(f"cell {cell!r}, sample {sample} lacks all arms")
            positions = {item["position"] for item in observations.values()}
            if positions != set(range(len(ARMS))):
                raise ValueError(f"cell {cell!r}, sample {sample} has bad positions")
            for arm, observation in observations.items():
                expected_arm = ARMS[
                    (sample + warmup + observation["position"]) % len(ARMS)
                ]
                if arm != expected_arm:
                    raise ValueError(
                        f"cell {cell!r}, sample {sample} violates balanced rotation"
                    )
    return raw, meta, loadavg, cells


def cell_name(cell: Cell) -> str:
    width, body, reuse, output, projected = cell
    return (
        f"width={width}/body={body}/reuse={reuse}/output={output}/projected={projected}"
    )


def main() -> None:
    args = parse_args()
    if args.expected_reps < 3:
        raise ValueError("at least three paired observations are required")
    raw, meta, loadavg, cells = read_protocol(args.input, args.expected_reps)
    results: dict[str, Any] = {}
    tsv_rows: list[list[str]] = []
    comparisons_by_key: dict[str, dict[str, Any]] = {}
    eligible_control: list[float] = []
    eligible_selective: list[float] = []
    eligible_unit_keys: list[str] = []

    for cell_index, cell in enumerate(sorted(cells)):
        samples = cells[cell]
        name = cell_name(cell)
        control = [
            float(samples[index][CONTROL]["elapsed_ns"])
            for index in range(args.expected_reps)
        ]
        comparisons: dict[str, Any] = {}
        for arm_index, arm in enumerate(ARMS[1:], 1):
            treatment = [
                float(samples[index][arm]["elapsed_ns"])
                for index in range(args.expected_reps)
            ]
            comparison = paired_statistics(
                control,
                treatment,
                args.bootstrap_replicates,
                SEED + cell_index * len(ARMS) + arm_index,
            )
            comparison["paired_difference_ns"] = comparison.pop("paired_difference")
            comparison["position_median_ns"] = {
                str(position): statistics.median(
                    float(observations[arm]["elapsed_ns"])
                    for observations in samples.values()
                    if observations[arm]["position"] == position
                )
                for position in range(len(ARMS))
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
                )
            }
            for arm in ARMS
        }
        results[name] = {
            "comparisons_against_adaptive": comparisons,
            "resources": resources,
        }

        if cell[2] >= 2:
            for sample in range(args.expected_reps):
                eligible_control.append(float(samples[sample][CONTROL]["elapsed_ns"]))
                eligible_selective.append(
                    float(samples[sample]["selective"]["elapsed_ns"])
                )
                eligible_unit_keys.append(f"{name}/sample={sample}")

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
            eligible_selective,
            args.bootstrap_replicates,
            SEED ^ 0xA661_E6A7,
        )
        aggregate["paired_difference_ns"] = aggregate.pop("paired_difference")
        aggregate["unit_keys"] = eligible_unit_keys

    report = {
        "schema": SCHEMA,
        "input": str(args.input),
        "input_sha256": hashlib.sha256(raw).hexdigest(),
        "meta": meta,
        "loadavg": loadavg,
        "paired_samples_per_cell": args.expected_reps,
        "bootstrap_replicates": args.bootstrap_replicates,
        "bootstrap_seed": hex(SEED),
        "python": platform.python_version(),
        "numpy": np.__version__,
        "scipy": scipy_version,
        "protocol_validation": "passed",
        "eligible_selective_aggregate": aggregate,
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


if __name__ == "__main__":
    main()

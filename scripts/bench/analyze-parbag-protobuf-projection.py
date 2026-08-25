#!/usr/bin/env python3
"""Validate and analyze the ParBag protobuf projection-policy experiment.

The benchmark counterbalances five arms within every sample. This analyzer
rejects protocol drift, incomplete cells, byte/resource-counter disagreement,
and ordering drift before computing paired 99% bootstrap intervals. Bootstrap
resampling retains complete within-sample pairs.
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
from scipy import __version__ as scipy_version
from scipy.stats import shapiro, ttest_rel, wilcoxon

SCHEMA = "parbag-protobuf-projection-v5"
SEED = 0x4131_6027_2026_0825
ARMS = (
    "direct",
    "cold_cached",
    "warm_cached",
    "cold_adaptive",
    "warm_adaptive",
)
CONTROL = "direct"
HEADER = (
    "HEADER",
    "family",
    "width",
    "body_bytes",
    "depth",
    "multiplicity",
    "output_bytes",
    "arm",
    "sink",
    "sample",
    "position",
    "elapsed_ns",
    "allocations",
    "allocated_bytes",
    "pathmap_traversals",
    "sink_input_bytes",
    "newly_retained_projection_bytes",
    "retained_projection_bytes",
)


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
    required = {
        "format",
        "mode",
        "reps",
        "warmup",
        "ordering",
        "byte_metrics",
    }
    if not required <= result.keys():
        raise ValueError(f"line {line}: META is missing {required - result.keys()!r}")
    if result["format"] != SCHEMA:
        raise ValueError(f"line {line}: unexpected format {result['format']!r}")
    if result["ordering"] != "balanced-five-arm-rotation":
        raise ValueError(f"line {line}: unexpected ordering protocol")
    if result["byte_metrics"] != ("sink-input-exact,new-retained-projection-exact"):
        raise ValueError(f"line {line}: unexpected byte metric semantics")
    return result


Cell = tuple[str, int, int, int, int, int, str]


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
        if meta is None and line.lstrip().startswith(
            ("Compiling ", "Finished `bench` profile", "Running benches/")
        ):
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
        family = fields[1]
        width = parse_nonnegative(fields[2], "width", line_number)
        body_bytes = parse_nonnegative(fields[3], "body bytes", line_number)
        depth = parse_nonnegative(fields[4], "depth", line_number)
        multiplicity = parse_nonnegative(fields[5], "multiplicity", line_number)
        output_bytes = parse_nonnegative(fields[6], "output bytes", line_number)
        arm = fields[7]
        sink = fields[8]
        sample = parse_nonnegative(fields[9], "sample", line_number)
        position = parse_nonnegative(fields[10], "position", line_number)
        if arm not in ARMS:
            raise ValueError(f"line {line_number}: unknown arm {arm!r}")
        if sample >= expected_reps:
            raise ValueError(f"line {line_number}: sample is outside the locked range")
        if position >= len(ARMS):
            raise ValueError(f"line {line_number}: position is outside the arm batch")
        cell = (family, width, body_bytes, depth, multiplicity, output_bytes, sink)
        if arm in cells[cell][sample]:
            raise ValueError(
                f"line {line_number}: duplicate {arm} observation for {cell!r}, sample {sample}"
            )
        observation = {
            "position": position,
            "elapsed_ns": parse_nonnegative(fields[11], "elapsed ns", line_number),
            "allocations": parse_nonnegative(fields[12], "allocations", line_number),
            "allocated_bytes": parse_nonnegative(
                fields[13], "allocated bytes", line_number
            ),
            "pathmap_traversals": parse_nonnegative(
                fields[14], "PathMap traversals", line_number
            ),
            "sink_input_bytes": parse_nonnegative(
                fields[15], "sink input bytes", line_number
            ),
            "newly_retained_projection_bytes": parse_nonnegative(
                fields[16], "newly retained projection bytes", line_number
            ),
            "retained_projection_bytes": parse_nonnegative(
                fields[17], "retained projection bytes", line_number
            ),
        }
        if observation["elapsed_ns"] == 0:
            raise ValueError(f"line {line_number}: elapsed time must be positive")
        if observation["sink_input_bytes"] != output_bytes:
            raise ValueError(f"line {line_number}: sink byte count differs from output")
        if arm == "cold_cached":
            if (
                observation["newly_retained_projection_bytes"]
                != observation["retained_projection_bytes"]
            ):
                raise ValueError(
                    f"line {line_number}: cold preparation retained-byte counters disagree"
                )
        elif observation["newly_retained_projection_bytes"] != 0:
            raise ValueError(f"line {line_number}: non-cold arm retained new bytes")
        expected_traversals = 0 if arm in {"warm_cached", "warm_adaptive"} else 1
        if observation["pathmap_traversals"] != expected_traversals:
            raise ValueError(
                f"line {line_number}: traversal counter disagrees with arm"
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


def sample_summary(values: list[float]) -> dict[str, float | int]:
    return {
        "n": len(values),
        "mean": statistics.mean(values),
        "median": statistics.median(values),
        "minimum": min(values),
        "maximum": max(values),
        "standard_deviation": statistics.stdev(values),
    }


def paired_bootstrap_ratio(
    controls: list[float], treatments: list[float], replicates: int, seed: int
) -> dict[str, float]:
    if replicates < 10_000:
        raise ValueError("at least 10,000 bootstrap replicates are required")
    generator = np.random.default_rng(seed)
    indices = generator.integers(0, len(controls), size=(replicates, len(controls)))
    control = np.asarray(controls, dtype=np.float64)
    treatment = np.asarray(treatments, dtype=np.float64)
    ratios = np.median(treatment[indices], axis=1) / np.median(control[indices], axis=1)
    low, median, high = np.quantile(ratios, (0.005, 0.5, 0.995))
    return {
        "lower_0_5_percent": float(low),
        "median": float(median),
        "upper_99_5_percent": float(high),
    }


def paired_statistics(
    control: list[float], treatment: list[float], replicates: int, seed: int
) -> dict[str, Any]:
    differences = [b - a for a, b in zip(control, treatment)]
    if all(difference == 0 for difference in differences):
        paired_t_pvalue = 1.0
        signed_rank_pvalue = 1.0
        shapiro_pvalue = 1.0
    else:
        paired_t_pvalue = float(ttest_rel(treatment, control).pvalue)
        signed_rank_pvalue = float(
            wilcoxon(treatment, control, alternative="two-sided").pvalue
        )
        shapiro_pvalue = float(shapiro(differences).pvalue)
    return {
        "control": sample_summary(control),
        "treatment": sample_summary(treatment),
        "paired_difference_ns": sample_summary(differences),
        "mean_treatment_control_ratio": statistics.mean(treatment)
        / statistics.mean(control),
        "median_treatment_control_ratio": statistics.median(treatment)
        / statistics.median(control),
        "paired_bootstrap_median_ratio_99_percent": paired_bootstrap_ratio(
            control, treatment, replicates, seed
        ),
        "paired_t_two_sided_pvalue": paired_t_pvalue,
        "paired_wilcoxon_two_sided_pvalue": signed_rank_pvalue,
        "shapiro_pvalue_paired_difference": shapiro_pvalue,
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


def cell_name(cell: Cell) -> str:
    family, width, body, depth, multiplicity, output, sink = cell
    return (
        f"{family}/width={width}/body={body}/depth={depth}/"
        f"multiplicity={multiplicity}/output={output}/sink={sink}"
    )


def main() -> None:
    args = parse_args()
    if args.expected_reps < 3:
        raise ValueError("at least three paired observations are required")
    raw, meta, loadavg, cells = read_protocol(args.input, args.expected_reps)
    results: dict[str, Any] = {}
    tsv_rows: list[list[str]] = []
    for cell_index, cell in enumerate(sorted(cells)):
        samples = cells[cell]
        control = [
            float(samples[index][CONTROL]["elapsed_ns"])
            for index in range(args.expected_reps)
        ]
        comparisons: dict[str, Any] = {}
        resources: dict[str, Any] = {}
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
            comparison["position_median_ns"] = {
                str(position): statistics.median(
                    float(observation[arm]["elapsed_ns"])
                    for observation in samples.values()
                    if observation[arm]["position"] == position
                )
                for position in range(len(ARMS))
                if any(
                    observation[arm]["position"] == position
                    for observation in samples.values()
                )
            }
            comparisons[arm] = comparison
            interval = comparison["paired_bootstrap_median_ratio_99_percent"]
            tsv_rows.append(
                [
                    cell_name(cell),
                    arm,
                    str(comparison["median_treatment_control_ratio"]),
                    str(interval["lower_0_5_percent"]),
                    str(interval["upper_99_5_percent"]),
                    str(comparison["paired_wilcoxon_two_sided_pvalue"]),
                ]
            )
        for arm in ARMS:
            resources[arm] = {
                metric: sorted(
                    {samples[index][arm][metric] for index in range(args.expected_reps)}
                )
                for metric in (
                    "allocations",
                    "allocated_bytes",
                    "pathmap_traversals",
                    "sink_input_bytes",
                    "newly_retained_projection_bytes",
                    "retained_projection_bytes",
                )
            }
        results[cell_name(cell)] = {
            "comparisons_against_direct": comparisons,
            "resources": resources,
        }

    comparison_keys = {
        f"{cell_name_value}|{arm}": comparison
        for cell_name_value, cell_result in results.items()
        for arm, comparison in cell_result["comparisons_against_direct"].items()
    }
    paired_t_qvalues = benjamini_hochberg(
        {
            key: comparison["paired_t_two_sided_pvalue"]
            for key, comparison in comparison_keys.items()
        }
    )
    signed_rank_qvalues = benjamini_hochberg(
        {
            key: comparison["paired_wilcoxon_two_sided_pvalue"]
            for key, comparison in comparison_keys.items()
        }
    )
    for key, comparison in comparison_keys.items():
        comparison["paired_t_benjamini_hochberg_qvalue"] = paired_t_qvalues[key]
        comparison["paired_wilcoxon_benjamini_hochberg_qvalue"] = signed_rank_qvalues[
            key
        ]

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
        "cells": results,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.summary_tsv.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    with args.summary_tsv.open("w", encoding="utf-8", newline="") as output:
        output.write(
            "cell\ttreatment\tmedian_ratio\tbootstrap_99_low\t"
            "bootstrap_99_high\twilcoxon_pvalue\n"
        )
        for row in tsv_rows:
            output.write("\t".join(row) + "\n")


if __name__ == "__main__":
    main()

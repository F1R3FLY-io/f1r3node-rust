#!/usr/bin/env python3
"""Validate and analyze the final-value RSpace DATA egress experiment.

The benchmark emits one byte/hash CHECK row and 31 counterbalanced AB/BA
pairs per preregistered case. This analyzer rejects protocol drift and witness
disagreement before computing paired timing and exact allocation evidence.
Bootstrap resampling retains complete pairs.
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

SCHEMA = "rspace-canonical-egress-v2-final-persisted-value"
SEED = 0x4131_6026_2026_0825
RETAINED_PAIRS = 31
ARMS = ("legacy_typed", "canonical_arena")
EXPECTED_CASES = {
    ("shared", 1, 1),
    ("shared", 16, 1),
    ("shared", 256, 1),
    ("shared", 4_096, 1),
    ("unshared", 256, 1),
    ("shared", 16, 2),
    ("shared", 16, 16),
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--summary-tsv", type=Path, required=True)
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


def validate_digest(value: str, line: int) -> str:
    if len(value) != 64:
        raise ValueError(f"line {line}: digest must contain 64 hexadecimal digits")
    try:
        bytes.fromhex(value)
    except ValueError as error:
        raise ValueError(f"line {line}: digest is not hexadecimal") from error
    return value


def read_protocol(
    path: Path,
) -> tuple[
    bytes,
    dict[tuple[str, int, int], dict[str, Any]],
    dict[tuple[str, int, int, int], dict[str, dict[str, Any]]],
]:
    raw = path.read_bytes()
    checks: dict[tuple[str, int, int], dict[str, Any]] = {}
    pairs: dict[tuple[str, int, int, int], dict[str, dict[str, Any]]] = defaultdict(
        dict
    )

    for line_number, line in enumerate(raw.decode("utf-8").splitlines(), 1):
        fields = line.split("\t")
        if not fields or fields[0] not in {"CHECK", "OBS"}:
            raise ValueError(f"line {line_number}: unknown row kind")
        expected_width = 8 if fields[0] == "CHECK" else 13
        if len(fields) != expected_width:
            raise ValueError(
                f"line {line_number}: expected {expected_width} fields, got {len(fields)}"
            )
        if fields[1] != SCHEMA:
            raise ValueError(f"line {line_number}: unexpected schema {fields[1]!r}")
        shape = fields[2]
        distinct = parse_nonnegative(fields[3], "distinct", line_number)
        multiplicity = parse_nonnegative(fields[4], "multiplicity", line_number)
        logical = parse_nonnegative(fields[5], "logical cardinality", line_number)
        case = (shape, distinct, multiplicity)
        if case not in EXPECTED_CASES:
            raise ValueError(f"line {line_number}: unregistered case {case!r}")
        if logical != distinct * multiplicity:
            raise ValueError(f"line {line_number}: logical cardinality is inconsistent")

        if fields[0] == "CHECK":
            if case in checks:
                raise ValueError(
                    f"line {line_number}: duplicate CHECK row for {case!r}"
                )
            checks[case] = {
                "logical": logical,
                "output_len": parse_nonnegative(
                    fields[6], "output length", line_number
                ),
                "digest": validate_digest(fields[7], line_number),
            }
            continue

        pair = parse_nonnegative(fields[6], "pair", line_number)
        if pair >= RETAINED_PAIRS:
            raise ValueError(f"line {line_number}: pair is outside the locked range")
        order = fields[7]
        if order not in {"AB", "BA"}:
            raise ValueError(f"line {line_number}: invalid order {order!r}")
        arm = fields[8]
        if arm not in ARMS:
            raise ValueError(f"line {line_number}: unknown arm {arm!r}")
        key = (*case, pair)
        if arm in pairs[key]:
            raise ValueError(
                f"line {line_number}: duplicate {arm} observation for {key!r}"
            )
        elapsed_ns = parse_nonnegative(fields[9], "elapsed nanoseconds", line_number)
        if elapsed_ns == 0:
            raise ValueError(f"line {line_number}: elapsed time must be positive")
        pairs[key][arm] = {
            "order": order,
            "elapsed_ns": elapsed_ns,
            "allocations": parse_nonnegative(fields[10], "allocations", line_number),
            "allocation_bytes": parse_nonnegative(
                fields[11], "allocation bytes", line_number
            ),
            "output_len": parse_nonnegative(fields[12], "output length", line_number),
        }

    if set(checks) != EXPECTED_CASES:
        raise ValueError(
            f"CHECK cases differ: missing={EXPECTED_CASES - set(checks)!r}, "
            f"extra={set(checks) - EXPECTED_CASES!r}"
        )
    expected_pair_keys = {
        (*case, pair) for case in EXPECTED_CASES for pair in range(RETAINED_PAIRS)
    }
    if set(pairs) != expected_pair_keys:
        raise ValueError(
            f"OBS pairs differ: missing={len(expected_pair_keys - set(pairs))}, "
            f"extra={len(set(pairs) - expected_pair_keys)}"
        )
    for key, observations in pairs.items():
        if set(observations) != set(ARMS):
            raise ValueError(f"pair {key!r} does not contain both arms")
        if observations[ARMS[0]]["order"] != observations[ARMS[1]]["order"]:
            raise ValueError(f"pair {key!r} has inconsistent order labels")
        expected_output_len = checks[key[:3]]["output_len"]
        for arm, observation in observations.items():
            if observation["output_len"] != expected_output_len:
                raise ValueError(
                    f"pair {key!r} arm {arm} output length "
                    f"{observation['output_len']} != {expected_output_len}"
                )
    return raw, checks, pairs


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
    controls: list[float], treatments: list[float], replicates: int, seed_offset: int
) -> dict[str, float]:
    if replicates < 10_000:
        raise ValueError("at least 10,000 bootstrap replicates are required")
    generator = np.random.default_rng(SEED + seed_offset)
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


def analyze_case(
    case: tuple[str, int, int],
    pairs: dict[tuple[str, int, int, int], dict[str, dict[str, Any]]],
    bootstrap_replicates: int,
    seed_offset: int,
) -> dict[str, Any]:
    observations = [pairs[(*case, pair)] for pair in range(RETAINED_PAIRS)]
    controls = [float(item[ARMS[0]]["elapsed_ns"]) for item in observations]
    treatments = [float(item[ARMS[1]]["elapsed_ns"]) for item in observations]
    differences = [
        treatment - control for control, treatment in zip(controls, treatments)
    ]
    paired_t = ttest_rel(treatments, controls, alternative="less")
    signed_rank = wilcoxon(treatments, controls, alternative="less")
    result: dict[str, Any] = {
        "control_elapsed_ns": sample_summary(controls),
        "treatment_elapsed_ns": sample_summary(treatments),
        "paired_difference_ns": sample_summary(differences),
        "mean_treatment_control_ratio": statistics.mean(treatments)
        / statistics.mean(controls),
        "median_treatment_control_ratio": statistics.median(treatments)
        / statistics.median(controls),
        "paired_t_treatment_less": {
            "statistic": float(paired_t.statistic),
            "pvalue": float(paired_t.pvalue),
        },
        "paired_wilcoxon_treatment_less": {
            "statistic": float(signed_rank.statistic),
            "pvalue": float(signed_rank.pvalue),
        },
        "shapiro_pvalue_paired_difference": float(shapiro(differences).pvalue),
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
    for metric in ("allocations", "allocation_bytes"):
        control_values = [int(item[ARMS[0]][metric]) for item in observations]
        treatment_values = [int(item[ARMS[1]][metric]) for item in observations]
        result["resources"][metric] = {
            "control_unique": sorted(set(control_values)),
            "treatment_unique": sorted(set(treatment_values)),
            "treatment_minus_control": sorted(
                {
                    treatment - control
                    for control, treatment in zip(control_values, treatment_values)
                }
            ),
        }
    return result


def main() -> None:
    args = parse_args()
    raw, checks, pairs = read_protocol(args.input)
    cells: dict[str, Any] = {}
    for seed_offset, case in enumerate(sorted(EXPECTED_CASES)):
        key = f"{case[0]}/distinct={case[1]}/multiplicity={case[2]}"
        cells[key] = analyze_case(case, pairs, args.bootstrap_replicates, seed_offset)

    paired_t_qvalues = benjamini_hochberg(
        {
            key: value["paired_t_treatment_less"]["pvalue"]
            for key, value in cells.items()
        }
    )
    signed_rank_qvalues = benjamini_hochberg(
        {
            key: value["paired_wilcoxon_treatment_less"]["pvalue"]
            for key, value in cells.items()
        }
    )
    for key, value in cells.items():
        value["paired_t_treatment_less"]["benjamini_hochberg_qvalue"] = (
            paired_t_qvalues[key]
        )
        value["paired_wilcoxon_treatment_less"]["benjamini_hochberg_qvalue"] = (
            signed_rank_qvalues[key]
        )

    result = {
        "schema": SCHEMA,
        "input": str(args.input),
        "input_sha256": hashlib.sha256(raw).hexdigest(),
        "retained_pairs_per_cell": RETAINED_PAIRS,
        "bootstrap_replicates": args.bootstrap_replicates,
        "bootstrap_seed": hex(SEED),
        "python": platform.python_version(),
        "numpy": np.__version__,
        "scipy": scipy_version,
        "checks": {
            f"{case[0]}/distinct={case[1]}/multiplicity={case[2]}": witness
            for case, witness in sorted(checks.items())
        },
        "cells": cells,
        "all_cells_treatment_median_faster": all(
            cell["median_treatment_control_ratio"] < 1.0 for cell in cells.values()
        ),
        "all_cells_bootstrap_upper_below_one": all(
            cell["paired_bootstrap_median_ratio_99_percent"]["upper_99_5_percent"] < 1.0
            for cell in cells.values()
        ),
        "all_cells_exactly_fewer_allocation_calls_and_bytes": all(
            cell["resources"][metric]["treatment_minus_control"][-1] < 0
            for cell in cells.values()
            for metric in ("allocations", "allocation_bytes")
        ),
    }
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")

    rows = [
        "case\tcontrol_median_ns\ttreatment_median_ns\tmedian_ratio\tbootstrap_0.5%\tbootstrap_99.5%\talloc_call_delta\talloc_byte_delta"
    ]
    for key, cell in cells.items():
        bootstrap = cell["paired_bootstrap_median_ratio_99_percent"]
        rows.append(
            "\t".join(
                [
                    key,
                    str(cell["control_elapsed_ns"]["median"]),
                    str(cell["treatment_elapsed_ns"]["median"]),
                    str(cell["median_treatment_control_ratio"]),
                    str(bootstrap["lower_0_5_percent"]),
                    str(bootstrap["upper_99_5_percent"]),
                    str(cell["resources"]["allocations"]["treatment_minus_control"][0]),
                    str(
                        cell["resources"]["allocation_bytes"][
                            "treatment_minus_control"
                        ][0]
                    ),
                ]
            )
        )
    args.summary_tsv.write_text("\n".join(rows) + "\n")


if __name__ == "__main__":
    main()

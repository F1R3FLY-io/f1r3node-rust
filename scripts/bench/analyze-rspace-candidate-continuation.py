"""Validate and analyze the candidate-product continuation experiment.

The paired release matrix compares the frozen retained-read-zipper control
with the production moving-cursor/flat-arena PDA.  This analyzer rejects
missing cells, broken AB/BA pairs, digest drift, and inconsistent batch sizes
before computing paired statistics.  It also fits median time against encoded
depth on a log-log scale so removal of the legacy depth-growing prefix-copy
mechanism is measured independently from constant-factor latency.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import platform
import statistics
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

from paired_benchmark_statistics import benjamini_hochberg, paired_statistics

SCHEMA = "candidate-continuation-arena-v3"
CONTROL = "legacy_clone"
TREATMENT = "moving_arena"
ARMS = (CONTROL, TREATMENT)
SHAPES = ("compressed", "dense64")
QUERIES = ("exact", "wildcard", "repeated_capture", "nested_match", "accepts_remainder")
DEPTHS = (256, 512, 1024, 2048, 4096)
PAIRS = 30
SEED = 0x4131_5990_2026_0831


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--summary-tsv", type=Path, required=True)
    parser.add_argument("--bootstrap-replicates", type=int, default=100_000)
    return parser.parse_args()


def nonnegative(value: str, label: str, line_number: int) -> int:
    try:
        parsed = int(value)
    except ValueError as error:
        raise ValueError(f"line {line_number}: {label} is not an integer") from error
    if parsed < 0:
        raise ValueError(f"line {line_number}: {label} must be nonnegative")
    return parsed


def read_matrix(path: Path) -> tuple[bytes, dict[str, Any]]:
    raw = path.read_bytes()
    checks: dict[tuple[str, str, int], dict[str, Any]] = {}
    observations: dict[tuple[str, str, int, int], dict[str, dict[str, Any]]] = (
        defaultdict(dict)
    )
    meta_seen = False

    for line_number, line in enumerate(raw.decode("utf-8").splitlines(), 1):
        fields = line.split("\t")
        if not fields:
            continue
        if fields[0] == "META":
            if meta_seen or len(fields) != 6 or fields[1] != SCHEMA:
                raise ValueError(f"line {line_number}: invalid or duplicate META row")
            expected = {
                "seed=4697634361873664035",
                "warmups=3",
                "retained_pairs=30",
                "rows=3000",
            }
            if set(fields[2:]) != expected:
                raise ValueError(
                    f"line {line_number}: META protocol drift: {fields[2:]!r}"
                )
            meta_seen = True
            continue
        if fields[0] in {"CHECK_HEADER", "OBS_HEADER"}:
            continue
        if len(fields) < 2 or fields[1] != SCHEMA:
            continue
        if fields[0] == "CHECK":
            if len(fields) != 12:
                raise ValueError(
                    f"line {line_number}: CHECK width is {len(fields)}, expected 12"
                )
            shape, query = fields[2], fields[3]
            depth = nonnegative(fields[4], "depth", line_number)
            key = (shape, query, depth)
            if key in checks:
                raise ValueError(f"line {line_number}: duplicate CHECK {key!r}")
            checks[key] = {
                "logical_structure_bytes": nonnegative(
                    fields[5], "logical bytes", line_number
                ),
                "result_count": nonnegative(fields[6], "result count", line_number),
                "result_digest": fields[7],
                "legacy_visited_frames": nonnegative(
                    fields[8], "visited frames", line_number
                ),
                "legacy_zipper_pairs_cloned": nonnegative(
                    fields[9], "zipper clones", line_number
                ),
                "legacy_total_prefix_bytes_cloned": nonnegative(
                    fields[10], "prefix bytes", line_number
                ),
                "legacy_maximum_prefix_bytes_cloned": nonnegative(
                    fields[11], "maximum prefix bytes", line_number
                ),
            }
            continue
        if fields[0] == "OBS":
            if len(fields) != 12:
                raise ValueError(
                    f"line {line_number}: OBS width is {len(fields)}, expected 12"
                )
            shape, query = fields[2], fields[3]
            depth = nonnegative(fields[4], "depth", line_number)
            pair = nonnegative(fields[5], "pair", line_number)
            order, arm = fields[6], fields[7]
            if pair >= PAIRS or order not in {"AB", "BA"} or arm not in ARMS:
                raise ValueError(
                    f"line {line_number}: observation coordinate is outside protocol"
                )
            key = (shape, query, depth, pair)
            if arm in observations[key]:
                raise ValueError(f"line {line_number}: duplicate {arm} row for {key!r}")
            observations[key][arm] = {
                "order": order,
                "batch_iterations": nonnegative(
                    fields[8], "batch iterations", line_number
                ),
                "elapsed_ns": nonnegative(fields[9], "elapsed ns", line_number),
                "result_count": nonnegative(fields[10], "result count", line_number),
                "result_digest": fields[11],
            }
            continue
        raise ValueError(f"line {line_number}: unknown schema row {fields[0]!r}")

    expected_cells = {
        (shape, query, depth)
        for shape in SHAPES
        for query in QUERIES
        for depth in DEPTHS
    }
    if not meta_seen or set(checks) != expected_cells:
        raise ValueError("CHECK matrix is incomplete or contains unexpected cells")
    expected_pairs = {(*cell, pair) for cell in expected_cells for pair in range(PAIRS)}
    if set(observations) != expected_pairs:
        raise ValueError("OBS matrix is incomplete or contains unexpected pairs")

    for key, arms in observations.items():
        if set(arms) != set(ARMS):
            raise ValueError(f"pair {key!r} does not contain exactly both arms")
        control, treatment = arms[CONTROL], arms[TREATMENT]
        if control["order"] != treatment["order"]:
            raise ValueError(f"pair {key!r} has inconsistent AB/BA labels")
        if control["batch_iterations"] != treatment["batch_iterations"]:
            raise ValueError(f"pair {key!r} has inconsistent batch sizes")
        check = checks[key[:3]]
        for arm in arms.values():
            if (arm["result_count"], arm["result_digest"]) != (
                check["result_count"],
                check["result_digest"],
            ):
                raise ValueError(f"pair {key!r} diverges from its semantic CHECK")
    return raw, {"checks": checks, "observations": observations}


def log_log_slope(depths: list[int], medians: list[float]) -> float:
    xs = [math.log(float(depth)) for depth in depths]
    ys = [math.log(value) for value in medians]
    x_mean, y_mean = statistics.mean(xs), statistics.mean(ys)
    numerator = sum((x - x_mean) * (y - y_mean) for x, y in zip(xs, ys))
    denominator = sum((x - x_mean) ** 2 for x in xs)
    return numerator / denominator


def analyze(raw: bytes, matrix: dict[str, Any], replicates: int) -> dict[str, Any]:
    cells: dict[str, dict[str, Any]] = {}
    pvalues: dict[str, float] = {}
    medians: dict[tuple[str, str, str], list[float]] = defaultdict(list)
    for shape in SHAPES:
        for query in QUERIES:
            for depth in DEPTHS:
                key = (shape, query, depth)
                control: list[float] = []
                treatment: list[float] = []
                for pair in range(PAIRS):
                    arms = matrix["observations"][(*key, pair)]
                    batch = arms[CONTROL]["batch_iterations"]
                    control.append(arms[CONTROL]["elapsed_ns"] / batch)
                    treatment.append(arms[TREATMENT]["elapsed_ns"] / batch)
                label = f"{shape}/{query}/{depth}"
                stats = paired_statistics(
                    control, treatment, replicates, SEED ^ len(cells)
                )
                stats["check"] = matrix["checks"][key]
                cells[label] = stats
                pvalues[label] = stats["paired_wilcoxon_two_sided_pvalue"]
                medians[(shape, query, CONTROL)].append(statistics.median(control))
                medians[(shape, query, TREATMENT)].append(statistics.median(treatment))

    adjusted = benjamini_hochberg(pvalues)
    for label, qvalue in adjusted.items():
        cells[label]["paired_wilcoxon_bh_qvalue"] = qvalue

    slopes = {
        f"{shape}/{query}/{arm}": log_log_slope(
            list(DEPTHS), medians[(shape, query, arm)]
        )
        for shape in SHAPES
        for query in QUERIES
        for arm in ARMS
    }
    ratios = [stats["median_treatment_control_ratio"] for stats in cells.values()]
    return {
        "schema": SCHEMA,
        "input_sha256": hashlib.sha256(raw).hexdigest(),
        "bootstrap": {"replicates": replicates, "seed": hex(SEED)},
        "environment": {
            "python": platform.python_version(),
            "platform": platform.platform(),
        },
        "matrix": {
            "shapes": SHAPES,
            "queries": QUERIES,
            "depths": DEPTHS,
            "pairs": PAIRS,
        },
        "cells": cells,
        "log_log_median_time_slopes": slopes,
        "decision": {
            "semantic_matrix_complete": True,
            "minimum_median_speedup": 1.0 / max(ratios),
            "maximum_median_speedup": 1.0 / min(ratios),
            "all_cell_median_ratios_below_one": all(ratio < 1.0 for ratio in ratios),
            "all_cell_bootstrap_upper_bounds_below_one": all(
                stats["paired_bootstrap_median_of_ratios_99_percent"][
                    "upper_99_5_percent"
                ]
                < 1.0
                for stats in cells.values()
            ),
        },
    }


def write_summary(path: Path, result: dict[str, Any]) -> None:
    lines = [
        "cell\tcontrol_median_ns\ttreatment_median_ns\tmedian_ratio\tbootstrap_99_low\tbootstrap_99_high\twilcoxon_bh_qvalue"
    ]
    for label, stats in sorted(result["cells"].items()):
        interval = stats["paired_bootstrap_median_of_ratios_99_percent"]
        lines.append(
            "\t".join(
                [
                    label,
                    str(stats["control"]["median"]),
                    str(stats["treatment"]["median"]),
                    str(stats["median_treatment_control_ratio"]),
                    str(interval["lower_0_5_percent"]),
                    str(interval["upper_99_5_percent"]),
                    str(stats["paired_wilcoxon_bh_qvalue"]),
                ]
            )
        )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    args = parse_args()
    raw, matrix = read_matrix(args.input)
    result = analyze(raw, matrix, args.bootstrap_replicates)
    args.output.write_text(
        json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    write_summary(args.summary_tsv, result)
    print(json.dumps(result["decision"], indent=2, sort_keys=True))


if __name__ == "__main__":
    main()

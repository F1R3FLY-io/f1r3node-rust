"""Validate and analyze the continuation/join record-order experiment.

The benchmark compares the production sorted-preparation representation with
two retained order-trie candidates.  This analyzer rejects incomplete or
mispaired matrices before computing paired 99% bootstrap intervals.  It keeps
timing, allocation, and retained-state evidence distinct: an order trie is not
eligible merely because repeated projections become faster if its mutation or
resident-memory costs violate the preregistered five-percent guardrail.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import statistics
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

from paired_benchmark_statistics import (
    benjamini_hochberg,
    paired_statistics,
)

SCHEMA = "rspace-record-order-index-v2"
CONTROL = "sorted_prepared"
CANDIDATES = ("shared_record_order_trie", "split_payload_order_trie")
ARMS = (CONTROL, *CANDIDATES)
RETAINED_SETS = 31
SEED = 0x4131_6029_2026_0826


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--summary-tsv", type=Path, required=True)
    parser.add_argument("--bootstrap-replicates", type=int, default=100_000)
    return parser.parse_args()


def parse_nonnegative(value: str, label: str, line_number: int) -> int:
    try:
        parsed = int(value)
    except ValueError as error:
        raise ValueError(f"line {line_number}: {label} is not an integer") from error
    if parsed < 0:
        raise ValueError(f"line {line_number}: {label} must be nonnegative")
    return parsed


def insert_unique(
    rows: dict[tuple[Any, ...], dict[str, Any]],
    key: tuple[Any, ...],
    arm: str,
    value: Any,
    line_number: int,
) -> None:
    if arm not in ARMS:
        raise ValueError(f"line {line_number}: unknown arm {arm!r}")
    if arm in rows[key]:
        raise ValueError(f"line {line_number}: duplicate {arm} row for {key!r}")
    rows[key][arm] = value


def read_matrix(path: Path) -> tuple[bytes, dict[str, Any]]:
    raw = path.read_bytes()
    checks: dict[tuple[str, str], dict[str, int]] = {}
    times: dict[tuple[str, str, int], dict[str, int]] = defaultdict(dict)
    allocations: dict[tuple[str, str, int], dict[str, dict[str, int]]] = defaultdict(
        dict
    )
    projection_times: dict[tuple[str, int, int], dict[str, int]] = defaultdict(dict)
    projection_allocations: dict[tuple[str, int, int], dict[str, dict[str, int]]] = (
        defaultdict(dict)
    )

    for line_number, line in enumerate(raw.decode("utf-8").splitlines(), 1):
        fields = line.split("\t")
        if len(fields) < 2 or fields[1] != SCHEMA:
            continue
        kind = fields[0]
        if kind in {"SCHEMA", "DECISION_CELL", "PROJECTION_DECISION"}:
            continue
        expected_width = {
            "CHECK": 9,
            "TIME": 11,
            "ALLOC": 10,
            "PROJECTION_TIME": 7,
            "PROJECTION_ALLOC": 10,
        }.get(kind)
        if expected_width is None:
            raise ValueError(f"line {line_number}: unknown benchmark row {kind!r}")
        if len(fields) != expected_width:
            raise ValueError(
                f"line {line_number}: {kind} has {len(fields)} fields, "
                f"expected {expected_width}"
            )

        case = fields[2]
        if kind == "CHECK":
            arm = fields[3]
            key = (case, arm)
            if key in checks:
                raise ValueError(f"line {line_number}: duplicate CHECK {key!r}")
            checks[key] = {
                "logical": parse_nonnegative(fields[4], "logical count", line_number),
                "primary": parse_nonnegative(fields[5], "primary count", line_number),
                "identity": parse_nonnegative(fields[6], "identity count", line_number),
                "order": parse_nonnegative(fields[7], "order count", line_number),
                "retained_bytes_floor": parse_nonnegative(
                    fields[8], "retained bytes", line_number
                ),
            }
            continue

        if kind in {"TIME", "ALLOC"}:
            operation = fields[3]
            set_number = parse_nonnegative(fields[4], "set", line_number)
            if set_number >= RETAINED_SETS:
                raise ValueError(f"line {line_number}: set is outside the locked range")
            arm = fields[5]
            key = (case, operation, set_number)
            if kind == "TIME":
                elapsed = parse_nonnegative(fields[6], "elapsed_ns", line_number)
                if elapsed == 0:
                    raise ValueError(f"line {line_number}: elapsed_ns must be positive")
                insert_unique(times, key, arm, elapsed, line_number)
            else:
                insert_unique(
                    allocations,
                    key,
                    arm,
                    {
                        "calls": parse_nonnegative(
                            fields[6], "alloc calls", line_number
                        ),
                        "bytes": parse_nonnegative(
                            fields[7], "alloc bytes", line_number
                        ),
                        "dealloc_calls": parse_nonnegative(
                            fields[8], "dealloc calls", line_number
                        ),
                        "dealloc_bytes": parse_nonnegative(
                            fields[9], "dealloc bytes", line_number
                        ),
                    },
                    line_number,
                )
            continue

        reuse = parse_nonnegative(fields[3], "projection reuse", line_number)
        set_number = parse_nonnegative(fields[4], "set", line_number)
        if set_number >= RETAINED_SETS:
            raise ValueError(f"line {line_number}: set is outside the locked range")
        arm = fields[5]
        key = (case, reuse, set_number)
        if kind == "PROJECTION_TIME":
            elapsed = parse_nonnegative(fields[6], "elapsed_ns", line_number)
            if elapsed == 0:
                raise ValueError(f"line {line_number}: elapsed_ns must be positive")
            insert_unique(projection_times, key, arm, elapsed, line_number)
        else:
            insert_unique(
                projection_allocations,
                key,
                arm,
                {
                    "calls": parse_nonnegative(fields[6], "alloc calls", line_number),
                    "bytes": parse_nonnegative(fields[7], "alloc bytes", line_number),
                    "dealloc_calls": parse_nonnegative(
                        fields[8], "dealloc calls", line_number
                    ),
                    "dealloc_bytes": parse_nonnegative(
                        fields[9], "dealloc bytes", line_number
                    ),
                },
                line_number,
            )

    cases = sorted({case for case, _ in checks})
    operations = sorted({operation for _, operation, _ in times})
    reuses = sorted({reuse for _, reuse, _ in projection_times})
    if not cases or not operations or not reuses:
        raise ValueError("benchmark matrix is empty")
    for case in cases:
        witnesses = [checks.get((case, arm)) for arm in ARMS]
        if any(witness is None for witness in witnesses):
            raise ValueError(f"case {case!r} is missing an arm CHECK")
        logical = {witness["logical"] for witness in witnesses if witness is not None}
        if len(logical) != 1:
            raise ValueError(f"case {case!r} has unequal logical witnesses")
    for mapping, label in [
        (times, "TIME"),
        (allocations, "ALLOC"),
        (projection_times, "PROJECTION_TIME"),
        (projection_allocations, "PROJECTION_ALLOC"),
    ]:
        for key, row in mapping.items():
            if set(row) != set(ARMS):
                raise ValueError(f"{label} {key!r} is missing a balanced arm")

    expected_lifecycle = len(cases) * len(operations) * RETAINED_SETS
    expected_projection = len(cases) * len(reuses) * RETAINED_SETS
    if len(times) != expected_lifecycle or len(allocations) != expected_lifecycle:
        raise ValueError("lifecycle timing/allocation matrix is incomplete")
    if (
        len(projection_times) != expected_projection
        or len(projection_allocations) != expected_projection
    ):
        raise ValueError("projection timing/allocation matrix is incomplete")

    return raw, {
        "checks": checks,
        "times": times,
        "allocations": allocations,
        "projection_times": projection_times,
        "projection_allocations": projection_allocations,
        "cases": cases,
        "operations": operations,
        "reuses": reuses,
    }


def ratio(candidate: float, control: float) -> float:
    if control == 0:
        return 1.0 if candidate == 0 else float("inf")
    return candidate / control


def allocation_summary(
    rows: list[dict[str, dict[str, int]]], candidate: str
) -> dict[str, float]:
    result: dict[str, float] = {}
    for metric in ("calls", "bytes", "dealloc_calls", "dealloc_bytes"):
        controls = [row[CONTROL][metric] for row in rows]
        treatments = [row[candidate][metric] for row in rows]
        control_median = float(statistics.median(controls))
        treatment_median = float(statistics.median(treatments))
        result[f"control_median_{metric}"] = control_median
        result[f"candidate_median_{metric}"] = treatment_median
        result[f"candidate_control_median_{metric}_ratio"] = ratio(
            treatment_median, control_median
        )
    return result


def analyze(raw: bytes, matrix: dict[str, Any], replicates: int) -> dict[str, Any]:
    checks = matrix["checks"]
    lifecycle: dict[str, Any] = {}
    projection: dict[str, Any] = {}
    pvalues: dict[str, float] = {}
    seed_offset = 0

    for case in matrix["cases"]:
        for operation in matrix["operations"]:
            rows = [
                matrix["times"][(case, operation, set_number)]
                for set_number in range(RETAINED_SETS)
            ]
            allocation_rows = [
                matrix["allocations"][(case, operation, set_number)]
                for set_number in range(RETAINED_SETS)
            ]
            for candidate in CANDIDATES:
                control = [float(row[CONTROL]) for row in rows]
                treatment = [float(row[candidate]) for row in rows]
                stats = paired_statistics(
                    control, treatment, replicates, SEED + seed_offset
                )
                seed_offset += 1
                stats["allocations"] = allocation_summary(allocation_rows, candidate)
                retained_control = checks[(case, CONTROL)]["retained_bytes_floor"]
                retained_candidate = checks[(case, candidate)]["retained_bytes_floor"]
                stats["retained"] = {
                    "control_bytes_floor": retained_control,
                    "candidate_bytes_floor": retained_candidate,
                    "candidate_control_ratio": ratio(
                        retained_candidate, retained_control
                    ),
                }
                key = f"{case}/{operation}/{candidate}"
                lifecycle[key] = stats
                pvalues[key] = stats["paired_wilcoxon_two_sided_pvalue"]

        for reuse in matrix["reuses"]:
            rows = [
                matrix["projection_times"][(case, reuse, set_number)]
                for set_number in range(RETAINED_SETS)
            ]
            allocation_rows = [
                matrix["projection_allocations"][(case, reuse, set_number)]
                for set_number in range(RETAINED_SETS)
            ]
            for candidate in CANDIDATES:
                stats = paired_statistics(
                    [float(row[CONTROL]) for row in rows],
                    [float(row[candidate]) for row in rows],
                    replicates,
                    SEED + seed_offset,
                )
                seed_offset += 1
                stats["allocations"] = allocation_summary(allocation_rows, candidate)
                key = f"{case}/reuse={reuse}/{candidate}"
                projection[key] = stats
                pvalues[f"projection/{key}"] = stats["paired_wilcoxon_two_sided_pvalue"]

    qvalues = benjamini_hochberg(pvalues)
    for key, stats in lifecycle.items():
        stats["paired_wilcoxon_bh_qvalue"] = qvalues[key]
    for key, stats in projection.items():
        stats["paired_wilcoxon_bh_qvalue"] = qvalues[f"projection/{key}"]

    decisions: dict[str, Any] = {}
    for candidate in CANDIDATES:
        candidate_lifecycle = {
            key: value for key, value in lifecycle.items() if key.endswith(candidate)
        }
        candidate_projection = {
            key: value for key, value in projection.items() if key.endswith(candidate)
        }
        retained_failures = [
            key
            for key, value in candidate_lifecycle.items()
            if value["retained"]["candidate_control_ratio"] > 1.05
        ]
        lifecycle_time_failures = [
            key
            for key, value in candidate_lifecycle.items()
            if value["paired_bootstrap_median_of_ratios_99_percent"][
                "upper_99_5_percent"
            ]
            > 1.05
        ]
        projection_time_failures = [
            key
            for key, value in candidate_projection.items()
            if value["paired_bootstrap_median_of_ratios_99_percent"][
                "upper_99_5_percent"
            ]
            > 1.05
        ]
        decisions[candidate] = {
            "selected": not (
                retained_failures or lifecycle_time_failures or projection_time_failures
            ),
            "retained_guardrail_failures": retained_failures,
            "lifecycle_time_guardrail_failures": lifecycle_time_failures,
            "projection_time_guardrail_failures": projection_time_failures,
        }

    return {
        "schema": SCHEMA,
        "input_sha256": hashlib.sha256(raw).hexdigest(),
        "bootstrap": {"replicates": replicates, "seed": hex(SEED)},
        "environment": {
            "python": platform.python_version(),
            "platform": platform.platform(),
        },
        "matrix": {
            "cases": matrix["cases"],
            "operations": matrix["operations"],
            "projection_reuses": matrix["reuses"],
            "sets_per_arm": RETAINED_SETS,
        },
        "lifecycle": lifecycle,
        "projection": projection,
        "candidate_decisions": decisions,
        "production_decision": (
            "retain sorted_prepared; both retained order-trie candidates violate "
            "the frozen lifecycle/resource guardrails"
        ),
    }


def write_summary(path: Path, result: dict[str, Any]) -> None:
    lines = [
        (
            "domain\tcell\tcandidate\tmedian_ratio\tbootstrap_99_low\t"
            "bootstrap_99_high\tretained_ratio\twilcoxon_bh_qvalue"
        )
    ]
    for domain in ("lifecycle", "projection"):
        for key, stats in sorted(result[domain].items()):
            candidate = key.rsplit("/", 1)[-1]
            interval = stats["paired_bootstrap_median_of_ratios_99_percent"]
            retained = stats.get("retained", {}).get("candidate_control_ratio", "-")
            lines.append(
                "\t".join(
                    [
                        domain,
                        key,
                        candidate,
                        str(stats["median_treatment_control_ratio"]),
                        str(interval["lower_0_5_percent"]),
                        str(interval["upper_99_5_percent"]),
                        str(retained),
                        str(stats["paired_wilcoxon_bh_qvalue"]),
                    ]
                )
            )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    args = parse_args()
    raw, matrix = read_matrix(args.input)
    result = analyze(raw, matrix, args.bootstrap_replicates)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    write_summary(args.summary_tsv, result)
    print(json.dumps(result["candidate_decisions"], indent=2, sort_keys=True))
    print(result["production_decision"])


if __name__ == "__main__":
    main()

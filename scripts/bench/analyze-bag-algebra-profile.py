#!/usr/bin/env python3

import argparse
import csv
import random
import statistics
from collections import defaultdict
from pathlib import Path

CASES = {
    "empty",
    "one_equal",
    "one_disjoint",
    "one_maximum",
    "shared_64_equal",
    "shared_64_half",
    "prefix_chain_64_half",
    "shared_1024_equal",
    "shared_1024_half",
    "shared_1024_disjoint",
    "left_one_right_1024",
    "left_1024_right_one",
    "dispersed_1024_equal",
}
OPERATIONS = {"meet", "subtract", "subbag"}
ARMS = {"control", "treatment"}
SAMPLES = 31
BOOTSTRAPS = 10_000
SEED = 4_131_795_7


def read_rows(path):
    grouped = defaultdict(dict)
    with path.open(encoding="utf-8") as source:
        for line in source:
            marker = line.find("BAGALG\t")
            if marker < 0:
                continue
            fields = line[marker:].rstrip("\n").split("\t")
            if len(fields) != 9:
                raise ValueError(f"invalid BAGALG row: {line.rstrip()}")
            _, case, operation, arm, sample, repetitions, calls, bytes_, nanos = fields
            if case not in CASES or operation not in OPERATIONS or arm not in ARMS:
                raise ValueError(
                    f"unexpected profile dimension: {case}/{operation}/{arm}"
                )
            sample = int(sample)
            repetitions = int(repetitions)
            calls = int(calls)
            bytes_ = int(bytes_)
            nanos = int(nanos)
            if (
                sample not in range(SAMPLES)
                or min(repetitions, calls, bytes_, nanos) < 0
            ):
                raise ValueError(f"invalid numeric profile field: {line.rstrip()}")
            key = case, operation, arm
            if sample in grouped[key]:
                raise ValueError(f"duplicate profile row: {key}/{sample}")
            grouped[key][sample] = repetitions, calls, bytes_, nanos
    expected_keys = {
        (case, operation, arm)
        for case in CASES
        for operation in OPERATIONS
        for arm in ARMS
    }
    if set(grouped) != expected_keys:
        raise ValueError(
            f"profile cells differ: missing={expected_keys - set(grouped)} extra={set(grouped) - expected_keys}"
        )
    for key, samples in grouped.items():
        if set(samples) != set(range(SAMPLES)):
            raise ValueError(f"profile samples incomplete: {key}")
        if len({row[0] for row in samples.values()}) != 1:
            raise ValueError(f"profile repetitions differ: {key}")
    return grouped


def upper_median_ratio(control, treatment, seed):
    rng = random.Random(seed)
    n = len(control)
    ratios = []
    for _ in range(BOOTSTRAPS):
        indices = [rng.randrange(n) for _ in range(n)]
        control_median = statistics.median(control[index] for index in indices)
        treatment_median = statistics.median(treatment[index] for index in indices)
        ratios.append(treatment_median / control_median)
    ratios.sort()
    return ratios[int(0.99 * len(ratios))]


def analyze(grouped):
    rows = []
    for case in sorted(CASES):
        for operation in sorted(OPERATIONS):
            control = [
                grouped[case, operation, "control"][index] for index in range(SAMPLES)
            ]
            treatment = [
                grouped[case, operation, "treatment"][index] for index in range(SAMPLES)
            ]
            repetitions = control[0][0]
            if any(row[0] != repetitions for row in treatment):
                raise ValueError(f"arm repetitions differ: {case}/{operation}")
            control_wall = [row[3] / repetitions for row in control]
            treatment_wall = [row[3] / repetitions for row in treatment]
            control_median = statistics.median(control_wall)
            treatment_median = statistics.median(treatment_wall)
            ratio = treatment_median / control_median
            upper = upper_median_ratio(
                control_wall, treatment_wall, SEED ^ sum(map(ord, case + operation))
            )
            control_calls = max(row[1] / repetitions for row in control)
            treatment_calls = max(row[1] / repetitions for row in treatment)
            control_bytes = max(row[2] / repetitions for row in control)
            treatment_bytes = max(row[2] / repetitions for row in treatment)
            rows.append(
                {
                    "case": case,
                    "operation": operation,
                    "samples_per_arm": SAMPLES,
                    "repetitions": repetitions,
                    "control_median_ns_per_op": f"{control_median:.6f}",
                    "treatment_median_ns_per_op": f"{treatment_median:.6f}",
                    "median_ratio": f"{ratio:.9f}",
                    "median_ratio_upper99": f"{upper:.9f}",
                    "control_max_alloc_calls_per_op": f"{control_calls:.6f}",
                    "treatment_max_alloc_calls_per_op": f"{treatment_calls:.6f}",
                    "control_max_alloc_bytes_per_op": f"{control_bytes:.6f}",
                    "treatment_max_alloc_bytes_per_op": f"{treatment_bytes:.6f}",
                    "time_gate": "pass" if upper <= 1.05 else "fail",
                    "allocation_call_gate": "pass"
                    if treatment_calls <= control_calls
                    else "fail",
                }
            )
    return rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("raw", type=Path)
    parser.add_argument("fits", type=Path)
    args = parser.parse_args()
    rows = analyze(read_rows(args.raw))
    with args.fits.open("w", encoding="utf-8", newline="") as destination:
        writer = csv.DictWriter(
            destination, fieldnames=list(rows[0]), delimiter="\t", lineterminator="\n"
        )
        writer.writeheader()
        writer.writerows(rows)
    max_upper = max(float(row["median_ratio_upper99"]) for row in rows)
    failed_time = [
        f"{row['case']}/{row['operation']}"
        for row in rows
        if row["time_gate"] == "fail"
    ]
    failed_alloc = [
        f"{row['case']}/{row['operation']}"
        for row in rows
        if row["allocation_call_gate"] == "fail"
    ]
    print(
        f"cells={len(rows)} samples_per_arm={SAMPLES} bootstrap_draws={BOOTSTRAPS} max_upper99={max_upper:.9f}"
    )
    print(f"time_failures={','.join(failed_time) or 'none'}")
    print(f"allocation_call_failures={','.join(failed_alloc) or 'none'}")


if __name__ == "__main__":
    main()

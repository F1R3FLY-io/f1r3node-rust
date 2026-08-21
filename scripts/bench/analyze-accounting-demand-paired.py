#!/usr/bin/env python3
"""Validate and analyze pgmcp experiment 303's locked paired TSV.

The runner writes one timed observation per arm. This analyzer first rejects any
protocol drift or semantic-witness mismatch, then computes the preregistered
one-sided Welch test, Cohen effect size, median ratio, and paired-bootstrap 99%
upper confidence bound. The bootstrap resamples complete AB/BA pairs, never
individual arms, so window-local variation remains paired.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import math
import random
import re
import statistics
from collections import defaultdict
from pathlib import Path

from scipy import __version__ as scipy_version
from scipy.stats import ttest_ind

EXPERIMENT_ID = "303"
SEED_TEXT = "0x4131d35909"
SEED = 0x0041_31D3_5909
TREATMENT_GIT = "3582f8a72813cb9ee2d07d5b7d9e9cb16d64cfb5"
FROZEN_CONTROL_GIT = "c847838127231fcfcd9a547a3591c86313a1626c"
FROZEN_SOURCE_SHA256 = (
    "7c4d4bf64e4705d27b55e3d1484464d13ce2b2ccc52e37242bde6be54f13aacf"
)
EXPECTED_WITNESS = "0f20f52fcef3aaca"
EXPECTED_CALLBACK_STATE = "e07385157d37dab8"
LOCKED_DEPTH = 1_024
LOCKED_WARMUP_PAIRS = 3
LOCKED_RETAINED_PAIRS = 30
EXPECTED_HEADER = (
    "phase\tpair\torder\tarm\telapsed_ns\tdepth\tseed\twitness\tcallback_count"
    "\tcallback_state\tcpu_before\tcpu_after\ttreatment_ref\tfrozen_control_git"
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--bootstrap-replicates", type=int, default=200_000)
    return parser.parse_args()


def read_tsv(path: Path) -> tuple[bytes, dict[str, str], list[dict[str, str]]]:
    raw = path.read_bytes()
    text = raw.decode("utf-8")
    metadata: dict[str, str] = {}
    data_lines: list[str] = []
    for line in text.splitlines(keepends=True):
        if line.startswith("# "):
            key, separator, value = line[2:].rstrip("\r\n").partition("=")
            if not separator or key in metadata:
                raise ValueError(f"invalid or duplicate metadata line: {line.rstrip()}")
            metadata[key] = value
        else:
            data_lines.append(line)
    if not data_lines:
        raise ValueError("measurement file has no TSV header")
    require_equal(data_lines[0].rstrip("\r\n"), EXPECTED_HEADER, "TSV header")
    rows = list(csv.DictReader(io.StringIO("".join(data_lines)), delimiter="\t"))
    if not rows:
        raise ValueError("measurement file contains no rows")
    return raw, metadata, rows


def require_equal(actual: object, expected: object, label: str) -> None:
    if actual != expected:
        raise ValueError(f"{label}: expected {expected!r}, got {actual!r}")


def validate_protocol(
    metadata: dict[str, str], rows: list[dict[str, str]]
) -> dict[tuple[str, int], dict[str, dict[str, str]]]:
    require_equal(metadata.get("experiment_id"), EXPERIMENT_ID, "experiment id")
    require_equal(metadata.get("mode"), "locked", "mode")
    require_equal(int(metadata.get("depth", "-1")), LOCKED_DEPTH, "depth")
    require_equal(
        int(metadata.get("warmup_pairs", "-1")),
        LOCKED_WARMUP_PAIRS,
        "warm-up pair count",
    )
    require_equal(
        int(metadata.get("retained_pairs", "-1")),
        LOCKED_RETAINED_PAIRS,
        "retained pair count",
    )
    require_equal(metadata.get("seed"), SEED_TEXT, "seed")
    require_equal(
        metadata.get("frozen_control_git"),
        FROZEN_CONTROL_GIT,
        "frozen control revision",
    )
    require_equal(
        metadata.get("frozen_source_sha256"),
        FROZEN_SOURCE_SHA256,
        "frozen source digest",
    )
    treatment_ref = metadata.get("treatment_ref")
    require_equal(treatment_ref, TREATMENT_GIT, "treatment revision")
    if treatment_ref is None or re.fullmatch(r"[0-9a-f]{40}", treatment_ref) is None:
        raise ValueError("treatment_ref must be a lowercase 40-hex-digit Git object id")
    if re.fullmatch(r"[0-9a-f]{40}", FROZEN_CONTROL_GIT) is None:
        raise AssertionError("invalid frozen control revision constant")

    grouped: dict[tuple[str, int], dict[str, dict[str, str]]] = defaultdict(dict)
    affinity = metadata.get("affinity_cpus")
    if affinity is None or not affinity.startswith("[") or not affinity.endswith("]"):
        raise ValueError("one-CPU affinity metadata is absent")
    expected_cpu = affinity[1:-1]
    if not expected_cpu.isdigit():
        raise ValueError(f"locked affinity must contain exactly one CPU: {affinity}")

    expected_sequence: list[tuple[str, int, str]] = []
    for phase, count in (
        ("warmup", LOCKED_WARMUP_PAIRS),
        ("retained", LOCKED_RETAINED_PAIRS),
    ):
        for pair in range(count):
            arms = (
                ("control", "treatment") if pair % 2 == 0 else ("treatment", "control")
            )
            expected_sequence.extend((phase, pair, arm) for arm in arms)
    actual_sequence = [(row["phase"], int(row["pair"]), row["arm"]) for row in rows]
    require_equal(actual_sequence, expected_sequence, "raw AB/BA row sequence")

    for row in rows:
        phase = row["phase"]
        if phase not in {"warmup", "retained"}:
            raise ValueError(f"unknown phase {phase!r}")
        pair = int(row["pair"])
        arm = row["arm"]
        if arm not in {"control", "treatment"}:
            raise ValueError(f"unknown arm {arm!r}")
        key = (phase, pair)
        if arm in grouped[key]:
            raise ValueError(f"duplicate {arm} row in {phase} pair {pair}")
        grouped[key][arm] = row

        require_equal(int(row["depth"]), LOCKED_DEPTH, "row depth")
        require_equal(row["seed"], SEED_TEXT, "row seed")
        require_equal(row["treatment_ref"], treatment_ref, "row treatment ref")
        require_equal(
            row["frozen_control_git"],
            FROZEN_CONTROL_GIT,
            "row frozen control ref",
        )
        require_equal(row["callback_count"], "2", "callback count")
        require_equal(row["cpu_before"], expected_cpu, "CPU before")
        require_equal(row["cpu_after"], expected_cpu, "CPU after")
        if int(row["elapsed_ns"]) <= 0:
            raise ValueError("elapsed time must be positive")

        require_equal(row["witness"], EXPECTED_WITNESS, "semantic witness")
        require_equal(
            row["callback_state"], EXPECTED_CALLBACK_STATE, "callback final state"
        )

    phase_counts = {
        "warmup": LOCKED_WARMUP_PAIRS,
        "retained": LOCKED_RETAINED_PAIRS,
    }
    for phase, count in phase_counts.items():
        expected_pairs = set(range(count))
        actual_pairs = {pair for row_phase, pair in grouped if row_phase == phase}
        require_equal(actual_pairs, expected_pairs, f"{phase} pair identifiers")
        for pair in range(count):
            arms = grouped[(phase, pair)]
            require_equal(
                set(arms), {"control", "treatment"}, f"{phase} pair {pair} arms"
            )
            expected_order = "AB" if pair % 2 == 0 else "BA"
            require_equal(
                {arms["control"]["order"], arms["treatment"]["order"]},
                {expected_order},
                f"{phase} pair {pair} order",
            )

    require_equal(len(rows), 2 * sum(phase_counts.values()), "total row count")
    return grouped


def sample_summary(values: list[float]) -> dict[str, float | int]:
    return {
        "n": len(values),
        "min": min(values),
        "median": statistics.median(values),
        "mean": statistics.mean(values),
        "sample_stddev": statistics.stdev(values),
        "max": max(values),
    }


def percentile(values: list[float], probability: float) -> float:
    ordered = sorted(values)
    position = probability * (len(ordered) - 1)
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    fraction = position - lower
    return ordered[lower] * (1.0 - fraction) + ordered[upper] * fraction


def paired_bootstrap_upper(
    pairs: list[tuple[float, float]], replicates: int
) -> tuple[float, float, float]:
    if replicates < 10_000:
        raise ValueError("at least 10,000 bootstrap replicates are required")
    generator = random.Random(SEED)
    ratios: list[float] = []
    for _ in range(replicates):
        sample = [pairs[generator.randrange(len(pairs))] for _ in pairs]
        controls = [control for control, _ in sample]
        treatments = [treatment for _, treatment in sample]
        ratios.append(statistics.median(treatments) / statistics.median(controls))
    return (
        percentile(ratios, 0.005),
        percentile(ratios, 0.5),
        percentile(ratios, 0.99),
    )


def cohen_d(treatment: list[float], control: list[float]) -> float:
    treatment_variance = statistics.variance(treatment)
    control_variance = statistics.variance(control)
    pooled = math.sqrt(
        (
            (len(treatment) - 1) * treatment_variance
            + (len(control) - 1) * control_variance
        )
        / (len(treatment) + len(control) - 2)
    )
    if not math.isfinite(pooled) or pooled == 0.0:
        raise ValueError("Cohen's d requires finite, nonzero pooled sample variance")
    effect = (statistics.mean(treatment) - statistics.mean(control)) / pooled
    if not math.isfinite(effect):
        raise ValueError("Cohen's d is non-finite")
    return effect


def analyze(
    path: Path,
    raw: bytes,
    metadata: dict[str, str],
    grouped: dict[tuple[str, int], dict[str, dict[str, str]]],
    bootstrap_replicates: int,
) -> dict[str, object]:
    retained: list[tuple[float, float, str]] = []
    for pair in range(LOCKED_RETAINED_PAIRS):
        arms = grouped[("retained", pair)]
        retained.append(
            (
                float(arms["control"]["elapsed_ns"]),
                float(arms["treatment"]["elapsed_ns"]),
                arms["control"]["order"],
            )
        )

    controls = [control for control, _, _ in retained]
    treatments = [treatment for _, treatment, _ in retained]
    welch = ttest_ind(treatments, controls, equal_var=False, alternative="less")
    if not math.isfinite(float(welch.statistic)) or not math.isfinite(
        float(welch.pvalue)
    ):
        raise ValueError("Welch test produced a non-finite result")
    effect = cohen_d(treatments, controls)
    median_ratio = statistics.median(treatments) / statistics.median(controls)
    bootstrap_low, bootstrap_median, bootstrap_upper = paired_bootstrap_upper(
        [(control, treatment) for control, treatment, _ in retained],
        bootstrap_replicates,
    )

    order_effect: dict[str, object] = {}
    for order in ("AB", "BA"):
        subset = [pair for pair in retained if pair[2] == order]
        subset_controls = [control for control, _, _ in subset]
        subset_treatments = [treatment for _, treatment, _ in subset]
        order_effect[order] = {
            "n_pairs": len(subset),
            "control_median_ns": statistics.median(subset_controls),
            "treatment_median_ns": statistics.median(subset_treatments),
            "ratio_of_medians": statistics.median(subset_treatments)
            / statistics.median(subset_controls),
            "median_pair_ratio": statistics.median(
                treatment / control for control, treatment, _ in subset
            ),
        }

    warmups = {
        arm: [
            int(grouped[("warmup", pair)][arm]["elapsed_ns"])
            for pair in range(LOCKED_WARMUP_PAIRS)
        ]
        for arm in ("control", "treatment")
    }
    raw_digest = hashlib.sha256(raw).hexdigest()
    gates = {
        "one_sided_welch_p_lt_0_01": float(welch.pvalue) < 0.01,
        "cohens_d_magnitude_gte_1": abs(effect) >= 1.0,
        "median_treatment_control_lte_0_25": median_ratio <= 0.25,
        "paired_bootstrap_99_upper_lt_0_50": bootstrap_upper < 0.50,
    }
    return {
        "experiment_id": int(EXPERIMENT_ID),
        "locked_tsv_protocol_valid": True,
        "input": str(path),
        "input_sha256": raw_digest,
        "metadata": metadata,
        "scipy_version": scipy_version,
        "bootstrap_seed": SEED_TEXT,
        "bootstrap_replicates": bootstrap_replicates,
        "warmup_elapsed_ns": warmups,
        "control_elapsed_ns": sample_summary(controls),
        "treatment_elapsed_ns": sample_summary(treatments),
        "median_treatment_control_ratio": median_ratio,
        "one_sided_welch": {
            "alternative": "treatment less than control",
            "statistic": float(welch.statistic),
            "pvalue": float(welch.pvalue),
        },
        "cohens_d_treatment_minus_control": effect,
        "paired_bootstrap_ratio": {
            "lower_0_5_percent": bootstrap_low,
            "median": bootstrap_median,
            "upper_99_percent": bootstrap_upper,
        },
        "order_effect": order_effect,
        "acceptance_gates": gates,
        "all_locked_timing_gates_pass": all(gates.values()),
    }


def main() -> None:
    args = parse_args()
    if args.output is not None and args.output.resolve() == args.input.resolve():
        raise ValueError("analysis output must not overwrite the raw measurement input")
    raw, metadata, rows = read_tsv(args.input)
    grouped = validate_protocol(metadata, rows)
    result = analyze(args.input, raw, metadata, grouped, args.bootstrap_replicates)
    rendered = json.dumps(result, indent=2, sort_keys=True, allow_nan=False) + "\n"
    if args.output is not None:
        with args.output.open("x", encoding="utf-8") as stream:
            stream.write(rendered)
    print(rendered, end="")


if __name__ == "__main__":
    main()

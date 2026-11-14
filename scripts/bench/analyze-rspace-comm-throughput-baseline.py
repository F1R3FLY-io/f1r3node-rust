"""Analyze the preregistered current-epoch RSpace COMM throughput baseline (task 14264).

Task 14264 is the successor of pgmcp hypothesis 208, whose locked protocol it repeats on the unchanged harness
rspace++/benches/rspace_comm.rs: metric comm_operations_per_second, Welch's t-test with the alternative "pathmap is
greater than control", alpha 0.05, |Cohen's d| >= 0.5, 51 samples per arm. The decided cell is
waiting_receive_shared_prefix at 4,096 waiting receives; every other cell is reported as context.

One observation is one Criterion sample of baseline <name>: sample i of a benchmark ran iters[i] COMMs in
times[i] nanoseconds, so its throughput is ops_per_second = 1e9 * iters[i] / times[i] and its mean latency is
mean_ns = times[i] / iters[i].

The analyzer rejects an incomplete run before computing anything (exit 2): every one of the 37 benchmarks must
hold 51 samples, reach "Analyzing" in the run log, and no panic may occur anywhere in the log.

Red control (the leaf's acceptance): the same test on two halves of the control arm of the decided cell, the even
sample indices (26 samples, pseudo-treatment) against the odd ones (25 samples, pseudo-control). If that A/A split
is accepted, the run is void (exit 3) and is repeated once on a quiet host with baseline suffix -2. A/A splits of
the other two-arm cells are reported as context and decide nothing.

Outputs: the samples CSV (group, arm, size, sample, iters, time_ns, mean_ns, ops_per_second, role), with role
"decided" for the two arms of the decided cell and "context" for every other row, and the summary TSV, whose "# "
header records the facts of the window, the analyzer's view of the decision and the SHA-256 of the log, the
samples CSV and this analyzer.

Exit status: 0 for a valid run, 3 for a void run, 2 for an invalid input.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import math
import platform
import statistics
import sys
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import numpy as np
import scipy
from scipy.stats import ttest_ind

from paired_benchmark_statistics import sample_summary

SCHEMA = "rspace-comm-throughput-current-epoch-v1"
CONTROL = "control"
TREATMENT = "pathmap"
SINGLE_ARM = "single_arm"
ALPHA = 0.05
MIN_COHENS_D = 0.5
SAMPLES = 51
SELF_TEST_SEED = 0x4131_4264
TWO_ARM_GROUPS: tuple[tuple[str, tuple[int, ...]], ...] = (
    ("waiting_receive_diverse_prefix", (16, 256, 4096)),
    ("waiting_receive_shared_prefix", (16, 256, 4096)),
    ("distinct_diverse_prefix", (1, 16, 256, 4096)),
    ("distinct_shared_prefix", (1, 16, 256, 4096)),
)
SINGLE_ARM_GROUPS: tuple[tuple[str, tuple[int, ...]], ...] = (
    ("duplicate_multiplicity", (1, 16, 256, 4096)),
    ("join_arity", (1, 2, 4, 8, 16)),
)
PRIMARY = ("waiting_receive_shared_prefix", 4096)
SAMPLE_COLUMNS = ("group", "arm", "size", "sample", "iters", "time_ns", "mean_ns", "ops_per_second", "role")
SUMMARY_COLUMNS = (
    "cell",
    "size",
    "role",
    "control_n",
    "pathmap_n",
    "control_mean_ops_per_second",
    "control_standard_deviation",
    "pathmap_mean_ops_per_second",
    "pathmap_standard_deviation",
    "control_mean_ns",
    "pathmap_mean_ns",
    "throughput_ratio",
    "welch_t",
    "welch_p_greater",
    "cohens_d",
    "rule_accepted",
    "aa_welch_p_greater",
    "aa_cohens_d",
    "aa_rule_accepted",
)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path, help="the window's CRITERION_HOME")
    parser.add_argument("--baseline", required=True)
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--stem", type=Path, required=True)
    parser.add_argument("--facts", type=Path)
    return parser.parse_args(argv)


def sha256_of(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def number_text(value: float) -> str:
    if float(value).is_integer() and abs(value) < 2**53:
        return str(int(value))
    return repr(float(value))


def read_sample(path: Path, expected: int) -> list[tuple[float, float]]:
    try:
        data = json.loads(path.read_text())
    except FileNotFoundError as error:
        raise ValueError(f"{path}: missing") from error
    if not isinstance(data, dict) or set(data) != {"sampling_mode", "iters", "times"}:
        raise ValueError(f"{path}: not a Criterion sample.json")
    if data["sampling_mode"] not in ("Linear", "Flat"):
        raise ValueError(f"{path}: unknown sampling mode {data['sampling_mode']!r}")
    iters, times = data["iters"], data["times"]
    if len(iters) != len(times):
        raise ValueError(f"{path}: {len(iters)} iteration counts for {len(times)} times")
    if len(iters) != expected:
        raise ValueError(f"{path}: {len(iters)} samples, {expected} preregistered")
    pairs = []
    for index, (count, elapsed) in enumerate(zip(iters, times)):
        count, elapsed = float(count), float(elapsed)
        if not (math.isfinite(count) and count > 0 and count.is_integer()):
            raise ValueError(f"{path}: sample {index} has iteration count {count!r}")
        if not (math.isfinite(elapsed) and elapsed > 0):
            raise ValueError(f"{path}: sample {index} has time {elapsed!r}")
        pairs.append((count, elapsed))
    return pairs


def benchmarks() -> list[tuple[str, str, int, str]]:
    """Every benchmark as (group, arm, size, Criterion id), in the harness's order."""
    found = []
    for group, sizes in TWO_ARM_GROUPS:
        for size in sizes:
            for arm in (CONTROL, TREATMENT):
                found.append((group, arm, size, f"{group}/{arm}/{size}"))
    for group, sizes in SINGLE_ARM_GROUPS:
        for size in sizes:
            found.append((group, SINGLE_ARM, size, f"{group}/{size}"))
    return found


def read_matrix(root: Path, baseline: str) -> dict[tuple[str, str, int], list[tuple[float, float]]]:
    return {
        (group, arm, size): read_sample(root / identifier / baseline / "sample.json", SAMPLES)
        for group, arm, size, identifier in benchmarks()
    }


def check_log(text: str) -> None:
    lines = text.splitlines()
    panics = [line for line in lines if "panicked at" in line]
    if panics:
        raise ValueError(f"the log holds a panic: {panics[0]!r}")
    analyzed = {line.rstrip() for line in lines}
    missing = [
        identifier
        for _, _, _, identifier in benchmarks()
        if f"Benchmarking {identifier}: Analyzing" not in analyzed
    ]
    if missing:
        raise ValueError(f"{len(missing)} benchmarks never reach Analyzing in the log, first {missing[0]}")


def throughput(pairs: list[tuple[float, float]]) -> list[float]:
    return [1e9 * count / elapsed for count, elapsed in pairs]


def per_iteration(pairs: list[tuple[float, float]]) -> list[float]:
    return [elapsed / count for count, elapsed in pairs]


def cohens_d(control: list[float], treatment: list[float]) -> float:
    count_control, count_treatment = len(control), len(treatment)
    pooled = math.sqrt(
        (
            (count_control - 1) * statistics.variance(control)
            + (count_treatment - 1) * statistics.variance(treatment)
        )
        / (count_control + count_treatment - 2)
    )
    if pooled == 0:
        return 0.0
    return (statistics.mean(treatment) - statistics.mean(control)) / pooled


def rule(control: list[float], treatment: list[float]) -> dict[str, Any]:
    """pgmcp's welch_t {alpha 0.05, tail greater, min_effect cohens_d 0.5}: p < alpha and |d| >= 0.5."""
    test = ttest_ind(treatment, control, equal_var=False, alternative="greater")
    statistic, pvalue = float(test.statistic), float(test.pvalue)
    effect = cohens_d(control, treatment)
    accepted = math.isfinite(pvalue) and pvalue < ALPHA and abs(effect) >= MIN_COHENS_D
    return {"welch_t": statistic, "welch_p_greater": pvalue, "cohens_d": effect, "accepted": accepted}


def aa_split(control: list[float]) -> tuple[list[float], list[float]]:
    """The A/A split: even sample indices (pseudo-treatment) and odd sample indices (pseudo-control)."""
    return control[0::2], control[1::2]


def two_arm_cell(group: str, size: int, matrix: dict[tuple[str, str, int], list[tuple[float, float]]]) -> dict[str, Any]:
    control = throughput(matrix[(group, CONTROL, size)])
    treatment = throughput(matrix[(group, TREATMENT, size)])
    even, odd = aa_split(control)
    control_summary = sample_summary(control)
    treatment_summary = sample_summary(treatment)
    return {
        "cell": f"{group}/{size}",
        "group": group,
        "size": size,
        "control_summary": control_summary,
        "treatment_summary": treatment_summary,
        "control_mean_ns": statistics.mean(per_iteration(matrix[(group, CONTROL, size)])),
        "treatment_mean_ns": statistics.mean(per_iteration(matrix[(group, TREATMENT, size)])),
        "ratio": treatment_summary["mean"] / control_summary["mean"],
        "rule": rule(control, treatment),
        "aa_rule": rule(odd, even),
        "aa_sizes": (len(even), len(odd)),
    }


def sample_rows(matrix: dict[tuple[str, str, int], list[tuple[float, float]]]) -> str:
    buffer = io.StringIO()
    writer = csv.writer(buffer, lineterminator="\n")
    writer.writerow(SAMPLE_COLUMNS)
    for group, arm, size, _ in benchmarks():
        role = "decided" if (group, size) == PRIMARY else "context"
        for index, (count, elapsed) in enumerate(matrix[(group, arm, size)]):
            writer.writerow(
                (
                    group,
                    arm,
                    size,
                    index,
                    number_text(count),
                    number_text(elapsed),
                    repr(elapsed / count),
                    repr(1e9 * count / elapsed),
                    role,
                )
            )
    return buffer.getvalue()


def fixed(value: float | None, digits: int = 6) -> str:
    if value is None:
        return "NA"
    return f"{value:.{digits}f}"


def summary_table(
    cells: list[dict[str, Any]], matrix: dict[tuple[str, str, int], list[tuple[float, float]]]
) -> list[str]:
    lines = ["\t".join(SUMMARY_COLUMNS)]
    for cell in cells:
        role = "decided" if (cell["group"], cell["size"]) == PRIMARY else "context"
        result, aa = cell["rule"], cell["aa_rule"]
        lines.append(
            "\t".join(
                (
                    cell["cell"],
                    str(cell["size"]),
                    role,
                    str(cell["control_summary"]["n"]),
                    str(cell["treatment_summary"]["n"]),
                    fixed(cell["control_summary"]["mean"], 3),
                    fixed(cell["control_summary"]["standard_deviation"], 3),
                    fixed(cell["treatment_summary"]["mean"], 3),
                    fixed(cell["treatment_summary"]["standard_deviation"], 3),
                    fixed(cell["control_mean_ns"], 3),
                    fixed(cell["treatment_mean_ns"], 3),
                    fixed(cell["ratio"]),
                    fixed(result["welch_t"], 4),
                    f"{result['welch_p_greater']:.6g}",
                    fixed(result["cohens_d"], 4),
                    "yes" if result["accepted"] else "no",
                    f"{aa['welch_p_greater']:.6g}",
                    fixed(aa["cohens_d"], 4),
                    "yes" if aa["accepted"] else "no",
                )
            )
        )
    for group, sizes in SINGLE_ARM_GROUPS:
        for size in sizes:
            values = throughput(matrix[(group, SINGLE_ARM, size)])
            summary = sample_summary(values)
            lines.append(
                "\t".join(
                    (
                        f"{group}/{size}",
                        str(size),
                        "context",
                        "NA",
                        str(summary["n"]),
                        "NA",
                        "NA",
                        fixed(summary["mean"], 3),
                        fixed(summary["standard_deviation"], 3),
                        "NA",
                        fixed(statistics.mean(per_iteration(matrix[(group, SINGLE_ARM, size)])), 3),
                        *(["NA"] * 8),
                    )
                )
            )
    return lines


def read_facts(path: Path | None) -> list[str]:
    if path is None:
        return []
    facts = json.loads(path.read_text())
    lines = facts.get("header", [])
    if not isinstance(lines, list) or not all(isinstance(line, str) for line in lines):
        raise ValueError(f"{path}: header must be a list of strings")
    if any("\n" in line or "\r" in line for line in lines):
        raise ValueError(f"{path}: a header line holds a line break")
    return lines


def analyze(args: argparse.Namespace) -> int:
    analyzer = Path(__file__).resolve().read_bytes()
    log_bytes = args.log.read_bytes()
    check_log(log_bytes.decode("utf-8", errors="replace"))
    matrix = read_matrix(args.input, args.baseline)
    cells = [two_arm_cell(group, size, matrix) for group, sizes in TWO_ARM_GROUPS for size in sizes]
    samples_text = sample_rows(matrix)
    samples_path = Path(f"{args.stem}.samples.csv")
    summary_path = Path(f"{args.stem}.tsv")
    samples_path.write_text(samples_text)

    primary = next(cell for cell in cells if (cell["group"], cell["size"]) == PRIMARY)
    result, aa = primary["rule"], primary["aa_rule"]
    void = aa["accepted"]
    context_aa = [cell["cell"] for cell in cells if cell["aa_rule"]["accepted"] and cell is not primary]
    header = [
        "RSpace COMM throughput, current epoch: the successor of pgmcp hypothesis 208 (task 14264).",
        f"Schema {SCHEMA}; analyzer sha256:{sha256_of(analyzer)}; Criterion baseline {args.baseline}.",
        *read_facts(args.facts),
        "Statistics: one observation is one Criterion sample, ops_per_second = 1e9 * iters / time_ns; the rule of"
        f" the decided cell {PRIMARY[0]}/{PRIMARY[1]} is Welch's t-test (pathmap greater than control) at alpha"
        f" {ALPHA} with |Cohen's d| >= {MIN_COHENS_D}, {SAMPLES} samples per arm; every other cell is context.",
        f"Software: Python {platform.python_version()}, NumPy {np.__version__}, SciPy {scipy.__version__}.",
        "Validity: all 37 benchmarks hold 51 samples, reach Analyzing in the log and no panic occurs; red control"
        f" (A/A split of the decided cell's control arm, even indices {primary['aa_sizes'][0]} against odd"
        f" {primary['aa_sizes'][1]}, same rule): "
        + (
            f"VOID: the rule accepts the A/A split (p {aa['welch_p_greater']:.6g}, d {aa['cohens_d']:.4f})."
            if void
            else f"not accepted (p {aa['welch_p_greater']:.6g}, d {aa['cohens_d']:.4f})."
        )
        + (
            f" Context A/A splits accepted by the rule (decide nothing): {', '.join(context_aa)}."
            if context_aa
            else " No context A/A split is accepted by the rule."
        ),
        f"Analyzer view of the decision ({PRIMARY[0]}/{PRIMARY[1]}): {'ACCEPTED' if result['accepted'] else 'REJECTED'};"
        f" pathmap/control throughput ratio {primary['ratio']:.6f}, Welch t {result['welch_t']:.4f}, one-sided p"
        f" {result['welch_p_greater']:.6g}, Cohen's d {result['cohens_d']:.4f}.",
        f"SHA-256: {args.log.name} {sha256_of(log_bytes)}; {samples_path.name} {sha256_of(samples_text.encode())};"
        f" {Path(__file__).name} {sha256_of(analyzer)}.",
    ]
    lines = [f"# {line}" for line in header] + summary_table(cells, matrix)
    summary_path.write_text("\n".join(lines) + "\n")
    print(f"decision {'accepted' if result['accepted'] else 'rejected'} by the analyzer: ratio {primary['ratio']:.6f},"
          f" p {result['welch_p_greater']:.6g}, d {result['cohens_d']:.4f}")
    print(f"red control: A/A {'ACCEPTED (void)' if void else 'not accepted'} (p {aa['welch_p_greater']:.6g})")
    print(f"wrote {samples_path}, {summary_path}")
    return 3 if void else 0


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def self_test() -> int:
    generator = np.random.default_rng(SELF_TEST_SEED)
    control = [float(value) for value in 200_000.0 * np.exp(generator.normal(0.0, 0.02, SAMPLES))]
    even, odd = aa_split(control)
    require((len(even), len(odd)) == (26, 25), f"the A/A split of 51 samples is {len(even)} and {len(odd)}")
    aa = rule(odd, even)
    require(not aa["accepted"], f"the rule accepts the A/A split of a quiet control (p {aa['welch_p_greater']})")
    faster = [value * 1.10 for value in control]
    accepted = rule(control, faster)
    require(accepted["accepted"], "the rule rejects 10% more throughput")
    slower = [value * 0.90 for value in control]
    rejected = rule(control, slower)
    require(not rejected["accepted"], "the rule accepts 10% less throughput")
    tiny = [value * 1.002 for value in control]
    gated = rule(control, tiny)
    require(not gated["accepted"], f"the effect gate passes a 0.2% shift (d {gated['cohens_d']})")
    shifted = [value * 1.05 for value in control]
    loud_aa = rule(odd, shifted[0::2])
    require(loud_aa["accepted"], "a 5% shifted half is not accepted, so the A/A check could never fire")
    require(throughput([(2.0, 4000.0)]) == [5e5], "ops_per_second must be 1e9 * iters / time_ns")
    require(per_iteration([(1.0, 10.0), (2.0, 30.0)]) == [10.0, 15.0], "mean_ns must be time_ns / iters")
    require(number_text(3.0) == "3" and number_text(2.5) == "2.5", "integral values must print as integers")
    require(len(benchmarks()) == 37, f"{len(benchmarks())} benchmarks, 37 expected")
    print(
        f"self-test: A/A rule rejected (p {aa['welch_p_greater']:.6g}, d {aa['cohens_d']:.4f}); x1.10 accepted"
        f" (p {accepted['welch_p_greater']:.3g}); x0.90 rejected; x1.002 gated (d {gated['cohens_d']:.4f});"
        " a 5% shifted half is accepted, so the void check can fire"
    )
    print("self-test passed")
    return 0


def main(argv: list[str]) -> int:
    if argv == ["--self-test"]:
        return self_test()
    try:
        return analyze(parse_args(argv))
    except ValueError as error:
        print(f"invalid input: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

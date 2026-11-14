"""Analyze the preregistered current-epoch accounting-demand scaling experiment (task 14281).

Task 14281 is the successor of pgmcp hypothesis 303 (criterion 878 of
accounting-demand-local-occurrence-linear-walk), which no longer exists. The harness
rholang/benches/accounting_demand.rs is unchanged and runs once in full (no filter): its eight groups, 25 benchmarks,
each with the group's own sample_size(30), 1 s warm-up and 4 s measurement. The decided group is
accounting_demand/byte_pda at depths 128, 512, 2,048, 8,192 and 20,000; the frozen quadratic group
accounting_demand/suffix_control at depths 64, 128, 256 and 512 is the red control; the other six groups are context.

One observation is one Criterion sample: sample i ran iters[i] demand computations (bencher.iter) in times[i]
nanoseconds, so its time per computation is time_ns / iters (Criterion may choose flat sampling for the slow
suffix_control cells; the quotient is the same).

Hypothesis P: the 95% upper bound of the log-log exponent of byte_pda median time against depth is at most 1.20. The
exponent is the ordinary least-squares slope of log(median time of a cell) against log(depth); its bounds come from
a stratified bootstrap that resamples each cell's 30 samples with replacement (10,000 replicates, seed 0x41315636,
cells in increasing depth). This is the method of scripts/bench/paired_benchmark_statistics.py
stratified_loglog_slope with the cell statistic the median (the leaf's "median time") instead of the mean of the
logarithms; upper95 is the 97.5th percentile of the replicates, as stratified_loglog_slope names it (the upper end of
the two-sided 95% interval), and the one-sided 95th percentile is reported beside it. pgmcp decides P with
absolute_threshold {max, le, 1.20} over byte_pda's upper95 (arm byte_pda).

Red control (the leaf's acceptance): the same fit on suffix_control must give an exponent above 1.5 and be rejected,
that is estimate > 1.5 and upper95 > 1.20; its upper95 is P's control arm suffix_control in pgmcp. If the red control
fails, the fit cannot be shown to reject a superlinear walk and the leaf's acceptance cannot be met: the run is
invalid (exit 2) and is reported to the project owner.

Standing gates (the leaf's acceptance): the log must show accounting_demand_and_decomposition_are_stack_safe and
accounting_demand_allocations_are_linear_in_encoded_depth passing ("test result: ok. 2 passed") in the window's
standing test run, which ran before the timed run at the measured commit.

The analyzer rejects an incomplete run before computing anything (exit 2): each of the 25 benchmarks must hold 30
samples and reach "Analyzing" in the run log, the standing gates must have passed, and no panic may occur anywhere in
the log.

Outputs: <stem>.samples.csv (group, parameter, sample, iters, time_ns, time_per_iteration_ns, role: decided,
red_control or context), <stem>.exponents.csv (hypothesis, arm, role, estimate, lower95, upper95, upper95_one_sided)
and the summary <stem>.tsv, whose "# " header records the facts of the window, the analyzer's view of the decision
and the SHA-256 of the log, both CSV files and this analyzer.

Exit status: 0 for a valid run, 2 for an invalid input or a failed red control.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import math
import platform
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import numpy as np
import scipy

from paired_benchmark_statistics import sample_summary, stratified_loglog_slope

SCHEMA = "accounting-demand-scaling-current-epoch-v1"
PREFIX = "accounting_demand"
DECIDED = "byte_pda"
RED_CONTROL = "suffix_control"
GROUPS = (
    (DECIDED, (128, 512, 2048, 8192, 20000)),
    (RED_CONTROL, (64, 128, 256, 512)),
    ("wide_scalar", (16, 64, 256)),
    ("lane_byte_native", (16, 64)),
    ("lane_par_adapter", (16, 64)),
    ("compressed_scalar", (1, 1024, 4294967295)),
    ("compressed_lane_replay", (1, 64, 1024)),
    ("unknown_scan", (16, 64, 256)),
)
SAMPLES = 30
BOUND = 1.20
RED_CONTROL_MIN_EXPONENT = 1.5
SEED = 0x4131_5636
REPLICATES = 10_000
SELF_TEST_SEED = 0x4131_4281
STANDING_GATES = (
    "accounting_demand_and_decomposition_are_stack_safe",
    "accounting_demand_allocations_are_linear_in_encoded_depth",
)
SAMPLE_COLUMNS = ("group", "parameter", "sample", "iters", "time_ns", "time_per_iteration_ns", "role")
EXPONENT_COLUMNS = ("hypothesis", "arm", "role", "estimate", "lower95", "upper95", "upper95_one_sided")
SUMMARY_COLUMNS = (
    "cell",
    "group",
    "parameter",
    "role",
    "n",
    "median_ns",
    "mean_ns",
    "standard_deviation_ns",
    "sampling_mode",
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


def role_of(group: str) -> str:
    return {DECIDED: "decided", RED_CONTROL: "red_control"}.get(group, "context")


def benchmarks() -> list[tuple[str, int, str, str]]:
    """Every benchmark as (group, parameter, Criterion id, directory), in the harness's order.

    Criterion names the group accounting_demand/<group>; make_filename_safe turns its "/" into "_" on disk."""
    return [
        (group, parameter, f"{PREFIX}/{group}/{parameter}", f"{PREFIX}_{group}/{parameter}")
        for group, parameters in GROUPS
        for parameter in parameters
    ]


def read_sample(path: Path, expected: int) -> tuple[str, list[tuple[float, float]]]:
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
    return data["sampling_mode"], pairs


def read_matrix(root: Path, baseline: str) -> dict[tuple[str, int], tuple[str, list[tuple[float, float]]]]:
    return {
        (group, parameter): read_sample(root / directory / baseline / "sample.json", SAMPLES)
        for group, parameter, _, directory in benchmarks()
    }


def check_log(text: str) -> None:
    lines = text.splitlines()
    panics = [line for line in lines if "panicked at" in line]
    if panics:
        raise ValueError(f"the log holds a panic: {panics[0]!r}")
    stripped = {line.rstrip() for line in lines}
    missing = [
        identifier
        for _, _, identifier, _ in benchmarks()
        if f"Benchmarking {identifier}: Analyzing" not in stripped
    ]
    if missing:
        raise ValueError(f"{len(missing)} benchmarks never reach Analyzing in the log, first {missing[0]}")
    failed = [name for name in STANDING_GATES if f"test {name} ... ok" not in stripped]
    if failed or not any(line.startswith("test result: ok. 2 passed") for line in stripped):
        raise ValueError(f"the log does not show both standing gates passing (missing {failed or 'the 2-passed result'})")


def per_iteration(pairs: list[tuple[float, float]]) -> list[float]:
    return [elapsed / count for count, elapsed in pairs]


def stratum_log_statistic(values: np.ndarray, statistic: str) -> np.ndarray:
    """log of the per-stratum statistic over the last axis; "geometric" is the mean of the logarithms."""
    if statistic == "median":
        return np.log(np.median(values, axis=-1))
    if statistic == "mean":
        return np.log(values.mean(axis=-1))
    if statistic == "geometric":
        return np.log(values).mean(axis=-1)
    raise ValueError(f"unknown statistic {statistic!r}")


def loglog_exponent(sizes: tuple[int, ...] | list[int], samples: list[list[float]], statistic: str,
                    replicates: int = REPLICATES, seed: int = SEED) -> dict[str, float]:
    """OLS slope of log(statistic of each stratum) on log(size), with a stratified bootstrap of its bounds."""
    if len(sizes) < 3 or len(sizes) != len(samples):
        raise ValueError("at least three aligned strata are required")
    if len(set(sizes)) != len(sizes) or any(size <= 0 for size in sizes):
        raise ValueError("sizes must be positive and distinct")
    if replicates < REPLICATES:
        raise ValueError(f"at least {REPLICATES} bootstrap replicates are required")
    widths = {len(group) for group in samples}
    if len(widths) != 1 or next(iter(widths)) < 3:
        raise ValueError("strata require equal sample sizes of at least three")
    values = np.asarray(samples, dtype=np.float64)
    if not np.all(np.isfinite(values)) or np.any(values <= 0):
        raise ValueError("observations must be finite and positive")
    x = np.log(np.asarray(sizes, dtype=np.float64))
    centered = x - np.mean(x)
    denominator = np.sum(centered**2)
    estimate = float(np.dot(centered, stratum_log_statistic(values, statistic)) / denominator)
    generator = np.random.default_rng(seed)
    boot = np.zeros(replicates)
    for index, group in enumerate(values):
        draws = generator.integers(0, len(group), size=(replicates, len(group)))
        boot += centered[index] * stratum_log_statistic(group[draws], statistic) / denominator
    low, high = np.quantile(boot, [0.025, 0.975])
    return {"estimate": estimate, "lower95": float(low), "upper95": float(high),
            "upper95_one_sided": float(np.quantile(boot, 0.95))}


def red_control_holds(fit: dict[str, float]) -> bool:
    """The leaf's red control: an exponent above 1.5, and the rule rejects (upper95 above the bound)."""
    return fit["estimate"] > RED_CONTROL_MIN_EXPONENT and fit["upper95"] > BOUND


def sample_rows(matrix: dict[tuple[str, int], tuple[str, list[tuple[float, float]]]]) -> str:
    buffer = io.StringIO()
    writer = csv.writer(buffer, lineterminator="\n")
    writer.writerow(SAMPLE_COLUMNS)
    for group, parameter, _, _ in benchmarks():
        for index, (count, elapsed) in enumerate(matrix[(group, parameter)][1]):
            writer.writerow((group, parameter, index, number_text(count), number_text(elapsed), repr(elapsed / count),
                             role_of(group)))
    return buffer.getvalue()


def exponent_rows(fits: list[tuple[str, str, dict[str, float]]]) -> str:
    buffer = io.StringIO()
    writer = csv.writer(buffer, lineterminator="\n")
    writer.writerow(EXPONENT_COLUMNS)
    for arm, role, fit in fits:
        writer.writerow(("P", arm, role, repr(fit["estimate"]), repr(fit["lower95"]), repr(fit["upper95"]),
                         repr(fit["upper95_one_sided"])))
    return buffer.getvalue()


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


def fit_text(name: str, fit: dict[str, float]) -> str:
    return (f"{name} exponent {fit['estimate']:.4f}, 95% interval [{fit['lower95']:.4f}, {fit['upper95']:.4f}],"
            f" one-sided 95th percentile {fit['upper95_one_sided']:.4f}")


def analyze(args: argparse.Namespace) -> int:
    analyzer = Path(__file__).resolve().read_bytes()
    log_bytes = args.log.read_bytes()
    check_log(log_bytes.decode("utf-8", errors="replace"))
    matrix = read_matrix(args.input, args.baseline)
    groups = dict(GROUPS)
    decided = loglog_exponent(groups[DECIDED], [per_iteration(matrix[(DECIDED, depth)][1])
                                                for depth in groups[DECIDED]], "median")
    red = loglog_exponent(groups[RED_CONTROL], [per_iteration(matrix[(RED_CONTROL, depth)][1])
                                                for depth in groups[RED_CONTROL]], "median")
    if not red_control_holds(red):
        raise ValueError(f"the red control failed: {fit_text(RED_CONTROL, red)}; it must exceed"
                         f" {RED_CONTROL_MIN_EXPONENT} and be rejected (upper95 above {BOUND}), so the leaf's"
                         " acceptance cannot be met; report to the project owner")
    samples_text = sample_rows(matrix)
    exponents_text = exponent_rows([(DECIDED, "decided", decided), (RED_CONTROL, "red_control", red)])
    samples_path = Path(f"{args.stem}.samples.csv")
    exponents_path = Path(f"{args.stem}.exponents.csv")
    summary_path = Path(f"{args.stem}.tsv")
    samples_path.write_text(samples_text)
    exponents_path.write_text(exponents_text)
    accepted = decided["upper95"] <= BOUND
    table = ["\t".join(SUMMARY_COLUMNS)]
    for group, parameter, identifier, _ in benchmarks():
        mode, pairs = matrix[(group, parameter)]
        values = per_iteration(pairs)
        summary = sample_summary(values)
        table.append("\t".join((identifier, group, str(parameter), role_of(group), str(summary["n"]),
                                f"{summary['median']:.3f}", f"{summary['mean']:.3f}",
                                f"{summary['standard_deviation']:.3f}", mode)))
    header = [
        "Accounting demand scaling, current epoch: the successor of pgmcp hypothesis 303 (task 14281).",
        f"Schema {SCHEMA}; analyzer sha256:{sha256_of(analyzer)}; Criterion baseline {args.baseline}.",
        *read_facts(args.facts),
        "Statistics: one observation is one Criterion sample, time_per_iteration_ns = time_ns / iters (one demand"
        f" computation per iteration); P: the 95% upper bound (97.5th percentile of a stratified bootstrap,"
        f" {REPLICATES} replicates, seed {SEED:#x}) of the OLS slope of log(median time) on log(depth) of {DECIDED}"
        f" over {', '.join(str(depth) for depth in groups[DECIDED])} is at most {BOUND}, {SAMPLES} samples per cell.",
        f"Software: Python {platform.python_version()}, NumPy {np.__version__}, SciPy {scipy.__version__}.",
        f"Validity: all 25 benchmarks hold {SAMPLES} samples, reach Analyzing in the log and no panic occurs; both"
        f" standing gates passed before the timed run ({' and '.join(STANDING_GATES)}: test result ok, 2 passed);"
        f" red control (the same fit on {RED_CONTROL} over {', '.join(str(depth) for depth in groups[RED_CONTROL])}):"
        f" {fit_text(RED_CONTROL, red)}: above {RED_CONTROL_MIN_EXPONENT} and rejected, upper95 {red['upper95']:.4f}"
        f" > {BOUND}.",
        f"Analyzer view of the decision (P): {'ACCEPTED' if accepted else 'REJECTED'}; {fit_text(DECIDED, decided)};"
        f" upper95 {decided['upper95']:.4f} against {BOUND}.",
        f"SHA-256: {args.log.name} {sha256_of(log_bytes)}; {samples_path.name} {sha256_of(samples_text.encode())};"
        f" {exponents_path.name} {sha256_of(exponents_text.encode())}; {Path(__file__).name} {sha256_of(analyzer)}.",
    ]
    lines = [f"# {line}" for line in header] + table
    summary_path.write_text("\n".join(lines) + "\n")
    print(f"P {'accepted' if accepted else 'rejected'} by the analyzer: {fit_text(DECIDED, decided)}")
    print(f"red control: {fit_text(RED_CONTROL, red)}: above {RED_CONTROL_MIN_EXPONENT} and rejected")
    print(f"wrote {samples_path}, {exponents_path}, {summary_path}")
    return 0


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def self_test() -> int:
    generator = np.random.default_rng(SELF_TEST_SEED)
    depths = dict(GROUPS)[DECIDED]

    def series(sizes: tuple[int, ...], exponent: float, noise: float = 0.02) -> list[list[float]]:
        return [[float(value) for value in 100.0 * size**exponent * np.exp(generator.normal(0.0, noise, SAMPLES))]
                for size in sizes]

    linear = loglog_exponent(depths, series(depths, 1.0), "median")
    require(abs(linear["estimate"] - 1.0) < 0.02 and linear["upper95"] <= BOUND,
            f"a linear series is not accepted (estimate {linear['estimate']}, upper95 {linear['upper95']})")
    require(not red_control_holds(linear), "a linear series passes as the red control")
    steep = loglog_exponent(depths, series(depths, 1.3), "median")
    require(steep["upper95"] > BOUND, f"the rule accepts a series of exponent 1.3 (upper95 {steep['upper95']})")
    red_sizes = dict(GROUPS)[RED_CONTROL]
    quadratic = loglog_exponent(red_sizes, series(red_sizes, 2.0), "median")
    require(quadratic["upper95"] > BOUND, "the rule accepts the quadratic series")
    require(red_control_holds(quadratic), f"a quadratic series fails the red control (estimate {quadratic['estimate']})")
    moderate = loglog_exponent(red_sizes, series(red_sizes, 1.35), "median")
    require(moderate["upper95"] > BOUND and not red_control_holds(moderate),
            "a series of exponent 1.35 passes the red control although its exponent is not above 1.5")
    data = series(depths, 1.05, noise=0.05)
    reference = stratified_loglog_slope(list(depths), data, replicates=REPLICATES, seed=SEED)
    geometric = loglog_exponent(depths, data, "geometric")
    require(geometric["estimate"] == reference["estimate"] and geometric["upper95"] == reference["upper95"]
            and geometric["lower95"] == reference["lower95"],
            "the geometric variant differs from scripts/bench stratified_loglog_slope")
    median_fit = loglog_exponent(depths, data, "median")
    require(abs(median_fit["estimate"] - geometric["estimate"]) < 0.02
            and median_fit["lower95"] <= median_fit["upper95_one_sided"] <= median_fit["upper95"],
            "the median fit is far from the geometric fit, or its percentiles are out of order")
    require(loglog_exponent(depths, data, "median") == median_fit, "the bootstrap is not deterministic")
    try:
        loglog_exponent(depths, data, "median", replicates=REPLICATES - 1)
    except ValueError:
        pass
    else:
        raise AssertionError("fewer than 10,000 replicates are accepted")
    require(per_iteration([(1.0, 10.0), (2.0, 30.0)]) == [10.0, 15.0], "time per iteration must be time_ns / iters")
    require(number_text(3.0) == "3" and number_text(2.5) == "2.5", "integral values must print as integers")
    found = benchmarks()
    require(len(found) == 25 and found[0][3] == "accounting_demand_byte_pda/128",
            f"25 benchmarks expected, the first in accounting_demand_byte_pda/128 ({len(found)}, {found[0]})")
    print(
        f"self-test n=30: linear accepted (upper95 {linear['upper95']:.4f}); exponent 1.3 rejected (upper95"
        f" {steep['upper95']:.4f}); quadratic red control holds (exponent {quadratic['estimate']:.4f}, upper95"
        f" {quadratic['upper95']:.4f}); exponent 1.35 fails the red control; the geometric variant equals"
        " stratified_loglog_slope"
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

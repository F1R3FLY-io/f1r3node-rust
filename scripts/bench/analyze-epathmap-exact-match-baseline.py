"""Analyze the preregistered current-epoch exact EPathMap match latency baseline (task 14266).

Task 14266 is the successor of pgmcp hypothesis 233 (criterion 799 of rspace-ac-multiplicity-matcher), which no
longer exists. The harness rholang/benches/epathmap_ac_exact.rs is unchanged: groups epathmap_ac_exact_set and
epathmap_ac_exact_map, arm production, at 16, 256 and 4,096 entries (6 benchmarks), run once with --sample-size 33.
The decoded-matching control of 2026-08-16 is gone from the harness, so the hypothesis is absolute.

One observation is one Criterion sample: sample i ran iters[i] exact matches (iter_batched, fixture cloning
excluded) in times[i] nanoseconds, so its latency is mean_ns = times[i] / iters[i]. The primary input is the mean
latency per cell.

Hypothesis P: for sets and for maps, the 95% upper bound of the log-log exponent of mean exact-match latency over
the three cardinalities is at most 1.20. The exponent is the ordinary least-squares slope of log(mean latency of a
cell) against log(cardinality); its bounds come from a stratified bootstrap that resamples each cell's 33 samples
with replacement (10,000 replicates, seed 0x41315636, cells in increasing cardinality). This is the method of
scripts/bench/paired_benchmark_statistics.py stratified_loglog_slope, with the cell statistic the arithmetic mean
(the leaf's "mean latency") instead of the mean of the logarithms; upper95 is the 97.5th percentile of the
replicates, as stratified_loglog_slope names it (the upper end of the two-sided 95% interval), and the one-sided 95th
percentile is reported beside it. With the cardinalities equally spaced in log scale the OLS weight of the middle
cell is zero, so the slope is the end-point slope log(m_4096 / m_16) / log(256); the middle cell is reported.
pgmcp decides P with absolute_threshold {max, le, 1.20} over the two upper95 bounds (arm production).

Red control (the leaf's acceptance): the same fit on a synthetic quadratic series, latency = cardinality squared
(33 identical samples per cell), must be rejected: its upper95 is exactly 2.0 for sets and maps, recorded as arm
synthetic_quadratic. Reported beside it, deciding nothing: the fit on the window's own samples rescaled to a
quadratic series (each sample multiplied by cardinality^2 / the cell mean), which keeps the measured dispersion. If
either quadratic fit were accepted, the fit could not reject and the run is invalid (exit 2).

Citation: the 2026-08-16 means of the byte-native final treatment are read from
docs/design/stack-safety/measurements/epathmap-ac-byte-native-2026-08-16.tsv (--cite) and printed beside the new
means, with their end-point exponent; the summary records the cited file's SHA-256.

The analyzer rejects an incomplete run before computing anything (exit 2): each of the 6 benchmarks must hold 33
samples and reach "Analyzing" in the run log, and no panic may occur anywhere in the log.

Outputs: <stem>.samples.csv (group, arm, size, sample, iters, time_ns, mean_ns), <stem>.exponents.csv (hypothesis,
arm, series, role, estimate, lower95, upper95, upper95_one_sided) and the summary <stem>.tsv, whose "# " header
records the facts of the window, the analyzer's view of the decision and the SHA-256 of the log, both CSV files, the
cited record and this analyzer.

Exit status: 0 for a valid run, 2 for an invalid input.
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

from paired_benchmark_statistics import sample_summary, stratified_loglog_slope

SCHEMA = "epathmap-exact-match-current-epoch-v1"
SERIES = (("set", "epathmap_ac_exact_set"), ("map", "epathmap_ac_exact_map"))
ARM = "production"
QUADRATIC = "synthetic_quadratic"
SIZES = (16, 256, 4096)
SAMPLES = 33
BOUND = 1.20
SEED = 0x4131_5636
REPLICATES = 10_000
SELF_TEST_SEED = 0x4131_4266
CITED_ARM = "byte_native_final"
SAMPLE_COLUMNS = ("group", "arm", "size", "sample", "iters", "time_ns", "mean_ns")
EXPONENT_COLUMNS = ("hypothesis", "arm", "series", "role", "estimate", "lower95", "upper95", "upper95_one_sided")
SUMMARY_COLUMNS = (
    "series",
    "cell",
    "size",
    "n",
    "mean_ns",
    "median_ns",
    "standard_deviation_ns",
    "mean_2026_08_16_ns",
    "ratio_to_2026_08_16",
)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path, help="the window's CRITERION_HOME")
    parser.add_argument("--baseline", required=True)
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--stem", type=Path, required=True)
    parser.add_argument("--cite", type=Path, required=True, help="epathmap-ac-byte-native-2026-08-16.tsv")
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
    return [(group, ARM, size, f"{group}/{ARM}/{size}") for _, group in SERIES for size in SIZES]


def read_matrix(root: Path, baseline: str) -> dict[tuple[str, int], list[tuple[float, float]]]:
    return {
        (group, size): read_sample(root / identifier / baseline / "sample.json", SAMPLES)
        for group, _, size, identifier in benchmarks()
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


def read_cited(path: Path) -> dict[tuple[str, int], float]:
    """The 2026-08-16 raw means of the byte-native final treatment, by (set|map, cardinality)."""
    lines = [line for line in path.read_text().splitlines() if line and not line.startswith("#")]
    rows = list(csv.DictReader(io.StringIO("\n".join(lines)), delimiter="\t"))
    cited = {
        (row["mode"], int(row["cardinality"])): float(row["raw_mean_ns"]) for row in rows if row["arm"] == CITED_ARM
    }
    expected = {(series, size) for series, _ in SERIES for size in SIZES}
    if set(cited) != expected:
        raise ValueError(f"{path}: the {CITED_ARM} means of {sorted(expected)} are expected, found {sorted(cited)}")
    return cited


def per_iteration(pairs: list[tuple[float, float]]) -> list[float]:
    return [elapsed / count for count, elapsed in pairs]


def stratum_log_statistic(values: np.ndarray, statistic: str) -> np.ndarray:
    """log of the per-stratum statistic over the last axis; "geometric" is the mean of the logarithms."""
    if statistic == "mean":
        return np.log(values.mean(axis=-1))
    if statistic == "median":
        return np.log(np.median(values, axis=-1))
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


def sample_rows(matrix: dict[tuple[str, int], list[tuple[float, float]]]) -> str:
    buffer = io.StringIO()
    writer = csv.writer(buffer, lineterminator="\n")
    writer.writerow(SAMPLE_COLUMNS)
    for group, arm, size, _ in benchmarks():
        for index, (count, elapsed) in enumerate(matrix[(group, size)]):
            writer.writerow((group, arm, size, index, number_text(count), number_text(elapsed), repr(elapsed / count)))
    return buffer.getvalue()


def exponent_rows(fits: list[tuple[str, str, str, dict[str, float]]]) -> str:
    buffer = io.StringIO()
    writer = csv.writer(buffer, lineterminator="\n")
    writer.writerow(EXPONENT_COLUMNS)
    for arm, series, role, fit in fits:
        writer.writerow(("P", arm, series, role, repr(fit["estimate"]), repr(fit["lower95"]), repr(fit["upper95"]),
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


def analyze(args: argparse.Namespace) -> int:
    analyzer = Path(__file__).resolve().read_bytes()
    log_bytes = args.log.read_bytes()
    check_log(log_bytes.decode("utf-8", errors="replace"))
    cite_bytes = args.cite.read_bytes()
    cited = read_cited(args.cite)
    matrix = read_matrix(args.input, args.baseline)
    fits: list[tuple[str, str, str, dict[str, float]]] = []
    table = ["\t".join(SUMMARY_COLUMNS)]
    views: dict[str, dict[str, Any]] = {}
    for series, group in SERIES:
        latencies = [per_iteration(matrix[(group, size)]) for size in SIZES]
        production = loglog_exponent(SIZES, latencies, "mean")
        quadratic = loglog_exponent(SIZES, [[float(size) ** 2] * SAMPLES for size in SIZES], "mean")
        rescaled = loglog_exponent(
            SIZES,
            [[value * float(size) ** 2 / statistics.mean(cell) for value in cell] for size, cell in zip(SIZES, latencies)],
            "mean",
        )
        cited_exponent = math.log(cited[(series, SIZES[-1])] / cited[(series, SIZES[0])]) / math.log(
            SIZES[-1] / SIZES[0])
        fits += [(ARM, series, "decided", production), (QUADRATIC, series, "red_control", quadratic),
                 ("quadratic_window_noise", series, "context", rescaled)]
        views[series] = {"production": production, "quadratic": quadratic, "rescaled": rescaled,
                         "cited": cited_exponent}
        for size, cell in zip(SIZES, latencies):
            summary = sample_summary(cell)
            old = cited[(series, size)]
            table.append("\t".join((series, f"{group}/{ARM}/{size}", str(size), str(summary["n"]),
                                    f"{summary['mean']:.3f}", f"{summary['median']:.3f}",
                                    f"{summary['standard_deviation']:.3f}", f"{old:.4f}",
                                    f"{summary['mean'] / old:.6f}")))
    accepted_quadratic = [f"{arm} {series}" for arm, series, role, fit in fits
                          if role in ("red_control", "context") and fit["upper95"] <= BOUND]
    if accepted_quadratic:
        raise ValueError(f"the exponent rule accepts a quadratic series ({', '.join(accepted_quadratic)}), so it"
                         " cannot reject")
    samples_text = sample_rows(matrix)
    exponents_text = exponent_rows(fits)
    samples_path = Path(f"{args.stem}.samples.csv")
    exponents_path = Path(f"{args.stem}.exponents.csv")
    summary_path = Path(f"{args.stem}.tsv")
    samples_path.write_text(samples_text)
    exponents_path.write_text(exponents_text)
    worst_series = max(views, key=lambda name: views[name]["production"]["upper95"])
    worst = views[worst_series]["production"]["upper95"]
    accepted = worst <= BOUND

    def fit_text(series: str) -> str:
        fit = views[series]["production"]
        return (f"{series} exponent {fit['estimate']:.4f}, 95% interval [{fit['lower95']:.4f}, {fit['upper95']:.4f}],"
                f" one-sided 95th percentile {fit['upper95_one_sided']:.4f}")

    header = [
        "Exact EPathMap match latency, current epoch: the successor of pgmcp hypothesis 233 (task 14266).",
        f"Schema {SCHEMA}; analyzer sha256:{sha256_of(analyzer)}; Criterion baseline {args.baseline}.",
        *read_facts(args.facts),
        "Statistics: one observation is one Criterion sample, mean_ns = time_ns / iters (one exact match per"
        f" iteration, fixture cloning excluded); P: for sets and for maps, the 95% upper bound (97.5th percentile of"
        f" a stratified bootstrap, {REPLICATES} replicates, seed {SEED:#x}) of the OLS slope of log(mean latency) on"
        f" log(cardinality) over {', '.join(str(size) for size in SIZES)} is at most {BOUND}, {SAMPLES} samples per"
        " cell; the middle cell has zero OLS weight (log-equally spaced cardinalities).",
        f"Software: Python {platform.python_version()}, NumPy {np.__version__}, SciPy {scipy.__version__}.",
        f"Validity: all 6 benchmarks hold {SAMPLES} samples, reach Analyzing in the log and no panic occurs; red"
        " control (the same fit on a synthetic quadratic series, latency = cardinality squared): rejected, upper95"
        f" {views['set']['quadratic']['upper95']:.4f} (set) and {views['map']['quadratic']['upper95']:.4f} (map);"
        " the window's samples rescaled to a quadratic series are rejected too, upper95"
        f" {views['set']['rescaled']['upper95']:.4f} (set) and {views['map']['rescaled']['upper95']:.4f} (map).",
        f"Citation: the 2026-08-16 means of {CITED_ARM} from {args.cite.name} (sha256:{sha256_of(cite_bytes)}) are"
        " the mean_2026_08_16_ns column; their end-point exponents are"
        f" {views['set']['cited']:.4f} (set) and {views['map']['cited']:.4f} (map); that run took 82 samples per cell.",
        f"Analyzer view of the decision (P): {'ACCEPTED' if accepted else 'REJECTED'}; {fit_text('set')};"
        f" {fit_text('map')}; the larger upper95, {worst:.4f} ({worst_series}), against {BOUND}.",
        f"SHA-256: {args.log.name} {sha256_of(log_bytes)}; {samples_path.name} {sha256_of(samples_text.encode())};"
        f" {exponents_path.name} {sha256_of(exponents_text.encode())}; {args.cite.name} {sha256_of(cite_bytes)};"
        f" {Path(__file__).name} {sha256_of(analyzer)}.",
    ]
    lines = [f"# {line}" for line in header] + table
    summary_path.write_text("\n".join(lines) + "\n")
    print(f"P {'accepted' if accepted else 'rejected'} by the analyzer: {fit_text('set')}; {fit_text('map')}")
    print(f"red control: quadratic upper95 {views['set']['quadratic']['upper95']:.4f} and"
          f" {views['map']['quadratic']['upper95']:.4f}, rejected")
    print(f"wrote {samples_path}, {exponents_path}, {summary_path}")
    return 0


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def self_test() -> int:
    generator = np.random.default_rng(SELF_TEST_SEED)

    def series(exponent: float, scale: float = 200.0, noise: float = 0.02) -> list[list[float]]:
        return [[float(value) for value in scale * size**exponent * np.exp(generator.normal(0.0, noise, SAMPLES))]
                for size in SIZES]

    linear = loglog_exponent(SIZES, series(1.0), "mean")
    require(abs(linear["estimate"] - 1.0) < 0.02 and linear["upper95"] <= BOUND,
            f"a linear series is not accepted (estimate {linear['estimate']}, upper95 {linear['upper95']})")
    sublinear = loglog_exponent(SIZES, series(0.9), "mean")
    require(sublinear["upper95"] <= BOUND, "a series of exponent 0.9 is not accepted")
    quadratic = loglog_exponent(SIZES, [[float(size) ** 2] * SAMPLES for size in SIZES], "mean")
    require(abs(quadratic["estimate"] - quadratic["upper95"]) < 1e-12 and abs(quadratic["upper95"] - 2.0) < 1e-12,
            f"the noise-free quadratic series does not give exponent 2 (upper95 {quadratic['upper95']})")
    require(quadratic["upper95"] > BOUND, "the rule accepts the quadratic series")
    steep = loglog_exponent(SIZES, series(1.3), "mean")
    require(steep["upper95"] > BOUND, f"the rule accepts a series of exponent 1.3 (upper95 {steep['upper95']})")
    data = series(1.1, noise=0.05)
    reference = stratified_loglog_slope(list(SIZES), data, replicates=REPLICATES, seed=SEED)
    geometric = loglog_exponent(SIZES, data, "geometric")
    require(geometric["estimate"] == reference["estimate"] and geometric["upper95"] == reference["upper95"]
            and geometric["lower95"] == reference["lower95"],
            "the geometric variant differs from scripts/bench stratified_loglog_slope")
    mean_fit = loglog_exponent(SIZES, data, "mean")
    require(abs(mean_fit["estimate"] - geometric["estimate"]) < 0.01 and mean_fit["upper95_one_sided"] <= mean_fit["upper95"],
            "the mean fit is far from the geometric fit, or its one-sided percentile exceeds upper95")
    endpoint = math.log(statistics.mean(data[2]) / statistics.mean(data[0])) / math.log(SIZES[2] / SIZES[0])
    require(abs(mean_fit["estimate"] - endpoint) < 1e-12, "with log-equal spacing the OLS slope is not the end-point slope")
    require(loglog_exponent(SIZES, data, "mean") == mean_fit, "the bootstrap is not deterministic")
    try:
        loglog_exponent(SIZES, data, "mean", replicates=REPLICATES - 1)
    except ValueError:
        pass
    else:
        raise AssertionError("fewer than 10,000 replicates are accepted")
    require(per_iteration([(1.0, 10.0), (2.0, 30.0)]) == [10.0, 15.0], "mean_ns must be time_ns / iters")
    require(len(benchmarks()) == 6, f"{len(benchmarks())} benchmarks, 6 expected")
    print(
        f"self-test n=33: linear accepted (upper95 {linear['upper95']:.4f}); exponent 0.9 accepted; quadratic"
        f" rejected (upper95 {quadratic['upper95']:.4f}); exponent 1.3 rejected (upper95 {steep['upper95']:.4f});"
        " the geometric variant equals stratified_loglog_slope; the OLS slope is the end-point slope"
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

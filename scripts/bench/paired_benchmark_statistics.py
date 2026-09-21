"""Shared paired statistics for campaign 4131 benchmark analyzers."""

from __future__ import annotations

import statistics
from typing import Any

import numpy as np
from scipy.stats import shapiro, ttest_rel, wilcoxon


def stratified_loglog_slope(depths, samples, replicates=10_000, seed=5916):
    if len(depths) < 3 or len(depths) != len(samples):
        raise ValueError("at least three aligned depth strata are required")
    if len(set(depths)) != len(depths) or any(depth <= 0 for depth in depths):
        raise ValueError("depths must be positive and distinct")
    if replicates < 10_000:
        raise ValueError("at least 10,000 bootstrap replicates are required")
    widths = {len(group) for group in samples}
    if len(widths) != 1 or next(iter(widths)) < 3:
        raise ValueError("strata require equal sample sizes of at least three")
    values = np.asarray(samples, dtype=np.float64)
    if not np.all(np.isfinite(values)) or np.any(values <= 0):
        raise ValueError("observations must be finite and positive")
    x = np.log(np.asarray(depths, dtype=np.float64))
    centered = x - np.mean(x)
    denominator = np.sum(centered**2)
    logs = np.log(values)
    estimate = np.dot(centered, logs.mean(axis=1)) / denominator
    generator = np.random.default_rng(seed)
    boot = np.zeros(replicates)
    for index, group in enumerate(logs):
        draws = generator.integers(0, len(group), size=(replicates, len(group)))
        boot += centered[index] * group[draws].mean(axis=1) / denominator
    low, high = np.quantile(boot, [0.025, 0.975])
    return {
        "estimate": float(estimate),
        "lower95": float(low),
        "upper95": float(high),
        "replicates": replicates,
        "seed": seed,
    }


def sample_summary(values: list[float]) -> dict[str, float | int]:
    if len(values) < 2:
        raise ValueError("sample summaries require at least two observations")
    return {
        "n": len(values),
        "mean": statistics.mean(values),
        "median": statistics.median(values),
        "minimum": min(values),
        "maximum": max(values),
        "standard_deviation": statistics.stdev(values),
    }


def paired_bootstrap_ratio_of_medians(
    controls: list[float],
    treatments: list[float],
    replicates: int,
    seed: int,
) -> dict[str, float]:
    """Bootstrap the ratio of treatment/control medians over intact pairs."""
    _validate_pairs(controls, treatments, replicates)
    generator = np.random.default_rng(seed)
    indices = generator.integers(0, len(controls), size=(replicates, len(controls)))
    control = np.asarray(controls, dtype=np.float64)
    treatment = np.asarray(treatments, dtype=np.float64)
    ratios = np.median(treatment[indices], axis=1) / np.median(control[indices], axis=1)
    return _ratio_interval(ratios)


def paired_bootstrap_median_of_ratios(
    controls: list[float],
    treatments: list[float],
    replicates: int,
    seed: int,
) -> dict[str, float]:
    """Bootstrap the median of within-pair treatment/control ratios."""
    _validate_pairs(controls, treatments, replicates)
    pair_ratios = np.asarray(treatments, dtype=np.float64) / np.asarray(
        controls, dtype=np.float64
    )
    generator = np.random.default_rng(seed)
    indices = generator.integers(
        0, len(pair_ratios), size=(replicates, len(pair_ratios))
    )
    ratios = np.median(pair_ratios[indices], axis=1)
    return _ratio_interval(ratios)


def _validate_pairs(
    controls: list[float], treatments: list[float], replicates: int
) -> None:
    if len(controls) != len(treatments):
        raise ValueError("paired arms have different observation counts")
    if len(controls) < 3:
        raise ValueError("paired statistics require at least three observations")
    if replicates < 10_000:
        raise ValueError("at least 10,000 bootstrap replicates are required")
    if any(value <= 0 for value in controls):
        raise ValueError("ratio controls must be positive")


def _ratio_interval(ratios: np.ndarray[Any, np.dtype[np.float64]]) -> dict[str, float]:
    low, median, high = np.quantile(ratios, (0.005, 0.5, 0.995))
    return {
        "lower_0_5_percent": float(low),
        "median": float(median),
        "upper_99_5_percent": float(high),
    }


def paired_statistics(
    control: list[float],
    treatment: list[float],
    replicates: int,
    seed: int,
) -> dict[str, Any]:
    """Return descriptive, paired-test, and two paired-bootstrap views."""
    _validate_pairs(control, treatment, replicates)
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
    pair_ratios = [b / a for a, b in zip(control, treatment)]
    return {
        "control": sample_summary(control),
        "treatment": sample_summary(treatment),
        "paired_difference": sample_summary(differences),
        "paired_ratio": sample_summary(pair_ratios),
        "mean_treatment_control_ratio": statistics.mean(treatment)
        / statistics.mean(control),
        "median_treatment_control_ratio": statistics.median(treatment)
        / statistics.median(control),
        "paired_bootstrap_ratio_of_medians_99_percent": (
            paired_bootstrap_ratio_of_medians(control, treatment, replicates, seed)
        ),
        "paired_bootstrap_median_of_ratios_99_percent": (
            paired_bootstrap_median_of_ratios(
                control, treatment, replicates, seed ^ 0x5A5A_4131
            )
        ),
        "paired_t_two_sided_pvalue": paired_t_pvalue,
        "paired_wilcoxon_two_sided_pvalue": signed_rank_pvalue,
        "shapiro_pvalue_paired_difference": shapiro_pvalue,
    }


def benjamini_hochberg(pvalues: dict[str, float]) -> dict[str, float]:
    """Adjust a named family of p-values with Benjamini–Hochberg FDR."""
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

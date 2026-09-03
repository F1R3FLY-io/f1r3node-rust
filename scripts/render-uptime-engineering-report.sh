#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INPUT="${1:-$ROOT/target/verification/uptime/storm/engineering-envelope.json}"
OUTPUT="${2:-$ROOT/target/verification/uptime/engineering-envelope.md}"

command -v jq >/dev/null 2>&1 || {
  echo "error: jq is required to render the uptime report" >&2
  exit 1
}
test -s "$INPUT" || {
  echo "error: missing uptime engineering envelope: $INPUT" >&2
  exit 1
}
mkdir -p "$(dirname "$OUTPUT")"

jq -r '
  def shown:
    if type == "number" then
      if . == 0 then "0"
      elif (. < 0.000001 or . >= 1000000) then @text
      else ((. * 1000000000 | round) / 1000000000 | tostring)
      end
    else tostring
    end;
  def pct: (. * 100 | shown) + "%";
  def scenario_row:
    "| \(.case) | \(.expected_lifetime_hours | shown) | \(.month_uninterrupted_survival_probability | pct) | \(.expected_month_down_hours | shown) | \(.month_quorum_loss_probability | pct) | \(.month_common_failure_probability | pct) | \(.month_storage_unavailability_probability | pct) | \(.month_memory_cap_probability | pct) | \(.month_resource_exhaustion_probability | pct) |";
  def first_cause($cause):
    ($cause.probability | pct) + " / " + ($cause.conditional_share | pct);
  def first_loss_row:
    "| \(.case) | \(.first_service_loss.within_horizon_probability | pct) | \(first_cause(.first_service_loss.causes.quorum)) | \(first_cause(.first_service_loss.causes.common)) | \(first_cause(.first_service_loss.causes.storage)) | \(first_cause(.first_service_loss.causes.queue)) | \(first_cause(.first_service_loss.causes.lag)) | \(first_cause(.first_service_loss.causes.memory)) | \(.first_service_loss.partition_residual | shown) |";
  def sensitivity_row:
    "| `\(.parameter)` | \(.values.adverse | shown) / \(.values.central | shown) / \(.values.favorable | shown) | \(.expected_lifetime_hours.adverse | shown) / \(.expected_lifetime_hours.central | shown) / \(.expected_lifetime_hours.favorable | shown) | \(.month_survival_probability.adverse | pct) / \(.month_survival_probability.central | pct) / \(.month_survival_probability.favorable | pct) | \(.ranking_score | pct) |";
  [
    "# Thirty-day shard uptime engineering envelope",
    "",
    "> Status: **not release-certified**. These figures are reportable only together with the parameter table and exclusions below.",
    "",
    "## Evidence identity",
    "",
    "| Field | Value |",
    "| --- | --- |",
    "| Git commit | `\(.provenance.git_commit)` |",
    "| Relevant implementation dirty | `\(.provenance.implementation_dirty)` |",
    "| Formal authorities dirty | `\(.provenance.formal_authority_dirty)` |",
    "| Implementation hash | `\(.provenance.implementation_hash)` |",
    "| Model hash | `\(.provenance.model_hash)` |",
    "| Formal-authority hash | `\(.provenance.formal_authority_hash)` |",
    "| Profile hash | `\(.provenance.profile_hash)` |",
    "| Generated UTC | \(.generated_at) |",
    "| Horizon | \(.horizon_hours) hours |",
    "",
    "## Results",
    "",
    "The expected case is the central declared assumption, not a fitted population mean. Expected lifetime is mean time to first loss of the complete service predicate. Month survival means no service loss at any instant during 720 hours.",
    "",
    "| Case | Expected lifetime (h) | 30-day uninterrupted survival | Expected down time in 30 days (h) | Quorum loss | Common failure | Storage unavailable | Memory guard | Any bounded resource |",
    "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    (.scenarios[] | scenario_row),
    "",
    "## First service-loss attribution",
    "",
    "Each cause cell is `unconditional probability / conditional share of first service loss`. The six initiating causes are mutually exclusive in the CTMC and, together with uninterrupted survival, must partition probability one. Component-occurrence columns in the preceding table can overlap and must not be added.",
    "",
    "| Case | Any first loss | Quorum | Common cause | Storage | Queue cap | Lag SLO | Memory guard | Partition residual |",
    "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    (.scenarios[] | first_loss_row),
    "",
    "## One-at-a-time parameter sensitivity",
    "",
    "Each row changes only the named assumption from the central profile to its declared adverse or favorable endpoint. The table reports `adverse / central / favorable`. Ranking is the larger absolute fractional change in expected first-loss lifetime. Repair rates can therefore have zero influence here because first-loss lifetime and uninterrupted survival stop at the initial outage; repair rates still affect expected down-hours and steady-state availability. This is deterministic sensitivity of the CTMC, not a fitted regression and not a substitute for joint-interaction or calibrated uncertainty analysis.",
    "",
    "| Parameter | Values | Expected lifetime (h) | 30-day survival | Maximum lifetime shift |",
    "| --- | --- | ---: | ---: | ---: |",
    (.parameter_sensitivity[] | sensitivity_row),
    "",
    "## Parameters",
    "",
    "| Parameter | Worst | Expected | Best | Unit | Provenance | Source or assumption |",
    "| --- | ---: | ---: | ---: | --- | --- | --- |",
    (.assumptions.parameters | to_entries[] |
      "| `\(.key)` | \(.value.worst | shown) | \(.value.expected | shown) | \(.value.best | shown) | \(.value.unit) | `\(.value.provenance)` | \(.value.source) |"),
    "",
    "## Historical backtest",
    "",
    "The exact historical topology and workflow configuration were reconstructed for Actions run `\(.historical_backtest.manifest.github_run_id)`. A Laplace-smoothed failure probability from the nine preceding published iterations predicts \(.historical_backtest.expected_failures | shown) failures in the held-out 19-iteration run; 18 were observed. The probability of 18 or more failures was \(.historical_backtest.observed_or_worse_probability | pct).",
    "",
    "This reproduces aggregate pre-repair harness behavior only. Every iteration created a fresh shard, the detailed artifact expired, and failure classes are unavailable; therefore continuous uptime rates are **not** calibrated by this backtest.",
    "",
    "The independent RSS-guard reconstruction covers \(.rss_guard_backtest.manifest.records | length) published runs with reported aggregate RSS peaks. All \(.rss_guard_backtest.direct_crossings) peaks at or above their revision-specific guard coincided with a protection breach. \(.rss_guard_backtest.below_cap_breaches) additional breaches occurred below that guard, consistent with the separate host-free-memory and non-RSS protections. This validates the guard classification, not a continuous memory-growth rate.",
    "",
    "## Historical context",
    "",
    "- Published daily records: \(.assumptions.historical_context.daily_records)",
    "- Published weekend records: \(.assumptions.historical_context.weekend_records)",
    "- Aggregate elapsed time: \(.assumptions.historical_context.total_elapsed_hours | shown) hours",
    "- Passive iterations: \(.assumptions.historical_context.passive_iterations), of which \(.assumptions.historical_context.passive_failures) failed",
    "- Host-protection breaches: \(.assumptions.historical_context.protection_breaches)",
    "- Reported aggregate RSS peak range where available: \(.assumptions.historical_context.reported_rss_peak_range_mb[0])–\(.assumptions.historical_context.reported_rss_peak_range_mb[1]) MiB",
    "- Applicability: \(.assumptions.historical_context.applicability)",
    "",
    "## Exclusions and invalidation rules",
    "",
    (.assumptions.exclusions[] | "- " + .),
    "",
    "Any change to the bound implementation files, model, or profile changes a hash above and invalidates copied results. Release projection additionally requires a clean tree and a current calibrated profile."
  ] | .[]
' "$INPUT" >"$OUTPUT"

echo "Rendered uptime engineering report: $OUTPUT"

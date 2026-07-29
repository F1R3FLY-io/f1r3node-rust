#!/usr/bin/env bash
# Aggregate a weekend-soak run into the weekly benchmark record, compute the
# week-over-week regression verdict, and render the markdown report.
#
# Inputs (a soak output directory):
#   summary.json               passive per-iteration metrics rollup (TASK-010-1/2)
#   bench-segment-*/metrics.json  active controlled-rate segments
#
# Outputs (OUT_DIR):
#   weekly-summary.json   the record appended to the dashboard data history
#   verdict.json          pass/regress + per-metric deltas (release gate input)
#   perf-report.md        human summary (step summary / artifact)
#
# Verdict policy (thresholds file, maintainer-approved EPOCH-010):
#   passive metrics FAIL the run: failure rate (+pts), peak RSS (+%),
#   finalization p95 (+%), iteration throughput (-%). Active-segment
#   metrics WARN. A run with no passive summary.json FAILS (no data is
#   never a silent pass). First run (no baseline) passes as bootstrap.
#
# Environment:
#   SOAK_DIR (required)   soak output directory
#   OUT_DIR (required)    report output directory
#   THRESHOLDS_JSON       gate config (default: sibling soak-gate-thresholds.json)
#   BASELINE_JSON         previous weekly-summary.json (absent on first run)
#   SOAK_STATUS           complete (default) | in_progress
#   RUN_ID, DURATION_SECONDS, DASHBOARD_URL   metadata
#
# in_progress is for a mid-run checkpoint. It publishes what has happened so
# far but computes no verdict: a partial run has fewer iterations, a lower
# peak RSS and a throughput figure over a shorter window than the baseline it
# would be compared against, so a regression verdict at that point would be
# measuring the clock rather than the code.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOAK_DIR="${SOAK_DIR:?SOAK_DIR is required}"
OUT_DIR="${OUT_DIR:?OUT_DIR is required}"
THRESHOLDS_JSON="${THRESHOLDS_JSON:-$SCRIPT_DIR/soak-gate-thresholds.json}"
BASELINE_JSON="${BASELINE_JSON:-}"
SOAK_STATUS="${SOAK_STATUS:-complete}"
case "$SOAK_STATUS" in
  complete|in_progress) ;;
  *) echo "SOAK_STATUS must be 'complete' or 'in_progress'" >&2; exit 2 ;;
esac
# Belt and braces: the verdict is overridden for a checkpoint anyway, but
# dropping the baseline here means none of the comparison branches can fire
# even if that override is later changed.
[ "$SOAK_STATUS" = "in_progress" ] && BASELINE_JSON=""
RUN_ID="${RUN_ID:-unknown}"
DURATION_SECONDS="${DURATION_SECONDS:-0}"
DASHBOARD_URL="${DASHBOARD_URL:-https://f1r3fly-io.github.io/f1r3node-rust/}"

command -v jq >/dev/null || { echo "jq not found" >&2; exit 2; }
mkdir -p "$OUT_DIR"

SEGMENTS_JSON="$OUT_DIR/.segments.json"
find "$SOAK_DIR" -path '*bench-segment-*/metrics.json' -print0 \
  | sort -z \
  | xargs -0 --no-run-if-empty cat \
  | jq -s 'sort_by(.segment_index)' > "$SEGMENTS_JSON"
[ -s "$SEGMENTS_JSON" ] || echo '[]' > "$SEGMENTS_JSON"

PASSIVE_ARG='null'
if [ -s "$SOAK_DIR/summary.json" ]; then
  PASSIVE_ARG="$(cat "$SOAK_DIR/summary.json")"
fi

BASELINE_ARG='null'
if [ -n "$BASELINE_JSON" ] && [ -s "$BASELINE_JSON" ]; then
  BASELINE_ARG="$(cat "$BASELINE_JSON")"
fi

jq -n \
  --slurpfile segments "$SEGMENTS_JSON" \
  --argjson passive "$PASSIVE_ARG" \
  --arg run_id "$RUN_ID" \
  --argjson duration "$DURATION_SECONDS" \
  --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg status "$SOAK_STATUS" \
'
  def median: sort | if length == 0 then null else .[(length - 1) / 2 | floor] end;
  ($segments[0]) as $segs
  | ($segs | map(select(.ok == true))) as $ok
  | {
      run: {
        date: $date,
        run_id: $run_id,
        target_ref: ($passive.target_ref // "unknown"),
        target_sha: ($passive.target_sha // "unknown"),
        duration_seconds: $duration,
        status: $status,
        # Seconds actually soaked so far, against the run
        # budget — lets the dashboard show progress on a checkpoint.
        elapsed_seconds: ($passive.elapsed_seconds // null)
      },
      passive: (if $passive == null then null else {
        iterations: $passive.iterations,
        failures: $passive.failures,
        failure_rate: $passive.failure_rate,
        iterations_per_hour: $passive.iterations_per_hour,
        rss_peak_mb: $passive.rss_peak_mb,
        finalization_p95_ms: $passive.finalization_p95_ms,
        providers: $passive.providers
      } end),
      active: {
        segments_total: ($segs | length),
        segments_ok: ($ok | length),
        p50_ms: ($ok | map(.latency.p50_ms) | median),
        p95_ms: ($ok | map(.latency.p95_ms) | median),
        throughput: ($ok | map(.observed_throughput) | median),
        finalization_rate: ($ok | map(.finalization_rate) | median),
        rss_peak_mb: ($ok | map(.rss_peak_mb) | median),
        segments: $segs
      }
    }
' > "$OUT_DIR/weekly-summary.json"

jq -n \
  --argjson current "$(cat "$OUT_DIR/weekly-summary.json")" \
  --argjson baseline "$BASELINE_ARG" \
  --argjson thresholds "$(cat "$THRESHOLDS_JSON")" \
  --arg status "$SOAK_STATUS" \
'
  def pct_over(cur; base; pct):
    (cur != null and base != null and base > 0 and cur > (base * (1 + pct)));
  def pct_under(cur; base; pct):
    (cur != null and base != null and base > 0 and cur < (base * (1 - pct)));

  ($current.passive) as $p
  | ($baseline.passive // null) as $bp
  | ($current.active) as $a
  | ($baseline.active // null) as $ba
  | (
      []
      | if $p == null
          then . + ["no passive soak summary was produced (no data)"] else . end
      | if $bp != null and $p != null and $p.failure_rate != null and $bp.failure_rate != null
           and $p.failure_rate > ($bp.failure_rate + $thresholds.failure_rate_max_increase_pts)
          then . + ["failure rate \($p.failure_rate) exceeds baseline \($bp.failure_rate) by more than \($thresholds.failure_rate_max_increase_pts * 100)pts"] else . end
      | if $bp != null and $p != null and pct_over($p.rss_peak_mb; $bp.rss_peak_mb; $thresholds.rss_peak_max_increase_pct)
          then . + ["peak RSS \($p.rss_peak_mb)MB > baseline \($bp.rss_peak_mb)MB +\($thresholds.rss_peak_max_increase_pct * 100)%"] else . end
      | if $bp != null and $p != null and pct_over($p.finalization_p95_ms; $bp.finalization_p95_ms; $thresholds.finalization_p95_max_increase_pct)
          then . + ["finalization p95 \($p.finalization_p95_ms)ms > baseline \($bp.finalization_p95_ms)ms +\($thresholds.finalization_p95_max_increase_pct * 100)%"] else . end
      | if $bp != null and $p != null and pct_under($p.iterations_per_hour; $bp.iterations_per_hour; $thresholds.throughput_max_decrease_pct)
          then . + ["iteration throughput \($p.iterations_per_hour)/h < baseline \($bp.iterations_per_hour)/h -\($thresholds.throughput_max_decrease_pct * 100)%"] else . end
    ) as $failures
  | (
      []
      | if ($a.segments_total > 0) and ($a.segments_ok == 0)
          then . + ["all active benchmark segments failed"] else . end
      | if $ba != null and pct_over($a.p95_ms; $ba.p95_ms; $thresholds.active_p95_warn_increase_pct)
          then . + ["active-segment p95 \($a.p95_ms)ms > baseline \($ba.p95_ms)ms +\($thresholds.active_p95_warn_increase_pct * 100)%"] else . end
      | if $ba != null and pct_under($a.throughput; $ba.throughput; $thresholds.active_throughput_warn_decrease_pct)
          then . + ["active-segment throughput \($a.throughput)/s < baseline \($ba.throughput)/s -\($thresholds.active_throughput_warn_decrease_pct * 100)%"] else . end
    ) as $warnings
  | {
      # A checkpoint reports progress, never a judgement. Failures and
      # warnings are dropped rather than shown, because the only ones that
      # could fire mid-run are "no data yet" artefacts of an incomplete run,
      # and surfacing those would put a red strip on a healthy soak.
      verdict: (if $status == "in_progress" then "in_progress"
                elif ($failures | length) > 0 then "regress" else "pass" end),
      status: $status,
      bootstrap: ($status != "in_progress" and $baseline == null),
      failures: (if $status == "in_progress" then [] else $failures end),
      warnings: (if $status == "in_progress" then [] else $warnings end),
      thresholds: $thresholds,
      run: $current.run,
      baseline_run: ($baseline.run // null)
    }
' > "$OUT_DIR/verdict.json"

jq -r \
  --argjson verdict "$(cat "$OUT_DIR/verdict.json")" \
  --argjson baseline "$BASELINE_ARG" \
  --arg dashboard "$DASHBOARD_URL" \
'
  def fmt: if . == null then "-" else tostring end;
  ($baseline.passive // {}) as $bp
  | ($baseline.active // {}) as $ba
  | "# Weekend Soak Benchmark Report",
  "",
  "- **Date:** \(.run.date)",
  "- **Target:** `\(.run.target_ref)` @ `\(.run.target_sha[0:12])`",
  "- **Run:** \(.run.run_id), \(.run.duration_seconds)s soak",
  "- **Baseline:** \($verdict.baseline_run.date // "none (bootstrap)")",
  "- **Dashboard:** \($dashboard)",
  "",
  "## Verdict: \($verdict.verdict | ascii_upcase)",
  "",
  ($verdict.failures[] | "- FAIL: \(.)"),
  ($verdict.warnings[] | "- WARN: \(.)"),
  "",
  "## Passive soak metrics (whole run)",
  "",
  "| metric | this week | last week |",
  "|---|---|---|",
  "| iterations | \(.passive.iterations // null | fmt) | \($bp.iterations | fmt) |",
  "| failure rate | \(.passive.failure_rate // null | fmt) | \($bp.failure_rate | fmt) |",
  "| iterations/hour | \(.passive.iterations_per_hour // null | fmt) | \($bp.iterations_per_hour | fmt) |",
  "| peak RSS (MB) | \(.passive.rss_peak_mb // null | fmt) | \($bp.rss_peak_mb | fmt) |",
  "| finalization p95 (ms) | \(.passive.finalization_p95_ms // null | fmt) | \($bp.finalization_p95_ms | fmt) |",
  "",
  "## Active benchmark segments (controlled-rate, medians)",
  "",
  "| metric | this week | last week |",
  "|---|---|---|",
  "| segments ok | \(.active.segments_ok)/\(.active.segments_total) | \($ba.segments_ok | fmt)/\($ba.segments_total | fmt) |",
  "| p50 latency (ms) | \(.active.p50_ms | fmt) | \($ba.p50_ms | fmt) |",
  "| p95 latency (ms) | \(.active.p95_ms | fmt) | \($ba.p95_ms | fmt) |",
  "| throughput (deploys/s) | \(.active.throughput | fmt) | \($ba.throughput | fmt) |",
  "| finalization rate | \(.active.finalization_rate | fmt) | \($ba.finalization_rate | fmt) |",
  "| peak RSS (MB) | \(.active.rss_peak_mb | fmt) | \($ba.rss_peak_mb | fmt) |",
  "",
  "## Segment progression (performance vs. soak depth)",
  "",
  "| # | offset (h) | p50 ms | p95 ms | tput/s | finalized | RSS MB | ok |",
  "|---|---|---|---|---|---|---|---|",
  (.active.segments[] |
    "| \(.segment_index) | \((.offset_seconds / 3600 * 10 | floor) / 10) | \(.latency.p50_ms // null | fmt) | \(.latency.p95_ms // null | fmt) | \(.observed_throughput // null | fmt) | \(.finalized // 0)/\(.submitted // 0) | \(.rss_peak_mb | fmt) | \(.ok) |")
' "$OUT_DIR/weekly-summary.json" > "$OUT_DIR/perf-report.md"

rm -f "$SEGMENTS_JSON"
echo "wrote weekly-summary.json, verdict.json, perf-report.md to $OUT_DIR" >&2
jq -r '"verdict: \(.verdict)" + (if .failures | length > 0 then " — " + (.failures | join("; ")) else "" end)' \
  "$OUT_DIR/verdict.json" >&2

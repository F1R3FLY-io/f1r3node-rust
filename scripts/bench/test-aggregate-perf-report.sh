#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/failed" "$TMP/failed-report"
cat >"$TMP/failed/summary.json" <<'JSON'
{
  "target_ref": "dev",
  "target_sha": "0123456789abcdef",
  "version": "0.4.43",
  "started_at": 1000,
  "finished_at": 1100,
  "elapsed_seconds": 100,
  "iterations": 1,
  "failures": 1,
  "failure_rate": 1,
  "iterations_per_hour": 36,
  "rss_peak_mb": 17100,
  "cpu_peak_pct": 100,
  "cpu_peak_core_grid_pct": {"validator1": {"all": 55.5}, "bootstrap": {"all": 12}},
  "finalization_p50_ms": null,
  "finalization_p95_ms": null,
  "finalization_p99_ms": null,
  "too_far_ahead_errors": 0,
  "providers": {
    "docker": {"iterations": 1, "failures": 1, "avg_duration_s": 100},
    "subprocess": {"iterations": 0, "failures": 0, "avg_duration_s": null}
  },
  "tracked_metrics": {}
}
JSON
SOAK_DIR="$TMP/failed" OUT_DIR="$TMP/failed-report" RUN_ID=1 RUN_ATTEMPT=1 \
	SOAK_KIND=daily DURATION_SECONDS=1800 WINDOW_SECONDS=79200 RETRY_ATTEMPT=1 \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '
  .verdict == "regress"
  and .bootstrap == true
  and (.failures | any(test("1 passive soak iteration\\(s\\) failed")))
  and .run.shard_up_seconds == null
' "$TMP/failed-report/verdict.json" >/dev/null

mkdir -p "$TMP/checkpoint-report"
SOAK_DIR="$TMP/failed" OUT_DIR="$TMP/checkpoint-report" RUN_ID=1 RUN_ATTEMPT=1 \
	SOAK_KIND=daily DURATION_SECONDS=1800 WINDOW_SECONDS=79200 RETRY_ATTEMPT=1 \
	SOAK_STATUS=in_progress \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '.verdict == "in_progress" and .failures == []' \
	"$TMP/checkpoint-report/verdict.json" >/dev/null

mkdir -p "$TMP/passing" "$TMP/passing-report"
jq '.failures = 0 | .failure_rate = 0 | .providers.docker.failures = 0
	| .shard_up_seconds = 90' \
	"$TMP/failed/summary.json" >"$TMP/passing/summary.json"
SOAK_DIR="$TMP/passing" OUT_DIR="$TMP/passing-report" RUN_ID=2 RUN_ATTEMPT=1 \
	SOAK_KIND=daily DURATION_SECONDS=1800 WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '.verdict == "pass" and .bootstrap == true and .failures == []
	and .run.shard_up_seconds == 90' \
	"$TMP/passing-report/verdict.json" >/dev/null

mkdir -p "$TMP/segments/bench-segment-00001/bench" "$TMP/segments-report"
cp "$TMP/passing/summary.json" "$TMP/segments/summary.json"
cat >"$TMP/segments/bench-segment-00001/metrics.json" <<'JSON'
{
  "segment_index": 1,
  "offset_seconds": 60,
  "ok": true,
  "latency": {"p50_ms": 10, "p95_ms": 20},
  "observed_throughput": 2,
  "finalization_rate": 1,
  "rss_peak_mb": 100
}
JSON
cat >"$TMP/segments/bench-segment-00001/bench/metrics.json" <<'JSON'
{"segment_index": null, "offset_seconds": null, "ok": true}
JSON
SOAK_DIR="$TMP/segments" OUT_DIR="$TMP/segments-report" RUN_ID=4 RUN_ATTEMPT=1 \
	SOAK_KIND=daily DURATION_SECONDS=1800 WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '
  .active.segments_total == 1
  and .active.segments_ok == 1
  and (.active.segments | length) == 1
  and .active.segments[0].offset_seconds == 60
  and .passive.cpu_peak_core_grid_pct == {"validator1": {"all": 55.5}, "bootstrap": {"all": 12}}
' "$TMP/segments-report/weekly-summary.json" >/dev/null

mkdir -p "$TMP/breach" "$TMP/breach-report"
cp "$TMP/passing/summary.json" "$TMP/breach/summary.json"
printf '%s\n' 'host_protection_breach: injected guardian marker' \
	>"$TMP/breach/early-exit.txt"
SOAK_DIR="$TMP/breach" OUT_DIR="$TMP/breach-report" RUN_ID=3 RUN_ATTEMPT=1 \
	SOAK_KIND=daily DURATION_SECONDS=1800 WINDOW_SECONDS=79200 RETRY_ATTEMPT=0 \
	"$ROOT/scripts/bench/aggregate-perf-report.sh"
jq -e '
  .verdict == "regress"
  and .run.protection_breach == true
  and (.failures | any(. == "host protection breach aborted the soak"))
' "$TMP/breach-report/verdict.json" >/dev/null

printf 'soak report verdict tests passed\n'

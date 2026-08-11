#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
write_summary() {
	local output_dir="$1" iterations="$2" failures="$3" slot_delay="${4:-0}"
	SOAK_OUTPUT_DIR="$output_dir" \
		SOAK_METRICS_REGISTRY="$ROOT/scripts/bench/soak-metrics.json" \
		SOAK_TARGET_REF=dev \
		SOAK_TARGET_SHA=1086923fb257407baa2e15ce57841987503dbbb5 \
		SOAK_SLOT_DELAY_SECONDS="$slot_delay" \
		SOAK_VERSION=0.4.42 \
		SOAK_STARTED_AT=1000 \
		SOAK_FINISHED_AT=1180 \
		SOAK_DURATION_SECONDS=3600 \
		SOAK_ITERATIONS="$iterations" \
		SOAK_FAILURES="$failures" \
		SOAK_BENCH_SEGMENTS=1 \
		SOAK_BENCH_FAILURES=1 \
		"$ROOT/scripts/bench/write-soak-summary.sh"
}

BASE="$TMP/base"
mkdir -p "$BASE/iteration-00001-docker" "$BASE/iteration-00002-subprocess"
printf '%s\n' '{"iteration":1,"provider":"docker","duration_s":60,"ok":false,"rss_peak_mb":15580,"cpu_peak_pct":42.5,"cpu_peak_per_node_pct":{"validator1":42.5,"bootstrap":12},"finalization_latency":{"p50_ms":100,"p95_ms":200,"p99_ms":300},"too_far_ahead_errors":2,"metrics":{"lfb_spread":{"p50":3,"samples":4}}}' >"$BASE/iteration-00001-docker/metrics.json"
printf '%s\n' '{"iteration":2,"provider":"subprocess","duration_s":120,"ok":false,"rss_peak_mb":null,"cpu_peak_per_node_pct":{"validator1":55,"weird":"not-a-number"},"finalization_latency":{},"metrics":{}}' >"$BASE/iteration-00002-subprocess/metrics.json"
write_summary "$BASE" 2 2

jq -e '
  .target_ref == "dev"
  and .started_at == 1000
  and .elapsed_seconds == 180
  and .iterations == 2
  and .failures == 2
  and .shard_up_seconds == 0
  and .rss_peak_mb == 15580
  and .cpu_peak_core_grid_pct == {"validator1": {"all": 55}, "bootstrap": {"all": 12}}
  and .providers.docker.failures == 1
  and .providers.subprocess.failures == 1
  and .tracked_metrics.lfb_spread.p50 == 3
  and .tracked_metrics.lfb_spread.p95 == null
  and .tracked_metrics.lfb_spread.max == null
  and .tracked_metrics.lfb_spread.samples == 4
' "$BASE/summary.json" >/dev/null

EMPTY="$TMP/empty"
mkdir -p "$EMPTY"
write_summary "$EMPTY" 0 0 invalid
jq -e '
  .slot_delay_seconds == 0
  and .rss_peak_mb == null
  and .cpu_peak_pct == null
  and .cpu_peak_core_grid_pct == null
  and .shard_up_seconds == 0
  and .tracked_metrics == {}
  and .iteration_metrics == []
' "$EMPTY/summary.json" >/dev/null

# Shard uptime counts only completed-cycle iterations: the ok iteration
# contributes its duration, the failed one contributes nothing.
MIXED="$TMP/mixed"
mkdir -p "$MIXED/iteration-00001-docker" "$MIXED/iteration-00002-docker"
printf '%s\n' '{"iteration":1,"provider":"docker","duration_s":60,"ok":true,"metrics":{}}' >"$MIXED/iteration-00001-docker/metrics.json"
printf '%s\n' '{"iteration":2,"provider":"docker","duration_s":120,"ok":false,"metrics":{}}' >"$MIXED/iteration-00002-docker/metrics.json"
write_summary "$MIXED" 2 1
jq -e '.shard_up_seconds == 60' "$MIXED/summary.json" >/dev/null

SPARSE="$TMP/sparse"
mkdir -p "$SPARSE/iteration-00001-docker"
printf '%s\n' '{"iteration":1,"provider":"docker","ok":true,"rss_peak_mb":null,"metrics":{"lfb_spread":{"samples":2}}}' >"$SPARSE/iteration-00001-docker/metrics.json"
write_summary "$SPARSE" 1 0
jq -e '
  .rss_peak_mb == null
  and .cpu_peak_pct == null
  and .cpu_peak_core_grid_pct == null
  and .shard_up_seconds == 0
  and .tracked_metrics.lfb_spread.p50 == null
  and .tracked_metrics.lfb_spread.p95 == null
  and .tracked_metrics.lfb_spread.max == null
  and .tracked_metrics.lfb_spread.samples == 2
' "$SPARSE/summary.json" >/dev/null

# jq 1.6 compatibility lint. The soak VM (Ubuntu 22.04) ships jq 1.6, which
# rejects jq keywords as variable names ("$def" cost run 30713818751 its
# summary and, through null metadata, its checkpoint). CI runs on jq 1.7+,
# which accepts them, so parsing alone cannot catch the regression here.
JQ_KEYWORDS='def|if|then|elif|else|end|as|reduce|foreach|try|catch|label|import|include|and|or|not|__loc__'
if grep -rnE "(as \\\$|--arg |--argjson |--slurpfile )($JQ_KEYWORDS)\\b" \
  "$ROOT/scripts/bench" "$ROOT/scripts/oci" "$ROOT/scripts/run-merge-recovery-soak.sh" \
  --include='*.sh' 2>/dev/null; then
  printf 'jq keyword used as a variable name (breaks jq 1.6 on the soak runner)\n' >&2
  exit 1
fi

printf 'soak summary writer tests passed\n'

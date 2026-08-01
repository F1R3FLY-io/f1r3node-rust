#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/iteration-00001-docker" "$TMP/iteration-00002-subprocess"
printf '%s\n' '{"iteration":1,"provider":"docker","duration_s":60,"ok":false,"rss_peak_mb":15580,"cpu_peak_pct":42.5,"finalization_latency":{"p50_ms":100,"p95_ms":200,"p99_ms":300},"too_far_ahead_errors":2,"metrics":{"lfb_spread":{"p50":3,"samples":4}}}' >"$TMP/iteration-00001-docker/metrics.json"
printf '%s\n' '{"iteration":2,"provider":"subprocess","duration_s":120,"ok":false,"rss_peak_mb":null,"finalization_latency":{},"metrics":{}}' >"$TMP/iteration-00002-subprocess/metrics.json"

SOAK_OUTPUT_DIR="$TMP" \
	SOAK_METRICS_REGISTRY="$ROOT/scripts/bench/soak-metrics.json" \
	SOAK_TARGET_REF=dev \
	SOAK_TARGET_SHA=1086923fb257407baa2e15ce57841987503dbbb5 \
	SOAK_VERSION=0.4.42 \
	SOAK_STARTED_AT=1000 \
	SOAK_FINISHED_AT=1180 \
	SOAK_DURATION_SECONDS=3600 \
	SOAK_ITERATIONS=2 \
	SOAK_FAILURES=2 \
	SOAK_BENCH_SEGMENTS=1 \
	SOAK_BENCH_FAILURES=1 \
	"$ROOT/scripts/bench/write-soak-summary.sh"

jq -e '
  .target_ref == "dev"
  and .started_at == 1000
  and .elapsed_seconds == 180
  and .iterations == 2
  and .failures == 2
  and .rss_peak_mb == 15580
  and .providers.docker.failures == 1
  and .providers.subprocess.failures == 1
  and .tracked_metrics.lfb_spread.p50 == 3
  and .tracked_metrics.lfb_spread.p95 == null
  and .tracked_metrics.lfb_spread.max == null
  and .tracked_metrics.lfb_spread.samples == 4
' "$TMP/summary.json" >/dev/null

printf 'soak summary writer tests passed\n'

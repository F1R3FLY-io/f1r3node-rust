#!/usr/bin/env bash
# One interleaved performance-benchmark segment for the weekend soak.
#
# Brings up the local docker shard from the node-under-test checkout, floods
# deploys via latency-benchmark.sh, samples validator container RSS while the
# flood runs, and writes a self-contained metrics.json into OUT_DIR. The
# shard is always torn down, even on failure. Exits nonzero when the segment
# produced no usable metrics (no deploy reached finalization).
#
# Required environment:
#   NODE_REPO_DIR   checkout containing docker/shard.yml (image must already
#                   be built/loaded as f1r3flyindustries/f1r3fly-rust:latest)
#   OUT_DIR         directory for this segment's outputs
# Optional:
#   BENCH_DURATION  flood seconds                 (default 300)
#   BENCH_RATE      deploys per second            (default 2)
#   SEGMENT_INDEX   ordinal within the soak       (default 0)
#   SOAK_STARTED_AT epoch seconds of soak start   (default now)
#   READY_TIMEOUT   seconds to wait for the shard (default 420)

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

NODE_REPO_DIR="${NODE_REPO_DIR:?NODE_REPO_DIR is required}"
OUT_DIR="${OUT_DIR:?OUT_DIR is required}"
BENCH_DURATION="${BENCH_DURATION:-300}"
BENCH_RATE="${BENCH_RATE:-2}"
SEGMENT_INDEX="${SEGMENT_INDEX:-0}"
SOAK_STARTED_AT="${SOAK_STARTED_AT:-$(date +%s)}"
READY_TIMEOUT="${READY_TIMEOUT:-420}"
HTTP_PORT=40413

SHARD_YML="$NODE_REPO_DIR/docker/shard.yml"
[ -f "$SHARD_YML" ] || { echo "missing $SHARD_YML" >&2; exit 2; }
command -v jq >/dev/null || { echo "jq not found" >&2; exit 2; }

# latency-benchmark.sh needs a funded deployer. Default to the shard's own
# bootstrap key from the checked-in fixture env — the documented default
# ("funded locally and in wallets.txt", docs/vps-cloud-testing.md) that the
# benchmark script requires but nothing in the soak path ever supplied, which
# made every soak bench segment die on its first line (run 30713818751:
# bench_segments=1 bench_failures=1, every week, silently).
if [ -z "${DEPLOYER_KEY:-}" ]; then
  DEPLOYER_KEY="$(awk -F= '$1 == "BOOTSTRAP_PRIVATE_KEY" { print $2; exit }' \
    "$NODE_REPO_DIR/docker/.env" 2>/dev/null || true)"
  if [ -z "$DEPLOYER_KEY" ]; then
    echo "DEPLOYER_KEY not set and no BOOTSTRAP_PRIVATE_KEY in $NODE_REPO_DIR/docker/.env" >&2
    exit 2
  fi
fi
export DEPLOYER_KEY

mkdir -p "$OUT_DIR"
COMPOSE=(docker compose -f "$SHARD_YML" -p soak-bench)
SAMPLER_PID=""

cleanup() {
  [ -n "$SAMPLER_PID" ] && kill "$SAMPLER_PID" 2>/dev/null
  "${COMPOSE[@]}" logs --no-color > "$OUT_DIR/shard.log" 2>&1
  "${COMPOSE[@]}" down -v --timeout 60 > /dev/null 2>&1
}
trap cleanup EXIT

SEGMENT_STARTED_AT="$(date +%s)"

if ! "${COMPOSE[@]}" up -d > "$OUT_DIR/compose-up.log" 2>&1; then
  echo "shard compose up failed" >&2
  exit 1
fi

READY_DEADLINE=$(( $(date +%s) + READY_TIMEOUT ))
READY=0
while [ "$(date +%s)" -lt "$READY_DEADLINE" ]; do
  if curl -fsS --max-time 5 "http://localhost:${HTTP_PORT}/api/status" > /dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 5
done
if [ "$READY" -ne 1 ]; then
  echo "shard not ready within ${READY_TIMEOUT}s" >&2
  exit 1
fi

# Extra settling time so all validators have approved genesis and bonded
sleep 30

# Background RSS sampler: peak MB across rnode.* containers, one sample/10s
RSS_FILE="$OUT_DIR/rss-samples.tsv"
: > "$RSS_FILE"
(
  while true; do
    NOW="$(date +%s)"
    docker stats --no-stream --format '{{.Name}}\t{{.MemUsage}}' 2>/dev/null \
      | awk -F'\t' -v ts="$NOW" '$1 ~ /^rnode\./ {
          split($2, m, " / "); v=m[1]
          if (v ~ /GiB/) { sub(/GiB/,"",v); mb=v*1024 }
          else if (v ~ /MiB/) { sub(/MiB/,"",v); mb=v }
          else if (v ~ /KiB/) { sub(/KiB/,"",v); mb=v/1024 }
          else mb=0
          printf "%s\t%s\t%.0f\n", ts, $1, mb
        }' >> "$RSS_FILE"
    sleep 10
  done
) &
SAMPLER_PID=$!

BENCH_OUT="$OUT_DIR/bench"
DURATION="$BENCH_DURATION" DEPLOYS_PER_SEC="$BENCH_RATE" OUT_DIR="$BENCH_OUT" \
  "$SCRIPT_DIR/latency-benchmark.sh" --apply \
  --duration "$BENCH_DURATION" --rate "$BENCH_RATE" --out-dir "$BENCH_OUT" \
  > "$OUT_DIR/bench.log" 2>&1
BENCH_STATUS=$?

kill "$SAMPLER_PID" 2>/dev/null || true
wait "$SAMPLER_PID" 2>/dev/null || true
SAMPLER_PID=""

RSS_PEAK_MB="$(awk -F'\t' 'BEGIN{max=0} {if ($3 > max) max=$3} END{print max}' "$RSS_FILE")"

if [ "$BENCH_STATUS" -ne 0 ] || [ ! -s "$BENCH_OUT/metrics.json" ]; then
  jq -n \
    --argjson idx "$SEGMENT_INDEX" \
    --argjson started "$SEGMENT_STARTED_AT" \
    --argjson offset "$((SEGMENT_STARTED_AT - SOAK_STARTED_AT))" \
    --argjson rss "${RSS_PEAK_MB:-0}" \
    '{segment_index: $idx, started_at: $started, offset_seconds: $offset,
      rss_peak_mb: $rss, ok: false}' > "$OUT_DIR/metrics.json"
  echo "benchmark flood failed (status $BENCH_STATUS); bench.log tail:" >&2
  tail -20 "$OUT_DIR/bench.log" >&2 || true
  exit 1
fi

jq \
  --argjson idx "$SEGMENT_INDEX" \
  --argjson started "$SEGMENT_STARTED_AT" \
  --argjson offset "$((SEGMENT_STARTED_AT - SOAK_STARTED_AT))" \
  --argjson rss "${RSS_PEAK_MB:-0}" \
  '. + {segment_index: $idx, started_at: $started, offset_seconds: $offset,
        rss_peak_mb: $rss, ok: (.finalized > 0)}' \
  "$BENCH_OUT/metrics.json" > "$OUT_DIR/metrics.json"

jq -e '.ok' "$OUT_DIR/metrics.json" > /dev/null

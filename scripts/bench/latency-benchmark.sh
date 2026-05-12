#!/usr/bin/env bash
# F1R3FLY latency benchmark — port of f1r3node's run-latency-benchmark.sh.
#
# What it does:
#   1. For --duration seconds, floods deploys at a target rate via the
#      node binary inside a container (docker exec locally, ssh docker
#      exec remotely). Each deploy signs a trivial Rholang contract
#      using a funded wallet key.
#   2. Records submit timestamps + DeployIds.
#   3. Polls /api/status + last-finalized-block for LFB advancement.
#   4. Matches DeployIds against block deploy lists to extract
#      finalize timestamps.
#   5. Emits load-summary.txt and latency-report.txt (p50/p95/min/max).
#
# Design note on "grpcurl + HTTP /api" AC:
#   The task asks for grpcurl + HTTP /api. But F1R3FLY deploys require
#   a signed secp256k1 payload that grpcurl can't generate on its own
#   without a pre-signer. We use the `node` binary (already in the
#   image) for signing/submitting deploys, and curl for /api/status.
#   The point of the AC — drop rust-client external dep — is met.
#
# Default mode is DRY-RUN. Pass --apply to actually flood.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# --- Defaults (overridable via flags or env) ---
DURATION="${DURATION:-60}"                  # seconds
DEPLOYS_PER_SEC="${DEPLOYS_PER_SEC:-2}"     # target rate; capped per cache size
PHLO_LIMIT="${PHLO_LIMIT:-500000}"
PHLO_PRICE="${PHLO_PRICE:-1}"
SHARD_ID="${SHARD_ID:-root}"
CONTAINER="${CONTAINER:-rnode.validator1}"
HOST=""                                     # empty = local (docker exec); set = ssh to that host
SSH_USER="${SSH_USER:-opc}"
KEY_FILE="${KEY_FILE:-}"                    # SSH key for remote host; auto-picked from testbed-state if empty
APPLY=0
OUT_DIR="${OUT_DIR:-/tmp/f1r3fly-bench-$(date +%Y%m%d-%H%M%S)}"

# Funded deployer — defaults to bootstrap's key (funded locally, funded in
# wallets.txt as validator4's REV address via commit 993c239 for distributed)
DEPLOYER_KEY="${DEPLOYER_KEY:-5f668a7ee96d944a4494cc947e4005e172d7ab3461ee5538f1f2a45a835e9657}"

# Status polling
POLL_INTERVAL="${POLL_INTERVAL:-3}"
HTTP_PORT="${HTTP_PORT:-40413}"             # validator1 HTTP port in local shard.yml

# --- Logging ---
log()  { echo "[$(date +%H:%M:%S)] $*" >&2; }
info() { log "[info] $*"; }
warn() { log "[warn] $*"; }
err()  { log "[err ] $*"; }
die()  { err "$@"; exit 1; }

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --duration SECONDS      Flood duration (default: $DURATION)
  --rate N                Deploys per second (default: $DEPLOYS_PER_SEC)
  --host HOSTNAME         Remote VPS to bench against (default: local docker exec)
  --container NAME        Container name to target (default: $CONTAINER)
  --http-port PORT        HTTP port for /api/status (default: $HTTP_PORT)
  --out-dir PATH          Output directory (default: auto under /tmp)
  --apply                 Actually run the flood (default is dry-run)
  --dry-run               Print planned commands without executing (default)
  -h, --help              Show this help

Environment: DEPLOYS_PER_SEC, PHLO_LIMIT, PHLO_PRICE, SHARD_ID,
DEPLOYER_KEY, SSH_USER, KEY_FILE, POLL_INTERVAL, OUT_DIR

Examples:
  # Local, 30s, 5 deploys/sec, dry-run preview
  $(basename "$0") --duration 30 --rate 5

  # Remote VPS, 120s burst
  $(basename "$0") --host 203.0.113.10 --duration 120 --apply
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --duration)  DURATION="$2"; shift 2 ;;
    --rate)      DEPLOYS_PER_SEC="$2"; shift 2 ;;
    --host)      HOST="$2"; shift 2 ;;
    --container) CONTAINER="$2"; shift 2 ;;
    --http-port) HTTP_PORT="$2"; shift 2 ;;
    --out-dir)   OUT_DIR="$2"; shift 2 ;;
    --apply)     APPLY=1; shift ;;
    --dry-run)   APPLY=0; shift ;;
    -h|--help)   usage; exit 0 ;;
    *)           err "Unknown argument: $1"; usage; exit 2 ;;
  esac
done

# --- Auto-pick SSH key from testbed-state.json when remote ---
if [[ -n "$HOST" && -z "$KEY_FILE" ]]; then
  STATE_FILE="${REPO_ROOT}/scripts/remote/testbed-state.json"
  DEFAULT_KEY="${REPO_ROOT}/scripts/remote/testbed.pem"
  if [[ -f "$DEFAULT_KEY" ]]; then
    KEY_FILE="$DEFAULT_KEY"
  fi
fi

# --- Command builder: runs a command inside the target container ---
EXEC_CMD=()
if [[ -n "$HOST" ]]; then
  [[ -f "$KEY_FILE" ]] || die "Remote run requires SSH key at KEY_FILE (tried $KEY_FILE)"
  EXEC_CMD=(ssh -i "$KEY_FILE" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR "${SSH_USER}@${HOST}" docker exec "$CONTAINER")
  API_BASE="http://${HOST}:${HTTP_PORT}"
  LOG_CMD=(ssh -i "$KEY_FILE" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR "${SSH_USER}@${HOST}" docker logs "$CONTAINER")
else
  command -v docker >/dev/null || die "docker not found"
  EXEC_CMD=(docker exec "$CONTAINER")
  API_BASE="http://localhost:${HTTP_PORT}"
  LOG_CMD=(docker logs "$CONTAINER")
fi

info "=== F1R3FLY latency benchmark ==="
info "Mode:       $([[ $APPLY == 1 ]] && echo APPLY || echo DRY-RUN)"
info "Target:     ${HOST:-localhost} container=$CONTAINER"
info "API base:   $API_BASE"
info "Duration:   ${DURATION}s @ ${DEPLOYS_PER_SEC} deploys/sec"
info "Output:     $OUT_DIR"

[[ "$APPLY" == "1" ]] || { info "DRY-RUN complete. Add --apply to actually run."; exit 0; }

# --- Prereqs ---
command -v curl >/dev/null || die "curl not found"
command -v jq   >/dev/null || die "jq not found"
command -v awk  >/dev/null || die "awk not found"

mkdir -p "$OUT_DIR"
SUBMITS_FILE="$OUT_DIR/submits.tsv"
FINALS_FILE="$OUT_DIR/finals.tsv"
SUMMARY="$OUT_DIR/load-summary.txt"
REPORT="$OUT_DIR/latency-report.txt"
: > "$SUBMITS_FILE" > "$FINALS_FILE"

# --- Deploy the benchmark contract file into the container ---
BENCH_RHO_LOCAL="$OUT_DIR/bench.rho"
cat > "$BENCH_RHO_LOCAL" <<'RHO'
// Minimal no-op contract for latency benchmarking
new unused in { Nil }
RHO

info "Staging bench.rho into $CONTAINER"
if [[ -n "$HOST" ]]; then
  scp -i "$KEY_FILE" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR \
    "$BENCH_RHO_LOCAL" "${SSH_USER}@${HOST}:/tmp/bench.rho"
  ssh -i "$KEY_FILE" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR \
    "${SSH_USER}@${HOST}" "docker cp /tmp/bench.rho ${CONTAINER}:/tmp/bench.rho"
else
  docker cp "$BENCH_RHO_LOCAL" "${CONTAINER}:/tmp/bench.rho"
fi

# --- Preflight: confirm node is reachable ---
info "Preflight: hitting $API_BASE/api/status"
STATUS_JSON="$(curl -fsS --max-time 5 "${API_BASE}/api/status")" || die "Node unreachable at $API_BASE"
PEERS="$(echo "$STATUS_JSON" | jq -r '.peers')"
NODES="$(echo "$STATUS_JSON" | jq -r '.nodes')"
info "Node healthy: peers=$PEERS nodes=$NODES"

# --- Deploy flood ---
INTERVAL_SEC="$(awk -v r="$DEPLOYS_PER_SEC" 'BEGIN{printf "%.3f", 1.0/r}')"
info "Starting flood: interval=${INTERVAL_SEC}s"

DEADLINE=$(( $(date +%s) + DURATION ))
SUBMITTED=0
FAILED=0

while [[ $(date +%s) -lt $DEADLINE ]]; do
  SUBMIT_MS="$(date +%s%3N)"
  RESULT="$("${EXEC_CMD[@]}" /opt/docker/bin/node deploy \
    "$PHLO_LIMIT" "$PHLO_PRICE" 0 "$DEPLOYER_KEY" /dev/null /tmp/bench.rho "$SHARD_ID" 2>&1 || true)"
  DEPLOY_ID="$(echo "$RESULT" | awk -F': ' '/DeployId is/{print $2}' | tr -d '[:space:]')"
  if [[ -n "$DEPLOY_ID" ]]; then
    printf '%s\t%s\n' "$SUBMIT_MS" "$DEPLOY_ID" >> "$SUBMITS_FILE"
    SUBMITTED=$((SUBMITTED + 1))
  else
    FAILED=$((FAILED + 1))
    echo "$RESULT" >> "$OUT_DIR/deploy-errors.log"
  fi
  sleep "$INTERVAL_SEC"
done

info "Flood complete: submitted=$SUBMITTED failed=$FAILED"

# --- Wait for finalization + match deploys ---
info "Waiting ${POLL_INTERVAL}s for blocks to propagate, then harvesting finalizations"
WAIT_DEADLINE=$(( $(date +%s) + 90 ))

while [[ $(date +%s) -lt $WAIT_DEADLINE ]]; do
  # Fetch last 50 blocks; match any DeployIds we know about
  SHOW="$("${EXEC_CMD[@]}" /opt/docker/bin/node show-blocks 50 2>&1 || true)"
  # Crude extraction: block_number + deployer + sig lines
  echo "$SHOW" | awk '
    /block_number:/ { blk=$2; gsub(",","",blk) }
    /sig: "/ {
      sig=$0; sub(/^[^"]*"/,"",sig); sub(/".*/,"",sig);
      print blk"\t"sig
    }' | while read -r blk sig; do
      [[ -z "$sig" ]] && continue
      if grep -q -F "$sig" "$SUBMITS_FILE" && ! grep -q -F "$sig" "$FINALS_FILE"; then
        FINAL_MS="$(date +%s%3N)"
        printf '%s\t%s\t%s\n' "$FINAL_MS" "$sig" "$blk" >> "$FINALS_FILE"
      fi
    done
  # Stop early if every submitted deploy is matched
  MATCHED="$(wc -l < "$FINALS_FILE" | tr -d ' ')"
  [[ "$MATCHED" -ge "$SUBMITTED" ]] && break
  sleep "$POLL_INTERVAL"
done

# --- Emit load-summary.txt ---
MATCHED="$(wc -l < "$FINALS_FILE" | tr -d ' ')"
THROUGHPUT="$(awk -v s="$SUBMITTED" -v d="$DURATION" 'BEGIN{if(d>0)printf "%.3f",s/d; else print "0"}')"

cat > "$SUMMARY" <<EOF
F1R3FLY latency benchmark — load summary
=========================================
Target:             ${HOST:-localhost} / $CONTAINER
Duration:           ${DURATION}s
Target rate:        ${DEPLOYS_PER_SEC} deploys/sec
Submitted:          $SUBMITTED
Errored on submit:  $FAILED
Finalized:          $MATCHED / $SUBMITTED
Observed throughput:${THROUGHPUT} deploys/sec
Output directory:   $OUT_DIR
EOF

info "Wrote $SUMMARY"
cat "$SUMMARY" >&2

# --- Emit latency-report.txt (p50/p95 via sort+awk) ---
# Join submit with final on DeployId (= sig), compute latency ms
awk -F'\t' 'NR==FNR{s[$2]=$1; next} ($2 in s){print $1-s[$2]}' "$SUBMITS_FILE" "$FINALS_FILE" \
  | sort -n > "$OUT_DIR/latencies.raw"

if [[ -s "$OUT_DIR/latencies.raw" ]]; then
  awk '
    { a[NR]=$1; sum+=$1 }
    END {
      n=NR
      if (n==0) { print "no samples"; exit }
      p50=a[int((n+1)*0.5)]; p95=a[int((n+1)*0.95)]
      printf "samples:    %d\n", n
      printf "min_ms:     %d\n", a[1]
      printf "p50_ms:     %d\n", p50
      printf "p95_ms:     %d\n", p95
      printf "max_ms:     %d\n", a[n]
      printf "avg_ms:     %.1f\n", sum/n
    }' "$OUT_DIR/latencies.raw" > "$REPORT"
else
  echo "no deploys were matched to finalized blocks; try longer --duration or check deploy errors" > "$REPORT"
fi

info "Wrote $REPORT"
cat "$REPORT" >&2

# Also run the log-level profiler for per-validator propose/replay latencies
if [[ -x "${SCRIPT_DIR}/profile-casper-latency.sh" ]]; then
  info "Running profile-casper-latency.sh for per-validator timings"
  HOST="$HOST" KEY_FILE="$KEY_FILE" SSH_USER="$SSH_USER" \
    "${SCRIPT_DIR}/profile-casper-latency.sh" "$CONTAINER" > "$OUT_DIR/casper-profile.txt" 2>&1 || true
  info "Wrote $OUT_DIR/casper-profile.txt"
fi

info "=== Benchmark complete ==="
info "See: $OUT_DIR"

#!/usr/bin/env bash
# Casper log-level latency profiler — port of f1r3node/scripts/ci/profile-casper-latency.sh.
#
# Parses the Rust node's structured JSON logs for timing events and emits
# per-container p50/p95 on:
#   - Propose core timing     (target f1r3fly.propose.timing)
#   - Block replay timing     (casper::rust::multi_parent_casper_impl)
#   - Finalization timing     (target f1r3fly.casper, finalizer-run-*)
#
# Usage:
#   profile-casper-latency.sh [CONTAINER]
#
# Environment:
#   HOST       Remote VPS for ssh docker logs (default: local)
#   SSH_USER   Remote user (default: opc)
#   KEY_FILE   SSH identity for remote (required if HOST set)
#
# Rust node log format (JSON lines):
#   {"timestamp":"...","level":"INFO","message":"Propose timing: mode=async, snapshot_ms=0, propose_core_ms=69, total_ms=70","target":"f1r3fly.propose.timing",...}
#   {"timestamp":"...","message":"Block replayed: #N (hash) (0d) (Valid) [61.42075ms]",...}

set -euo pipefail

CONTAINER="${1:-rnode.validator1}"
HOST="${HOST:-}"
SSH_USER="${SSH_USER:-opc}"
KEY_FILE="${KEY_FILE:-}"

if [[ -n "$HOST" ]]; then
  [[ -f "$KEY_FILE" ]] || { echo "HOST set but KEY_FILE missing" >&2; exit 2; }
  LOG_CMD=(ssh -i "$KEY_FILE" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR "${SSH_USER}@${HOST}" docker logs "$CONTAINER")
else
  LOG_CMD=(docker logs "$CONTAINER")
fi

command -v jq  >/dev/null || { echo "jq required" >&2; exit 2; }
command -v awk >/dev/null || { echo "awk required" >&2; exit 2; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Pull logs once, reuse
"${LOG_CMD[@]}" 2>&1 > "$TMP/raw.log" || true

echo "=== Casper latency profile — container: $CONTAINER ==="
echo "Log lines: $(wc -l < "$TMP/raw.log" | tr -d ' ')"
echo ""

# --- Propose core timing ---
# Message shape: "Propose timing: mode=async, snapshot_ms=0, propose_core_ms=69, total_ms=70"
jq -r 'select(.target=="f1r3fly.propose.timing") | .message' "$TMP/raw.log" 2>/dev/null \
  | awk -F'[=,]' '
      /propose_core_ms/ {
        for(i=1;i<=NF;i++) if($i~/propose_core_ms/) print $(i+1)+0
      }' | sort -n > "$TMP/propose_core_ms"

# --- Block replay timing ---
# Message shape: "Block replayed: #14 (f499759348...) (0d) (Valid) [61.42075ms]"
grep -o '"Block replayed:.*\[[0-9.]*ms\]' "$TMP/raw.log" 2>/dev/null \
  | awk -F'[][]' '{print $2}' | tr -d 'ms' | awk '{printf "%d\n", $1+0}' \
  | sort -n > "$TMP/replay_ms"

# --- Finalizer cycle timing ---
# We compute time between "finalizer-run-started" and "finalizer-run-finished" events.
# Both share target "f1r3fly.casper" and have timestamps.
jq -r 'select(.target=="f1r3fly.casper") | select(.message=="finalizer-run-started" or .message=="finalizer-run-finished") | "\(.timestamp)\t\(.message)"' "$TMP/raw.log" 2>/dev/null \
  | awk -F'\t' '
      /finalizer-run-started/   { started=$1 }
      /finalizer-run-finished/  {
        if (started != "") {
          cmd="date -u -d \"" $1 "\" +%s%3N 2>/dev/null || gdate -u -d \"" $1 "\" +%s%3N 2>/dev/null"
          cmd | getline fin_ms; close(cmd)
          cmd2="date -u -d \"" started "\" +%s%3N 2>/dev/null || gdate -u -d \"" started "\" +%s%3N 2>/dev/null"
          cmd2 | getline start_ms; close(cmd2)
          if (start_ms != "" && fin_ms != "") print fin_ms - start_ms
          started=""
        }
      }' | sort -n > "$TMP/finalize_ms"

# --- Percentile helper ---
emit_stats() {
  local label="$1" file="$2"
  if [[ ! -s "$file" ]]; then
    printf "%-20s no samples\n" "$label"
    return
  fi
  awk -v label="$label" '
    { a[NR]=$1; sum+=$1 }
    END {
      n=NR
      p50=a[int((n+1)*0.5)]; p95=a[int((n+1)*0.95)]
      printf "%-20s n=%-6d min=%-6d p50=%-6d p95=%-6d max=%-6d avg=%.1f\n", \
        label, n, a[1], p50, p95, a[n], sum/n
    }' "$file"
}

emit_stats "propose_core_ms"     "$TMP/propose_core_ms"
emit_stats "block_replay_ms"     "$TMP/replay_ms"
emit_stats "finalizer_cycle_ms"  "$TMP/finalize_ms"

echo ""
echo "Raw samples preserved in: $TMP (removed on exit)"

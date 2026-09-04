#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL="$ROOT/formal/tlaplus/uptime/UptimeEnvelopeDominance.tla"
SAFE="$ROOT/formal/tlaplus/uptime/MC_UptimeEnvelopeDominance.cfg"
UNSAFE="$ROOT/formal/tlaplus/uptime/MC_UptimeEnvelopeDominance_unsafe.cfg"
LOG_DIR="$ROOT/target/verification/uptime/tla"
mkdir -p "$LOG_DIR"
TLC_REPO_ROOT="$ROOT"
TLC_METADIR_ROOT="$LOG_DIR"
TLC_HEAP="${TLC_HEAP:-1g}"
TLC_RSS="${TLC_RSS:-3G}"
TLC_WORKERS="${TLC_WORKERS:-1}"
export TLC_REPO_ROOT TLC_METADIR_ROOT TLC_HEAP TLC_RSS TLC_WORKERS
source "$ROOT/scripts/lib/tlc-run.sh"

command -v tla2sany >/dev/null 2>&1 || {
  echo "error: required uptime TLA+ parser is unavailable: tla2sany" >&2
  exit 1
}

tla2sany "$MODEL" >"$LOG_DIR/sany.log" 2>&1
tlc_run "$LOG_DIR/safe-states" "$SAFE" "$MODEL" -deadlock >"$LOG_DIR/safe.log" 2>&1
grep -Fq 'No error has been found' "$LOG_DIR/safe.log" || {
  echo "error: uptime endpoint dominance model failed" >&2
  exit 1
}
if tlc_run "$LOG_DIR/unsafe-states" "$UNSAFE" "$MODEL" -deadlock >"$LOG_DIR/unsafe.log" 2>&1; then
  echo "error: favorable-only failure control unexpectedly preserved endpoint dominance" >&2
  exit 1
fi
grep -Eq 'Invariant (Dominance|ServiceOrder) is violated' "$LOG_DIR/unsafe.log" || {
  echo "error: uptime endpoint dominance control failed for an unrelated reason" >&2
  exit 1
}

echo "TLA+ uptime envelope dominance verification passed."

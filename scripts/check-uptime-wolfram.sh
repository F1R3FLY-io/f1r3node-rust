#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL="$ROOT/formal/wolfram/uptime/robust_operating_regions.wl"
REPORT="$ROOT/target/verification/uptime/storm/engineering-envelope.json"
LOG_DIR="$ROOT/target/verification/uptime/wolfram"
mkdir -p "$LOG_DIR"

test -s "$MODEL" || {
  echo "error: missing Wolfram uptime model: $MODEL" >&2
  exit 1
}
test -s "$REPORT" || {
  echo "error: the Wolfram uptime tier requires a successful Storm report" >&2
  exit 1
}

if command -v wolframscript >/dev/null 2>&1; then
  kernel=(wolframscript -file)
elif command -v math >/dev/null 2>&1; then
  kernel=(math -script)
elif command -v wolfram >/dev/null 2>&1; then
  kernel=(wolfram -script)
else
  echo "error: the explicitly selected Wolfram uptime tier requires a licensed kernel" >&2
  exit 1
fi

UPTIME_STORM_REPORT="$REPORT" "${kernel[@]}" "$MODEL" >"$LOG_DIR/robust-operating-regions.log" 2>&1 || {
  echo "error: Wolfram uptime exploration failed" >&2
  exit 1
}
grep -Fq '[robust_operating_regions] SELF-TEST: PASS' "$LOG_DIR/robust-operating-regions.log" || {
  echo "error: Wolfram uptime exploration omitted its PASS marker" >&2
  exit 1
}

echo "Wolfram uptime operating-region exploration passed."

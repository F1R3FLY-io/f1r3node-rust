#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL="$ROOT/formal/wolfram/cost_accounted_rho/reservation_admission_regions.wl"

if [[ ! -f "$MODEL" ]]; then
  echo "error: missing Wolfram cost-accounting model: $MODEL" >&2
  exit 1
fi

if command -v wolframscript >/dev/null 2>&1; then
  WOLFRAM_BIN=wolframscript
  WOLFRAM_RUN=(wolframscript -file)
elif command -v math >/dev/null 2>&1; then
  WOLFRAM_BIN=math
  WOLFRAM_RUN=(math -script)
elif command -v wolfram >/dev/null 2>&1; then
  WOLFRAM_BIN=wolfram
  WOLFRAM_RUN=(wolfram -script)
else
  echo "error: the explicitly selected Wolfram tier requires a licensed kernel on PATH" >&2
  exit 1
fi

set +e
output="$("${WOLFRAM_RUN[@]}" "$MODEL" 2>&1)"
status=$?
set -e

printf '%s\n' "$output"
if [[ $status -ne 0 ]]; then
  echo "error: Wolfram cost-accounting exploration failed under $WOLFRAM_BIN" >&2
  exit "$status"
fi
if ! grep -Fq '[reservation_admission_regions] SELF-TEST: PASS' <<<"$output"; then
  echo "error: Wolfram cost-accounting exploration omitted its PASS marker" >&2
  exit 1
fi

echo "Wolfram cost-accounting reservation/admission exploration passed."

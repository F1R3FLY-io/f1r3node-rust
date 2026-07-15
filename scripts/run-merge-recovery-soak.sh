#!/usr/bin/env bash
set -uo pipefail

DURATION_SECONDS="${SOAK_DURATION_SECONDS:?SOAK_DURATION_SECONDS is required}"
SYSTEM_INTEGRATION_DIR="${SYSTEM_INTEGRATION_DIR:?SYSTEM_INTEGRATION_DIR is required}"
OUTPUT_DIR="${SOAK_OUTPUT_DIR:-/tmp/merge-recovery-soak}"
TARGET_REF="${SOAK_TARGET_REF:-unknown}"
TARGET_SHA="${SOAK_TARGET_SHA:-unknown}"
if ! [[ "$DURATION_SECONDS" =~ ^[1-9][0-9]*$ ]]; then
  printf 'SOAK_DURATION_SECONDS must be a positive integer\n' >&2
  exit 2
fi
STARTED_AT="$(date +%s)"
DEADLINE="$((STARTED_AT + DURATION_SECONDS))"
FAILURES=0
ITERATIONS=0
PROVIDERS=(docker subprocess)

mkdir -p "$OUTPUT_DIR"

while [ "$(date +%s)" -lt "$DEADLINE" ]; do
  PROVIDER="${PROVIDERS[$((ITERATIONS % ${#PROVIDERS[@]}))]}"
  ITERATIONS="$((ITERATIONS + 1))"
  ITERATION_DIR="$OUTPUT_DIR/iteration-$(printf '%05d' "$ITERATIONS")-$PROVIDER"
  mkdir -p "$ITERATION_DIR"
  REMAINING="$((DEADLINE - $(date +%s)))"
  if [ "$REMAINING" -le 0 ]; then
    break
  fi

  set +e
  (
    cd "$SYSTEM_INTEGRATION_DIR"
    timeout --signal=TERM --kill-after=120 "${REMAINING}s" \
      poetry run pytest \
      integration-tests/test/tests/custom/test_load.py \
      --provider="$PROVIDER" \
      -v --tb=short --instafail --maxfail=20 \
      --timeout=1200
  ) 2>&1 | tee "$ITERATION_DIR/pytest.log"
  STATUS="${PIPESTATUS[0]}"
  set -e

  if [ "$STATUS" -eq 124 ] && [ "$(date +%s)" -ge "$DEADLINE" ]; then
    printf '%s\n' "deadline reached during iteration $ITERATIONS" > "$ITERATION_DIR/deadline.txt"
    break
  fi
  if [ "$STATUS" -ne 0 ]; then
    FAILURES="$((FAILURES + 1))"
    printf '%s\n' "$STATUS" > "$ITERATION_DIR/exit-code.txt"
    if [ -d "$SYSTEM_INTEGRATION_DIR/integration-tests/data" ]; then
      cp -a "$SYSTEM_INTEGRATION_DIR/integration-tests/data" "$ITERATION_DIR/data"
    fi
    sleep 30
  fi

done

FINISHED_AT="$(date +%s)"
{
  printf 'started_at=%s\n' "$STARTED_AT"
  printf 'finished_at=%s\n' "$FINISHED_AT"
  printf 'target_ref=%s\n' "$TARGET_REF"
  printf 'target_sha=%s\n' "$TARGET_SHA"
  printf 'requested_seconds=%s\n' "$DURATION_SECONDS"
  printf 'elapsed_seconds=%s\n' "$((FINISHED_AT - STARTED_AT))"
  printf 'iterations=%s\n' "$ITERATIONS"
  printf 'failures=%s\n' "$FAILURES"
} | tee "$OUTPUT_DIR/summary.txt"

if [ "$FAILURES" -ne 0 ]; then
  exit 1
fi

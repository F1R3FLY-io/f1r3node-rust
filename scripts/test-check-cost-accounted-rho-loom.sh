#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GATE="$ROOT/scripts/check-cost-accounted-rho-loom.sh"

cargo() {
  [[ "$*" == "test --locked -p cost-accounting-loom-models" ]] || return 97
  [[ "$RUSTFLAGS" == "--cfg loom -C target-cpu=native" ]] || return 96
  [[ "$LOOM_MAX_PREEMPTIONS" == "$EXPECTED_PREEMPTIONS" ]] || return 95
  case "$GATE_CASE" in
    fail) return 101 ;;
    empty) return 0 ;;
    zero) printf '%s\n' 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s' ;;
    ignored) printf '%s\n' 'test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s' ;;
    filtered) printf '%s\n' 'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s' ;;
    *) printf '%s\n' 'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s' ;;
  esac
}

timeout() {
  [[ "$1" == 900 ]] || return 94
  if [[ "$GATE_CASE" == timeout ]]; then
    return 124
  fi
  shift
  "$@"
}

command() {
  if [[ "$*" == "-v cargo" && "$GATE_CASE" == missing_cargo ]]; then
    return 1
  fi
  if [[ "$*" == "-v timeout" && "$GATE_CASE" == missing_timeout ]]; then
    return 1
  fi
  builtin command "$@"
}

export -f cargo timeout command
export EXPECTED_PREEMPTIONS=3
unset LOOM_MAX_PREEMPTIONS LOOM_MAX_BRANCHES LOOM_MAX_PERMUTATIONS LOOM_MAX_DURATION LOOM_CHECKPOINT_FILE

run_case() {
  local expected="$1" output status=0
  export GATE_CASE="$2"
  output="$(bash "$GATE" 2>&1)" || status=$?
  if [[ "$status" != "$expected" ]]; then
    printf 'FAIL %s: expected %s, got %s\n%s\n' "$GATE_CASE" "$expected" "$status" "$output" >&2
    exit 1
  fi
  if [[ "$status" == 0 && "$output" != *"preemption bound $EXPECTED_PREEMPTIONS"* ]]; then
    printf 'FAIL %s: missing exploration bound\n%s\n' "$GATE_CASE" "$output" >&2
    exit 1
  fi
  printf 'PASS %s\n' "$GATE_CASE"
}

run_case 0 success
run_case 101 fail
run_case 124 timeout
run_case 1 missing_cargo
run_case 1 missing_timeout
run_case 1 empty
run_case 1 zero
run_case 1 ignored
run_case 1 filtered

for partial_limit in LOOM_MAX_PERMUTATIONS LOOM_MAX_DURATION LOOM_CHECKPOINT_FILE; do
  export "$partial_limit=1"
  run_case 1 "$partial_limit"
  unset "$partial_limit"
done

export LOOM_MAX_PREEMPTIONS=invalid
run_case 1 invalid_preemptions
export LOOM_MAX_PREEMPTIONS=2 EXPECTED_PREEMPTIONS=2
run_case 0 configured_preemptions
export LOOM_MAX_BRANCHES=0
run_case 1 invalid_branches
export LOOM_MAX_BRANCHES=2000
run_case 0 configured_branches

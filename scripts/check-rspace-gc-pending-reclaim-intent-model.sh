#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export TLC_REPO_ROOT="$repo_root"
export TLC_HEAP="${TLC_HEAP:-512m}"
export TLC_WORKERS="${TLC_WORKERS:-2}"
export TLC_MEMORY_HIGH="${TLC_MEMORY_HIGH:-1G}"
export TLC_RSS="${TLC_RSS:-4G}"
export TLC_CPU_QUOTA="${TLC_CPU_QUOTA:-200%}"
export TLC_TASKS_MAX="${TLC_TASKS_MAX:-64}"
export TLC_RUNTIME_MAX_SEC="${TLC_RUNTIME_MAX_SEC:-180}"
source "$repo_root/scripts/lib/tlc-run.sh"

model=RSpaceGcPendingReclaimIntent
model_dir="$repo_root/formal/tlaplus/stack_safe_pda"
evidence_dir="$repo_root/target/verification/rspace-gc-pending-reclaim-intent"
mkdir -p "$evidence_dir"
cd "$model_dir"
metadir="$(tlc_metadir rspace-gc-pending-reclaim-intent)"
sha256sum "$model.tla" "$model"*.cfg \
  "$repo_root/scripts/check-rspace-gc-pending-reclaim-intent-model.sh"

baseline_log="$evidence_dir/baseline.log"
if ! tlc_run "$metadir" "$model.cfg" "$model.tla" -cleanup >"$baseline_log" 2>&1; then
  cat "$baseline_log"
  echo "gc-pending-reclaim-intent: baseline failed" >&2
  exit 1
fi
if ! rg -Fq 'Model checking completed. No error has been found.' "$baseline_log"; then
  cat "$baseline_log"
  echo "gc-pending-reclaim-intent: baseline did not complete cleanly" >&2
  exit 1
fi

expect_violation() {
  local variant="$1" invariant="$2"
  shift 2
  local log="$evidence_dir/$variant.log"
  if tlc_run "$metadir" "$model$variant.cfg" "$model.tla" -cleanup >"$log" 2>&1; then
    cat "$log"
    echo "gc-pending-reclaim-intent: $variant unexpectedly passed" >&2
    exit 1
  fi
  if ! rg -Fq "Invariant $invariant is violated" "$log" ||
     ! rg -Fq 'The behavior up to this point is:' "$log"; then
    cat "$log"
    echo "gc-pending-reclaim-intent: $variant failed without the expected $invariant trace" >&2
    exit 1
  fi
  if ! rg -o '^State [0-9]+: <[A-Za-z]+' "$log" | sed -E 's/^State [0-9]+: <//' |
     awk -v order="$*" '
       BEGIN { count = split(order, expected, " "); matched = 0 }
       { last = $0; if (matched < count && $0 == expected[matched + 1]) matched++ }
       END { exit !(matched == count && last == expected[count]) }
     '; then
    cat "$log"
    echo "gc-pending-reclaim-intent: $variant trace lacks $* in order, ending at the last" >&2
    exit 1
  fi
}

expect_violation TornIntentMutant RecoveryAvailable TearIntent
expect_violation TornIntentPublishedMutant TornIntentNeverPublished TearIntent
expect_violation KeepIntentTempMutant IntentTempNeverLive TearIntent Recover
expect_violation DropOnAdvanceMutant SelectedDeletionObligationCovered SelectAB AdvanceB
expect_violation OverwriteMutant SelectedDeletionObligationCovered SelectAB StageBC PersistBC
expect_violation PreCasWitness NoPreCasRecoveryOrphan StageAB Recover
expect_violation RetryWitness NoRetryAfterPendingAdvanceAndRestart \
  RefuseBC AdvanceB Recover FinishA

echo "gc-pending-reclaim-intent: baseline, five mutants and two witnesses passed; logs in $evidence_dir"

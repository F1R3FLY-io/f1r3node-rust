#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MODEL_ROOT="$ROOT/formal/tlaplus/host_work_budget"
ROCQ_ROOT="$ROOT/formal/rocq/host_work_budget"
LOG_ROOT="$ROOT/target/verification/host-work-budget"
mkdir -p "$LOG_ROOT"
WORK="$(mktemp -d "$LOG_ROOT/run.XXXXXX")"

cleanup() {
  find "$ROCQ_ROOT/theories" -maxdepth 1 -type f \
    \( -name '*.aux' -o -name '*.glob' -o -name '*.vo' \
       -o -name '*.vok' -o -name '*.vos' \) -delete
  rm -f "$ROCQ_ROOT/.lia.cache"
  rm -rf "$WORK"
}
trap cleanup EXIT

export TLC_REPO_ROOT="$ROOT"
export TLC_METADIR_ROOT="$WORK"
export TLC_WORKERS=1
export TLC_RSS=8G
source "$ROOT/scripts/lib/tlc-run.sh"

for command_name in tla2sany tlc coqc coqtop; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'error: %s is required for host-work verification\n' "$command_name" >&2
    exit 1
  fi
done

run_safe() {
  local log="$LOG_ROOT/tlc-safe.log"
  if tlc_run "$WORK/tlc-safe" \
      "$MODEL_ROOT/MC_HostWorkBudget.cfg" \
      "$MODEL_ROOT/MC_HostWorkBudget.tla" >"$log" 2>&1; then
    grep -Fq 'Model checking completed. No error has been found.' "$log"
  else
    return 1
  fi
}

run_unsafe() {
  local name="$1"
  local invariant="$2"
  local config="$MODEL_ROOT/MC_HostWorkBudget_${name}_unsafe.cfg"
  local log="$LOG_ROOT/tlc-${name}-unsafe.log"
  if tlc_run "$WORK/tlc-${name}-unsafe" "$config" \
      "$MODEL_ROOT/MC_HostWorkBudget.tla" >"$log" 2>&1; then
    printf 'error: unsafe control %s did not produce a counterexample\n' "$name" >&2
    return 1
  fi
  grep -Fq "Invariant $invariant is violated." "$log" \
    || grep -Fq "The invariant of $invariant is equal to FALSE" "$log"
}

(cd "$MODEL_ROOT" && tla2sany HostWorkBudget.tla) \
  >"$LOG_ROOT/sany-host-work-budget.log" 2>&1
(cd "$MODEL_ROOT" && tla2sany MC_HostWorkBudget.tla) \
  >"$LOG_ROOT/sany-model-checker.log" 2>&1

run_safe
run_unsafe arrival_order_result Inv_ArrivalOrderResult
run_unsafe wrapping_arithmetic Inv_CheckedReservation
run_unsafe saturating_arithmetic Inv_CheckedReservation
run_unsafe mutation_before_reservation Inv_FailureNonMutation
run_unsafe schedule_mismatch Inv_ReplayScheduleAgreement
run_unsafe replay_without_enforcement Inv_ReplayEnforced
run_unsafe noncumulative_replay Inv_ReplayCumulativeVerdictAgreement
run_unsafe cross_shard_shared_budget Inv_ShardLocalAdmission
run_unsafe economic_debit Inv_EconomicSeparation
run_unsafe decode_after_allocation Inv_DecodeAllocationBound

(cd "$ROCQ_ROOT" && tlc_bounded \
  coqc -Q theories CasperHostWork theories/HostWorkBudget.v) \
  >"$LOG_ROOT/rocq-host-work-budget.log" 2>&1
(cd "$ROCQ_ROOT" && tlc_bounded \
  coqc -Q theories CasperHostWork theories/HostWorkExecution.v) \
  >"$LOG_ROOT/rocq-host-work-execution.log" 2>&1

if rg -n '\b(Admitted|admit|Axiom|Parameter|Hypothesis)\b' \
    "$ROCQ_ROOT/theories" >"$LOG_ROOT/rocq-proof-escapes.log"; then
  printf 'error: host-work Rocq source contains a proof escape\n' >&2
  exit 1
fi

(cd "$ROCQ_ROOT" && tlc_bounded \
  coqtop -quiet -Q theories CasperHostWork <<'EOF'
From CasperHostWork Require Import HostWorkBudget HostWorkExecution.
Print Assumptions checked_reserve_granted_bounds.
Print Assumptions rejected_reserve_does_not_mutate.
Print Assumptions counter_overflow_is_rejected.
Print Assumptions host_reserve_preserves_economic_balance.
Print Assumptions rollback_restores_checkpoint.
Print Assumptions replay_schedule_agreement.
Print Assumptions failed_dimension_reserve_preserves_all_dimensions.
Print Assumptions zero_unit_reserve_preserves_usage.
Print Assumptions independent_dimension_reservations_commute.
Print Assumptions failed_transaction_rolls_back.
Print Assumptions failed_transaction_publishes_no_checkpoint_or_mutation.
Print Assumptions failed_transaction_end_verdict_is_schedule_independent.
Print Assumptions independent_shard_reservations_commute.
EOF
) >"$LOG_ROOT/rocq-assumptions.log" 2>&1

if grep -Fq 'Axioms:' "$LOG_ROOT/rocq-assumptions.log"; then
  printf 'error: host-work Rocq theorem depends on an unexpected axiom\n' >&2
  exit 1
fi

printf 'host-work TLA+ and Rocq verification passed\n'

#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-all}"
FAMILY="${2:-all}"
case "$MODE" in all|rocq|tlc|apalache) ;; *) exit 2 ;; esac
case "$FAMILY" in all|audit|recovery|effects|observation) ;; *) exit 2 ;; esac
(( $# <= 2 )) || exit 2
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MODEL_ROOT="$ROOT/formal/tlaplus/finalized_floor"
ROCQ_ROOT="$ROOT/formal/rocq/finalized_floor"
LOG_ROOT="$ROOT/target/verification/pr216/ledger-startup"
mkdir -p "$LOG_ROOT"
RUN="$(mktemp -d "$LOG_ROOT/verification.XXXXXX")"
export TMPDIR="$RUN"
export TLC_REPO_ROOT="$ROOT"
export TLC_METADIR_ROOT="$RUN"
export TLC_WORKERS=1
export TLC_HEAP="${TLC_HEAP:-1g}"
source "$ROOT/scripts/lib/tlc-run.sh"
printf 'Evidence directory: %s\n' "$RUN"
sha256sum "$MODEL_ROOT/FinalizationLedgerAudit.tla" \
  "$MODEL_ROOT/FinalizationRecovery.tla" \
  "$MODEL_ROOT/FinalizationEffectSelection.tla" \
  "$MODEL_ROOT/FinalizationEffectObservation.tla" \
  "$MODEL_ROOT"/MC_FinalizationLedgerAudit*.cfg \
  "$MODEL_ROOT"/MC_FinalizationRecovery*.cfg \
  "$MODEL_ROOT"/MC_FinalizationEffectSelection*.cfg \
  "$MODEL_ROOT"/MC_FinalizationEffectObservation*.cfg \
  "$ROCQ_ROOT/theories/FinalizationAtomicity.v" \
  "$ROCQ_ROOT/theories/FinalizationLedgerAudit.v" \
  "$ROCQ_ROOT/theories/FinalizationEffectSelection.v" \
  "$ROOT/scripts/check-finalization-ledger-audit.sh" >"$RUN/inputs.sha256"

cases=(
  'FinalizationLedgerAudit||Safety|safe'
  'FinalizationLedgerAudit|_skip_unsafe|Inv_ValidatedPrefix|unsafe'
  'FinalizationLedgerAudit|_early_ready_unsafe|Inv_Readiness|unsafe'
  'FinalizationLedgerAudit|_saved_cursor_unsafe|Inv_ValidatedPrefix|unsafe'
  'FinalizationLedgerAudit|_moving_target_unsafe|Inv_ExactTarget|unsafe'
  'FinalizationLedgerAudit|_over_budget_unsafe|Inv_BoundedBatch|unsafe'
  'FinalizationRecovery||Safety|safe'
  'FinalizationRecovery|_projection_gap_unsafe|Inv_ProjectionPrefix|unsafe'
  'FinalizationRecovery|_early_effect_unsafe|Inv_EffectsAfterProjection|unsafe'
  'FinalizationRecovery|_effect_gap_unsafe|Inv_EffectsCursorPrefix|unsafe'
  'FinalizationRecovery|_early_compaction_unsafe|Inv_RequiredReceiptsPresent|unsafe'
  'FinalizationRecovery|_compaction_cursor_unsafe|Inv_CompactionCursorClean|unsafe'
  'FinalizationEffectSelection||Safety|safe'
  'FinalizationEffectSelection|_fresh_head_unsafe|Inv_EffectsAfterProjection|unsafe'
  'FinalizationEffectSelection|_late_receipt_guard_unsafe|Inv_EffectsAfterProjection|unsafe'
  'FinalizationEffectObservation||Safety|safe'
  'FinalizationEffectObservation|_missing_recheck_unsafe|Inv_FalseWitness|unsafe'
)

selected=()
for entry in "${cases[@]}"; do
  case "$FAMILY:$entry" in
    all:*|audit:FinalizationLedgerAudit\|*|recovery:FinalizationRecovery\|*|effects:FinalizationEffectSelection\|*|observation:FinalizationEffectObservation\|*)
      selected+=("$entry") ;;
  esac
done
(( ${#selected[@]} > 0 ))

if [[ "$MODE" == all || "$MODE" == rocq ]]; then
  for theory in FinalizationAtomicity FinalizationLedgerAudit FinalizationEffectSelection; do
    coqc -Q "$ROCQ_ROOT/theories" FinalizedFloor "$ROCQ_ROOT/theories/$theory.v" \
      >"$RUN/rocq-$theory.log" 2>&1
  done
  coqchk -Q "$ROCQ_ROOT/theories" FinalizedFloor FinalizedFloor.FinalizationLedgerAudit \
    FinalizedFloor.FinalizationEffectSelection \
    >"$RUN/rocq-kernel.log" 2>&1
  rg -q 'Modules were successfully checked' "$RUN/rocq-kernel.log"
  printf 'PASS Rocq audit refinement and independent kernel check\n'
fi

if [[ "$MODE" == all || "$MODE" == tlc ]]; then
  for entry in "${selected[@]}"; do
    IFS='|' read -r model variant invariant expected <<<"$entry"
    name="$model$variant"
    log="$RUN/tlc-$name.log"
    if tlc_run "$RUN/tlc-$name" "$MODEL_ROOT/MC_$name.cfg" "$MODEL_ROOT/$model.tla" >"$log" 2>&1; then
      [[ "$expected" == safe ]]
      rg -q 'Model checking completed. No error has been found.' "$log"
    else
      [[ "$expected" == unsafe ]]
      rg -Fq "Invariant $invariant is violated." "$log" \
        || rg -Fq "The invariant of $invariant is equal to FALSE" "$log"
    fi
    printf 'PASS TLC %s (%s)\n' "$name" "$expected"
  done
  if [[ "$FAMILY" == all || "$FAMILY" == audit ]]; then
    tlc_run "$RUN/tlc-audit-liveness" "$MODEL_ROOT/MC_FinalizationLedgerAudit_liveness.cfg" \
      "$MODEL_ROOT/FinalizationLedgerAudit.tla" >"$RUN/tlc-audit-liveness.log" 2>&1
    rg -q 'Model checking completed. No error has been found.' "$RUN/tlc-audit-liveness.log"
    printf 'PASS TLC fair audit termination\n'
  fi
fi

if [[ "$MODE" == all || "$MODE" == apalache ]]; then
  for entry in "${selected[@]}"; do
    IFS='|' read -r model variant invariant expected <<<"$entry"
    name="$model$variant"
    config="MC_${name}_Apalache.cfg"
    if [[ "$model" == FinalizationRecovery && -z "$variant" ]]; then
      config=MC_FinalizationRecoveryApalache.cfg
    fi
    log="$RUN/apalache-$name.log"
    if (cd "$MODEL_ROOT" && apalache-mc --out-dir="$RUN/apalache-$name" check \
        --config="$config" --length=12 "$model.tla") >"$log" 2>&1; then
      [[ "$expected" == safe ]]
      rg -q 'The outcome is: NoError|EXITCODE: OK' "$log"
    else
      [[ "$expected" == unsafe ]]
      rg -Fq "Using inv predicate(s) $invariant" "$log"
      rg -q 'state invariant [0-9]+ violated' "$log"
      rg -q 'The outcome is: Error' "$log"
    fi
    printf 'PASS Apalache %s (%s)\n' "$name" "$expected"
    if [[ -z "$variant" ]]; then
      log="$RUN/apalache-$model-inductive.log"
      (cd "$MODEL_ROOT" && apalache-mc --out-dir="$RUN/apalache-$model-inductive" check \
        --config="MC_${model}_inductive_Apalache.cfg" --length=1 "$model.tla") \
        >"$log" 2>&1
      rg -q 'The outcome is: NoError|EXITCODE: OK' "$log"
      printf 'PASS Apalache %s (inductive step)\n' "$model"
    fi
  done
fi

sha256sum --check "$RUN/inputs.sha256" >"$RUN/unchanged-inputs.log"
printf 'Finalization-ledger %s verification passed (family=%s)\n' "$MODE" "$FAMILY"

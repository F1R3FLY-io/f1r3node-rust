#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MODEL_ROOT="$ROOT/formal/tlaplus/finalized_floor"
LOG_ROOT="$ROOT/target/verification/finalization-atomicity"
mkdir -p "$LOG_ROOT"
WORK="$(mktemp -d "$LOG_ROOT/run.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
export TLC_REPO_ROOT="$ROOT"
export TLC_METADIR_ROOT="$WORK"
export TLC_WORKERS=1
source "$ROOT/scripts/lib/tlc-run.sh"

for command_name in tla2sany tlc apalache-mc; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'error: %s is required for finalization atomicity verification\n' "$command_name" >&2
    exit 1
  fi
done

run_tlc_safe() {
  local name="$1" config="$2" model="$3"
  local log="$LOG_ROOT/tlc-$name.log"
  if tlc_run "$WORK/tlc-$name" "$MODEL_ROOT/$config" "$MODEL_ROOT/$model" >"$log" 2>&1; then
    grep -q 'Model checking completed. No error has been found.' "$log"
  else
    return 1
  fi
}

run_tlc_unsafe() {
  local name="$1" config="$2" model="$3" invariant="$4"
  local log="$LOG_ROOT/tlc-$name.log"
  if tlc_run "$WORK/tlc-$name" "$MODEL_ROOT/$config" "$MODEL_ROOT/$model" >"$log" 2>&1; then
    return 1
  fi
  grep -Fq "Invariant $invariant is violated." "$log" \
    || grep -Fq "The invariant of $invariant is equal to FALSE" "$log"
}

run_apalache_safe() {
  local name="$1" config="$2" model="$3" length="$4"
  local log="$LOG_ROOT/apalache-$name.log"
  if (cd "$MODEL_ROOT" && apalache-mc --out-dir="$WORK/apalache-$name" check --config="$config" --length="$length" "$model") >"$log" 2>&1; then
    grep -qE 'The outcome is: NoError|EXITCODE: OK' "$log"
  else
    return 1
  fi
}

run_apalache_unsafe() {
  local name="$1" config="$2" model="$3" length="$4" invariant="$5"
  local log="$LOG_ROOT/apalache-$name.log"
  if (cd "$MODEL_ROOT" && apalache-mc --out-dir="$WORK/apalache-$name" check --config="$config" --length="$length" "$model") >"$log" 2>&1; then
    return 1
  fi
  grep -Fq "Using inv predicate(s) $invariant" "$log" \
    && grep -qE 'state invariant [0-9]+ violated' "$log" \
    && grep -q 'The outcome is: Error' "$log"
}

tla2sany "$MODEL_ROOT/FinalizationAtomicity.tla" >"$LOG_ROOT/sany-atomicity.log" 2>&1
tla2sany "$MODEL_ROOT/FinalizationBoundHead.tla" >"$LOG_ROOT/sany-bound-head.log" 2>&1
tla2sany "$MODEL_ROOT/FinalizationRecovery.tla" >"$LOG_ROOT/sany-recovery.log" 2>&1
tla2sany "$MODEL_ROOT/FinalizationGenesisIdentity.tla" >"$LOG_ROOT/sany-genesis-identity.log" 2>&1
tla2sany "$MODEL_ROOT/FinalizationWorkerRetry.tla" >"$LOG_ROOT/sany-worker-retry.log" 2>&1
tla2sany "$MODEL_ROOT/FinalizationSnapshotRetry.tla" >"$LOG_ROOT/sany-snapshot-retry.log" 2>&1
tla2sany "$MODEL_ROOT/ProposalFloorReadiness.tla" >"$LOG_ROOT/sany-proposal-readiness.log" 2>&1
tla2sany "$MODEL_ROOT/FinalityThresholdAlignment.tla" >"$LOG_ROOT/sany-threshold-alignment.log" 2>&1
tla2sany "$MODEL_ROOT/GenesisApprovalTrust.tla" >"$LOG_ROOT/sany-genesis-approval-trust.log" 2>&1

run_tlc_safe atomicity MC_FinalizationAtomicity.cfg FinalizationAtomicity.tla
run_tlc_safe bound-head MC_FinalizationBoundHead.cfg FinalizationBoundHead.tla
run_tlc_unsafe late-bound-head MC_FinalizationBoundHead_late_bound_unsafe.cfg FinalizationBoundHead.tla Inv_AdjacentStatePreservation
run_tlc_unsafe split-commit MC_FinalizationAtomicity_split_commit_unsafe.cfg FinalizationAtomicity.tla Inv_HeadHasRecord
run_tlc_unsafe early-effect MC_FinalizationAtomicity_early_effect_unsafe.cfg FinalizationAtomicity.tla Inv_EffectsRequireCommit
run_tlc_unsafe stale-overwrite MC_FinalizationAtomicity_stale_overwrite_unsafe.cfg FinalizationAtomicity.tla Inv_RecordPrefix
run_tlc_unsafe regressive-publish MC_FinalizationAtomicity_regressive_publish_unsafe.cfg FinalizationAtomicity.tla Inv_PublicationMonotonic
run_tlc_unsafe lost-wake MC_FinalizationAtomicity_lost_wake_unsafe.cfg FinalizationAtomicity.tla Inv_NoLostWake
run_tlc_safe worker-retry MC_FinalizationWorkerRetry.cfg FinalizationWorkerRetry.tla
run_tlc_safe snapshot-retry MC_FinalizationSnapshotRetry.cfg FinalizationSnapshotRetry.tla
run_tlc_unsafe stale-snapshot-publish MC_FinalizationSnapshotRetry_stale_publish_unsafe.cfg FinalizationSnapshotRetry.tla Inv_ReaderResultCoherent
run_tlc_unsafe failure-completes MC_FinalizationWorkerRetry_failure_completes_unsafe.cfg FinalizationWorkerRetry.tla Inv_CompletionRequiresSuccess
run_tlc_safe proposal-readiness MC_ProposalFloorReadiness.cfg ProposalFloorReadiness.tla
run_tlc_unsafe proposal-pending-no-request MC_ProposalFloorReadiness_pending_no_request_unsafe.cfg ProposalFloorReadiness.tla Inv_FloorPendingRequestsFinalization
run_tlc_unsafe proposal-nonfloor-request MC_ProposalFloorReadiness_nonfloor_request_unsafe.cfg ProposalFloorReadiness.tla Inv_NonFloorDeferralDoesNotRequest
run_tlc_unsafe proposal-bypass MC_ProposalFloorReadiness_bypass_unsafe.cfg ProposalFloorReadiness.tla Inv_CreationRequiresReadyContext
run_tlc_unsafe proposal-equality-only MC_ProposalFloorReadiness_equality_only_unsafe.cfg ProposalFloorReadiness.tla Inv_FloorPendingIsStrictStatePreserving
run_tlc_unsafe proposal-nonstrict MC_ProposalFloorReadiness_nonstrict_unsafe.cfg ProposalFloorReadiness.tla Inv_OnlyStrictStatePreservingFloorsMaterialize
run_tlc_unsafe proposal-state-regressive MC_ProposalFloorReadiness_state_regressive_unsafe.cfg ProposalFloorReadiness.tla Inv_OnlyStrictStatePreservingFloorsMaterialize
run_tlc_safe threshold-alignment MC_FinalityThresholdAlignment.cfg FinalityThresholdAlignment.tla
run_tlc_unsafe threshold-inclusive MC_FinalityThresholdAlignment_inclusive_unsafe.cfg FinalityThresholdAlignment.tla CandidateAndFinalizerAgree
run_tlc_safe genesis-approval-trust MC_GenesisApprovalTrust.cfg GenesisApprovalTrust.tla
run_tlc_unsafe genesis-approval-local-count MC_GenesisApprovalTrust_local_count_unsafe.cfg GenesisApprovalTrust.tla Inv_InstalledIsProtocolAuthorized
run_tlc_unsafe genesis-approval-downgrade MC_GenesisApprovalTrust_downgrade_unsafe.cfg GenesisApprovalTrust.tla Inv_InstalledIsProtocolAuthorized
run_tlc_unsafe genesis-approval-invalid-count MC_GenesisApprovalTrust_invalid_count_unsafe.cfg GenesisApprovalTrust.tla Inv_InstalledIsProtocolAuthorized
run_tlc_unsafe genesis-approval-reject-mutation MC_GenesisApprovalTrust_reject_mutation_unsafe.cfg GenesisApprovalTrust.tla Inv_RejectionDoesNotMutate

run_tlc_safe genesis-identity MC_FinalizationGenesisIdentity.cfg FinalizationGenesisIdentity.tla
run_tlc_unsafe genesis-reset MC_FinalizationGenesisIdentity_reset_unsafe.cfg FinalizationGenesisIdentity.tla Inv_HeadMonotonic
run_tlc_unsafe genesis-conflict MC_FinalizationGenesisIdentity_conflict_unsafe.cfg FinalizationGenesisIdentity.tla Inv_CanonicalGenesis
run_tlc_unsafe genesis-split MC_FinalizationGenesisIdentity_split_unsafe.cfg FinalizationGenesisIdentity.tla Inv_AtomicBootstrap
run_tlc_unsafe genesis-backfill MC_FinalizationGenesisIdentity_backfill_unsafe.cfg FinalizationGenesisIdentity.tla Inv_HeadRooted
run_tlc_unsafe genesis-missing-mapping MC_FinalizationGenesisIdentity_mapping_unsafe.cfg FinalizationGenesisIdentity.tla Inv_ConstructedHasLedgerStore

run_tlc_safe recovery MC_FinalizationRecovery.cfg FinalizationRecovery.tla
run_tlc_unsafe projection-gap MC_FinalizationRecovery_projection_gap_unsafe.cfg FinalizationRecovery.tla Inv_ProjectionPrefix
run_tlc_unsafe recovery-early-effect MC_FinalizationRecovery_early_effect_unsafe.cfg FinalizationRecovery.tla Inv_EffectsAfterProjection
run_tlc_unsafe effect-cursor-gap MC_FinalizationRecovery_effect_gap_unsafe.cfg FinalizationRecovery.tla Inv_EffectsCursorPrefix

run_apalache_safe atomicity MC_FinalizationAtomicityApalache.cfg FinalizationAtomicity.tla "${FINALIZATION_ATOMICITY_APALACHE_LENGTH:-10}"
run_apalache_safe worker-retry MC_FinalizationWorkerRetryApalache.cfg FinalizationWorkerRetry.tla "${FINALIZATION_WORKER_RETRY_APALACHE_LENGTH:-12}"
run_apalache_safe snapshot-retry MC_FinalizationSnapshotRetryApalache.cfg FinalizationSnapshotRetry.tla "${FINALIZATION_SNAPSHOT_RETRY_APALACHE_LENGTH:-8}"
run_apalache_unsafe stale-snapshot-publish MC_FinalizationSnapshotRetry_stale_publish_unsafe_Apalache.cfg FinalizationSnapshotRetry.tla 5 Inv_ReaderResultCoherent
run_apalache_unsafe failure-completes MC_FinalizationWorkerRetry_failure_completes_unsafe_Apalache.cfg FinalizationWorkerRetry.tla 4 Inv_CompletionRequiresSuccess
run_apalache_safe proposal-readiness MC_ProposalFloorReadinessApalache.cfg ProposalFloorReadiness.tla "${PROPOSAL_FLOOR_READINESS_APALACHE_LENGTH:-8}"
run_apalache_unsafe proposal-pending-no-request MC_ProposalFloorReadiness_pending_no_request_unsafe_Apalache.cfg ProposalFloorReadiness.tla 2 Inv_FloorPendingRequestsFinalization
run_apalache_unsafe proposal-nonfloor-request MC_ProposalFloorReadiness_nonfloor_request_unsafe_Apalache.cfg ProposalFloorReadiness.tla 2 Inv_NonFloorDeferralDoesNotRequest
run_apalache_unsafe proposal-bypass MC_ProposalFloorReadiness_bypass_unsafe_Apalache.cfg ProposalFloorReadiness.tla 2 Inv_CreationRequiresReadyContext
run_apalache_unsafe proposal-equality-only MC_ProposalFloorReadiness_equality_only_unsafe_Apalache.cfg ProposalFloorReadiness.tla 3 Inv_FloorPendingIsStrictStatePreserving
run_apalache_unsafe proposal-nonstrict MC_ProposalFloorReadiness_nonstrict_unsafe_Apalache.cfg ProposalFloorReadiness.tla 4 Inv_OnlyStrictStatePreservingFloorsMaterialize
run_apalache_unsafe proposal-state-regressive MC_ProposalFloorReadiness_state_regressive_unsafe_Apalache.cfg ProposalFloorReadiness.tla 4 Inv_OnlyStrictStatePreservingFloorsMaterialize
run_apalache_safe threshold-alignment MC_FinalityThresholdAlignmentApalache.cfg FinalityThresholdAlignment.tla 1
run_apalache_unsafe threshold-inclusive MC_FinalityThresholdAlignment_inclusive_unsafe_Apalache.cfg FinalityThresholdAlignment.tla 1 CandidateAndFinalizerAgree
run_apalache_safe genesis-approval-trust MC_GenesisApprovalTrustApalache.cfg GenesisApprovalTrust.tla 1
run_apalache_unsafe genesis-approval-local-count MC_GenesisApprovalTrust_local_count_unsafe_Apalache.cfg GenesisApprovalTrust.tla 1 Inv_InstalledIsProtocolAuthorized
run_apalache_unsafe genesis-approval-downgrade MC_GenesisApprovalTrust_downgrade_unsafe_Apalache.cfg GenesisApprovalTrust.tla 1 Inv_InstalledIsProtocolAuthorized
run_apalache_unsafe genesis-approval-invalid-count MC_GenesisApprovalTrust_invalid_count_unsafe_Apalache.cfg GenesisApprovalTrust.tla 1 Inv_InstalledIsProtocolAuthorized
run_apalache_unsafe genesis-approval-reject-mutation MC_GenesisApprovalTrust_reject_mutation_unsafe_Apalache.cfg GenesisApprovalTrust.tla 1 Inv_RejectionDoesNotMutate
run_apalache_safe genesis-identity MC_FinalizationGenesisIdentityApalache.cfg FinalizationGenesisIdentity.tla "${FINALIZATION_GENESIS_IDENTITY_APALACHE_LENGTH:-6}"
run_apalache_unsafe genesis-reset MC_FinalizationGenesisIdentity_reset_unsafe_Apalache.cfg FinalizationGenesisIdentity.tla 4 Inv_HeadMonotonic
run_apalache_unsafe genesis-conflict MC_FinalizationGenesisIdentity_conflict_unsafe_Apalache.cfg FinalizationGenesisIdentity.tla 3 Inv_CanonicalGenesis
run_apalache_unsafe genesis-split MC_FinalizationGenesisIdentity_split_unsafe_Apalache.cfg FinalizationGenesisIdentity.tla 1 Inv_AtomicBootstrap
run_apalache_unsafe genesis-backfill MC_FinalizationGenesisIdentity_backfill_unsafe_Apalache.cfg FinalizationGenesisIdentity.tla 1 Inv_HeadRooted
run_apalache_unsafe genesis-missing-mapping MC_FinalizationGenesisIdentity_mapping_unsafe_Apalache.cfg FinalizationGenesisIdentity.tla 1 Inv_ConstructedHasLedgerStore
run_apalache_safe bound-head MC_FinalizationBoundHeadApalache.cfg FinalizationBoundHead.tla "${FINALIZATION_BOUND_HEAD_APALACHE_LENGTH:-6}"
run_apalache_unsafe late-bound-head MC_FinalizationBoundHead_late_bound_unsafe_Apalache.cfg FinalizationBoundHead.tla 6 Inv_AdjacentStatePreservation
run_apalache_safe recovery MC_FinalizationRecoveryApalache.cfg FinalizationRecovery.tla "${FINALIZATION_RECOVERY_APALACHE_LENGTH:-12}"
run_apalache_unsafe projection-gap MC_FinalizationRecovery_projection_gap_unsafe_Apalache.cfg FinalizationRecovery.tla 6 Inv_ProjectionPrefix
run_apalache_unsafe recovery-early-effect MC_FinalizationRecovery_early_effect_unsafe_Apalache.cfg FinalizationRecovery.tla 4 Inv_EffectsAfterProjection
run_apalache_unsafe effect-cursor-gap MC_FinalizationRecovery_effect_gap_unsafe_Apalache.cfg FinalizationRecovery.tla 10 Inv_EffectsCursorPrefix

printf 'finalization atomicity and crash-recovery verification passed\n'

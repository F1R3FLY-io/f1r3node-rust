#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export TLC_REPO_ROOT="$repo_root"
export TLC_HEAP="${TLC_HEAP:-512m}"
export TLC_WORKERS="${TLC_WORKERS:-2}"
export TLC_MEMORY_HIGH="${TLC_MEMORY_HIGH:-1G}"
export TLC_RSS="${TLC_RSS:-2G}"
export TLC_CPU_QUOTA="${TLC_CPU_QUOTA:-200%}"
export TLC_TASKS_MAX="${TLC_TASKS_MAX:-64}"
export TLC_RUNTIME_MAX_SEC="${TLC_RUNTIME_MAX_SEC:-120}"
source "$repo_root/scripts/lib/tlc-run.sh"

cd "$repo_root/formal/tlaplus/stack_safe_pda"
metadir="$(tlc_metadir rspace-journal-owner)"
model=RSpaceJournalPublicationOwner
tlc_run "$metadir" "$model.cfg" "$model.tla" -cleanup
for control in \
  "ForeignReceiptMutant:ExactPendingReceipt" \
  "EarlyAckMutant:AcknowledgementRequiresExactSuccessor" \
  "PartialClearMutant:WholeJournalAcknowledgement" \
  "LeaseGapMutant:ContinuousAuthority" \
  "UnresolvedRebaseMutant:ReadyHasCoherentBase" \
  "DoublePublishMutant:AtMostOnePublication" \
  "GateReleaseMutant:GateHeldUntilResolution" \
  "WrongRecoveryMutant:RecoveryAcceptsOnlyExactTerminal" \
  "LostAckMarkerMutant:ClearedPendingJournalHasExactMarker" \
  "AckMarkerCorruptionMutant:AcknowledgementRequiresExactReceipt" \
  "WrongAckInputMutant:AcknowledgementAcceptsOnlyExactInput" \
  "DoubleAckMutant:AtMostOneAcknowledgement" \
  "WrongPreparedWitnessMutant:PreparedWitnessBound" \
  "UnwindGateReleaseMutant:GateHeldUntilResolution" \
  "CancelGateReleaseMutant:GateHeldUntilResolution" \
  "ColdLimitIgnoredMutant:ColdRecoveryHonorsIncomingLimits" \
  "FabricatedColdMutant:TrustedSuccessorHasNoColdEvidence" \
  "JournalReplayMutant:ReopenDiscardsVolatileJournal" \
  "PathnameRecoveryMutant:RecoveryAcceptsOnlyExactTerminal" \
  "NeverAcknowledged:NeverAcknowledged" \
  "NeverRecoveredOld:NeverRecoveredOld" \
  "NeverRecoveredNew:NeverRecoveredNew" \
  "NeverRejectedBinding:NeverRejectedBinding" \
  "NeverAbortedCapture:NeverAbortedCapture" \
  "NeverUnwound:NeverUnwound" \
  "NeverCancelled:NeverCancelled" \
  "NeverRetriedAcknowledgementAfterUnwind:NeverRetriedAcknowledgementAfterUnwind" \
  "NeverEmptyCheckpointUnderTightenedColdLimits:NeverEmptyCheckpointUnderTightenedColdLimits" \
  "NeverAwaitingAckRetryUnderTightenedColdLimits:NeverAwaitingAckRetryUnderTightenedColdLimits" \
  "NeverReconciledUnderDecoy:NeverReconciledUnderDecoy"
do
  config="${control%%:*}"
  invariant="${control#*:}"
  if output="$(tlc_run "$metadir" "$model$config.cfg" "$model.tla" -cleanup 2>&1)"; then
    printf '%s\n' "$output"
    echo "journal-owner: expected counterexample was absent: $config" >&2
    exit 1
  fi
  printf '%s\n' "$output"
  if ! rg -q "Invariant ${invariant} is violated" <<<"$output"; then
    echo "journal-owner: $config failed without its expected invariant: $invariant" >&2
    exit 1
  fi
done
view_output="$(tlc_run "$metadir" "${model}Views.cfg" "$model.tla" -cleanup 2>&1)" || {
  printf '%s\n' "$view_output"
  echo "journal-owner: the view-gate control failed" >&2
  exit 1
}
printf '%s\n' "$view_output"
if ! rg -q 'temporal properties for the complete state space' <<<"$view_output"; then
  echo "journal-owner: the view-gate control did not check PendingResolves" >&2
  exit 1
fi
for control in \
  "RetakeGateMutant:Invariant LockOrder is violated" \
  "RetakeGateDeadlockMutant:Deadlock reached" \
  "CaptureAtViewBaseMutant:Invariant CapturedPredecessorIsOwnerRoot is violated" \
  "PublishOnCurrentMutant:Invariant PublishedSuccessorIsOfViewBase is violated" \
  "ResetMovesOwnerMutant:Invariant OwnerRootIsLastPublication is violated" \
  "SpawnCopiesBaseMutant:Action property SpawnSharesOwner is violated" \
  "ForeignPermitMutant:Invariant PermitProvenance is violated" \
  "CloseNoBumpMutant:Invariant StaleIncarnationRefused is violated" \
  "ReopenReusesIncarnationMutant:Invariant StaleIncarnationRefused is violated" \
  "NeverPublishedAcrossBases:Invariant NeverPublishedAcrossBases is violated" \
  "NeverRefusedRetainedIncarnation:Invariant NeverRefusedRetainedIncarnation is violated" \
  "NeverReconciledByOtherView:Invariant NeverReconciledByOtherView is violated"
do
  config="${control%%:*}"
  expected="${control#*:}"
  if output="$(tlc_run "$metadir" "$model$config.cfg" "$model.tla" -cleanup 2>&1)"; then
    printf '%s\n' "$output"
    echo "journal-owner: expected counterexample was absent: $config" >&2
    exit 1
  fi
  printf '%s\n' "$output"
  if ! rg -qF "$expected" <<<"$output"; then
    echo "journal-owner: $config failed without its expected text: $expected" >&2
    exit 1
  fi
done
echo "journal-owner: control, view-gate control with liveness, twenty-seven causal mutants (nineteen plus eight) and fourteen reachability witnesses (eleven plus three) passed"

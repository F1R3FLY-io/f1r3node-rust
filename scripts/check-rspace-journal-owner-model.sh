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
  "NeverAcknowledged:NeverAcknowledged" \
  "NeverRecoveredOld:NeverRecoveredOld" \
  "NeverRecoveredNew:NeverRecoveredNew" \
  "NeverRejectedBinding:NeverRejectedBinding" \
  "NeverAbortedCapture:NeverAbortedCapture" \
  "NeverUnwound:NeverUnwound" \
  "NeverCancelled:NeverCancelled" \
  "NeverRetriedAcknowledgementAfterUnwind:NeverRetriedAcknowledgementAfterUnwind" \
  "NeverEmptyCheckpointUnderTightenedColdLimits:NeverEmptyCheckpointUnderTightenedColdLimits" \
  "NeverAwaitingAckRetryUnderTightenedColdLimits:NeverAwaitingAckRetryUnderTightenedColdLimits"
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
echo "journal-owner: control, seventeen causal mutants and ten reachability witnesses passed"

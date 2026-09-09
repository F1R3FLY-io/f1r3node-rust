#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
publication_mode=${1:-all}
case "$publication_mode" in all|tla|rocq|handoff|boundaries|provenance|promotion|durable|retry-operations|owner-registry|deferred-completion|retry-selection|retry-renewal|retry-entry|retry-traversal|accepted-custody|retry-custody|retry-maintenance|local-activation|row-guard) ;; *) exit 2 ;; esac
publication_cgroup=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
publication_memory=$(<"/sys/fs/cgroup$publication_cgroup/memory.max")
publication_swap=$(<"/sys/fs/cgroup$publication_cgroup/memory.swap.max")
if [[ ! "$publication_memory" =~ ^[1-9][0-9]*$ ]] \
   || (( publication_memory > 2147483648 )) || [[ "$publication_swap" != 0 ]]; then
  echo 'Use a systemd scope with MemoryMax at most 2 GiB and MemorySwapMax=0.' >&2
  exit 2
fi
mkdir -p target/verification/buffer-publication
publication_evidence=$(mktemp -d "$PWD/target/verification/buffer-publication/run.XXXXXX")
mkdir -p "$publication_evidence/tmp"
export TMPDIR="$publication_evidence/tmp"
printf 'Evidence: %s\nMemoryMax: %s\n' "$publication_evidence" "$publication_memory"
sha256sum scripts/check-buffer-publication-ownership.sh \
  formal/tlaplus/block_admission/BufferPublicationOwnership* \
  formal/tlaplus/block_admission/BufferRetryHandoff* \
  formal/tlaplus/block_admission/BufferHandoffBoundaries* \
  formal/tlaplus/block_admission/BufferPendingProvenance* \
  formal/tlaplus/block_admission/DependencyRequestProvenance* \
  formal/tlaplus/block_admission/DurablePendingHandoff* \
  formal/tlaplus/block_admission/RetryOperationOwnership* \
  formal/tlaplus/block_admission/PendingOwnerRegistry* \
  formal/tlaplus/block_admission/DeferredRetryCompletion* \
  formal/tlaplus/block_admission/RetrySelectionPolicy* \
  formal/tlaplus/block_admission/RetryBudgetRenewal* \
  formal/tlaplus/block_admission/RetryQuarantineEntry* \
  formal/tlaplus/block_admission/PendingPolicyRowGuard* \
  formal/tlaplus/block_admission/BufferIndependentExpiry* \
  formal/tlaplus/block_admission/AcceptedPublicationCustody* \
  formal/tlaplus/block_admission/RetryBudgetCustody* \
  formal/tlaplus/block_admission/RetryMaintenanceOrder* \
  formal/tlaplus/block_admission/LocalRequestActivation* \
  formal/rocq/finalized_floor/theories/BufferPublicationOwnership.v \
  formal/rocq/finalized_floor/theories/BufferRetryHandoff.v \
  formal/rocq/finalized_floor/theories/BufferHandoffBoundaries.v \
  formal/rocq/finalized_floor/theories/DependencyRequestProvenance.v \
  formal/rocq/finalized_floor/theories/RetryOperationOwnership.v \
  > "$publication_evidence/inputs.sha256"
publication_finish() {
  local publication_result=$?
  trap - EXIT
  if ! sha256sum --check "$publication_evidence/inputs.sha256" > "$publication_evidence/inputs-check.log"; then
    tail -n 25 "$publication_evidence/inputs-check.log"
    publication_result=1
  fi
  printf 'Gate exit: %s\n' "$publication_result"
  exit "$publication_result"
}
trap publication_finish EXIT
if [[ "$publication_mode" == all || "$publication_mode" == tla || "$publication_mode" == local-activation ]]; then
for publication_variant in '' ReasonUnsafe LateUnsafe ErrorUnsafe MutationUnsafe MetricUnsafe LeaseUnsafe; do
  publication_name="LocalRequestActivation$publication_variant"
  mkdir -p "$publication_evidence/$publication_name"
  set +e
  timeout --signal=TERM --kill-after=10s 180s \
    java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
      -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
      -metadir "$publication_evidence/$publication_name" \
      -config "formal/tlaplus/block_admission/$publication_name.cfg" \
      formal/tlaplus/block_admission/LocalRequestActivation.tla \
      > "$publication_evidence/$publication_name.log" 2>&1
  publication_result=$?
  set -e
  tail -n 10 "$publication_evidence/$publication_name.log"
  case "$publication_variant" in
    ReasonUnsafe|LateUnsafe|ErrorUnsafe) publication_expected=Inv_Reason ;;
    MutationUnsafe) publication_expected=Inv_Preserved ;;
    MetricUnsafe) publication_expected=Inv_Metric ;;
    LeaseUnsafe) publication_expected=Inv_Lease ;;
    *) publication_expected='' ;;
  esac
  if [[ -n "$publication_expected" ]]; then
    [[ "$publication_result" == 12 ]]
    rg "Invariant $publication_expected is violated" "$publication_evidence/$publication_name.log"
  else
    [[ "$publication_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$publication_evidence/$publication_name.log"
  fi
done
fi
if [[ "$publication_mode" == all || "$publication_mode" == tla || "$publication_mode" == retry-maintenance ]]; then
for publication_variant in '' ExpiryUnsafe ResetUnsafe ClockUnsafe LateClockUnsafe SnapshotUnsafe CurrentDueUnsupported; do
  publication_name="RetryMaintenanceOrder$publication_variant"
  mkdir -p "$publication_evidence/$publication_name"
  set +e
  timeout --signal=TERM --kill-after=10s 180s \
    java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
      -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
      -metadir "$publication_evidence/$publication_name" \
      -config "formal/tlaplus/block_admission/$publication_name.cfg" \
      formal/tlaplus/block_admission/RetryMaintenanceOrder.tla \
      > "$publication_evidence/$publication_name.log" 2>&1
  publication_result=$?
  set -e
  tail -n 10 "$publication_evidence/$publication_name.log"
  case "$publication_variant" in
    ExpiryUnsafe) publication_expected=Inv_Order ;;
    ResetUnsafe) publication_expected=Inv_Decision ;;
    ClockUnsafe|LateClockUnsafe) publication_expected=Inv_Clock ;;
    SnapshotUnsafe) publication_expected=Inv_Snapshot ;;
    CurrentDueUnsupported) publication_expected=Inv_CurrentDue ;;
    *) publication_expected='' ;;
  esac
  if [[ -n "$publication_expected" ]]; then
    [[ "$publication_result" == 12 ]]
    rg "Invariant $publication_expected is violated" "$publication_evidence/$publication_name.log"
  else
    [[ "$publication_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$publication_evidence/$publication_name.log"
  fi
done
fi
if [[ "$publication_mode" == all || "$publication_mode" == tla || "$publication_mode" == retry-custody ]]; then
for publication_variant in '' DuplicateUnsafe PendingUnsafe ProvenanceUnsafe ScheduleUnsafe ActivationUnsafe OverwriteUnsafe FailureUnsafe ProbeUnsafe ExpiryUnsafe CapacityUnsafe DeadlineUnsafe FailureProvenanceUnsafe; do
  publication_name="RetryBudgetCustody$publication_variant"
  mkdir -p "$publication_evidence/$publication_name"
  set +e
  timeout --signal=TERM --kill-after=10s 180s \
    java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
      -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
      -metadir "$publication_evidence/$publication_name" \
      -config "formal/tlaplus/block_admission/$publication_name.cfg" \
      formal/tlaplus/block_admission/RetryBudgetCustody.tla \
      > "$publication_evidence/$publication_name.log" 2>&1
  publication_result=$?
  set -e
  tail -n 10 "$publication_evidence/$publication_name.log"
  case "$publication_variant" in
    DuplicateUnsafe) publication_expected=Inv_SingleBudget ;;
    PendingUnsafe|OverwriteUnsafe) publication_expected=Inv_PendingProvenance ;;
    ProvenanceUnsafe|ScheduleUnsafe) publication_expected=Inv_Retired ;;
    ActivationUnsafe) publication_expected=Inv_Activation ;;
    FailureUnsafe|CapacityUnsafe) publication_expected=Inv_Budget ;;
    ProbeUnsafe) publication_expected=Inv_NoProbe ;;
    ExpiryUnsafe) publication_expected=Inv_ActiveSchedule ;;
    DeadlineUnsafe) publication_expected=Inv_Deadline ;;
    FailureProvenanceUnsafe) publication_expected=Inv_Failure ;;
    *) publication_expected='' ;;
  esac
  if [[ -n "$publication_expected" ]]; then
    [[ "$publication_result" == 12 ]]
    rg "Invariant $publication_expected is violated" "$publication_evidence/$publication_name.log"
  else
    [[ "$publication_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$publication_evidence/$publication_name.log"
  fi
done
fi
if [[ "$publication_mode" == all || "$publication_mode" == tla || "$publication_mode" == accepted-custody ]]; then
for publication_variant in '' UncheckedUnsafe ReleaseUnsafe CaptureUnsafe RecaptureUnsafe OverwriteUnsafe HashUnsafe ContextUnsafe TerminalUnsafe MetadataUnsafe RestorationUnsafe; do
  publication_name="AcceptedPublicationCustody$publication_variant"
  mkdir -p "$publication_evidence/$publication_name"
  set +e
  timeout --signal=TERM --kill-after=10s 180s \
    java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
      -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
      -metadir "$publication_evidence/$publication_name" \
      -config "formal/tlaplus/block_admission/$publication_name.cfg" \
      formal/tlaplus/block_admission/AcceptedPublicationCustody.tla \
      > "$publication_evidence/$publication_name.log" 2>&1
  publication_result=$?
  set -e
  tail -n 10 "$publication_evidence/$publication_name.log"
  case "$publication_variant" in
    UncheckedUnsafe|RestorationUnsafe) publication_expected=Inv_Verified ;;
    ReleaseUnsafe) publication_expected=Inv_Custody ;;
    CaptureUnsafe) publication_expected=Inv_OldCapture ;;
    RecaptureUnsafe|OverwriteUnsafe) publication_expected=Inv_Provenance ;;
    HashUnsafe) publication_expected=Inv_ExactTarget ;;
    ContextUnsafe) publication_expected=Inv_Context ;;
    TerminalUnsafe|MetadataUnsafe) publication_expected=Inv_Terminal ;;
    *) publication_expected='' ;;
  esac
  if [[ -n "$publication_expected" ]]; then
    [[ "$publication_result" == 12 ]]
    rg "Invariant $publication_expected is violated" "$publication_evidence/$publication_name.log"
  else
    [[ "$publication_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$publication_evidence/$publication_name.log"
  fi
done
fi
if [[ "$publication_mode" == all || "$publication_mode" == tla || "$publication_mode" == retry-traversal ]]; then
for publication_variant in '' SharedUnsafe ArrivalUnsafe DuplicateUnsafe RemovalUnsafe ReturnUnsafe SkipUnsafe OverlapUnsafe; do
  publication_name="BufferIndependentExpiry$publication_variant"
  mkdir -p "$publication_evidence/$publication_name"
  set +e
  timeout --signal=TERM --kill-after=10s 180s \
    java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
      -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
      -metadir "$publication_evidence/$publication_name" \
      -config "formal/tlaplus/block_admission/$publication_name.cfg" \
      formal/tlaplus/block_admission/BufferIndependentExpiry.tla \
      > "$publication_evidence/$publication_name.log" 2>&1
  publication_result=$?
  set -e
  tail -n 10 "$publication_evidence/$publication_name.log"
  case "$publication_variant" in
    SharedUnsafe|ArrivalUnsafe|DuplicateUnsafe) publication_expected=Inv_IndependentOrder ;;
    RemovalUnsafe) publication_expected=Inv_Membership ;;
    ReturnUnsafe) publication_expected=Inv_CompleteReturn ;;
    SkipUnsafe) publication_expected=Inv_PassCoverage ;;
    OverlapUnsafe) publication_expected=Inv_SerializedPasses ;;
    *) publication_expected='' ;;
  esac
  if [[ -n "$publication_expected" ]]; then
    [[ "$publication_result" == 12 ]]
    rg "Invariant $publication_expected is violated" "$publication_evidence/$publication_name.log"
  else
    [[ "$publication_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$publication_evidence/$publication_name.log"
  fi
done
fi
if [[ "$publication_mode" == all || "$publication_mode" == tla || "$publication_mode" == retry-entry ]]; then
for publication_variant in '' Concurrent OverreserveUnsafe EarlyUnsafe TotalResetUnsafe PeerUnsafe ScheduleUnsafe UnderwriteUnsafe ProvenanceUnsafe FailureUnsafe FailedCountUnsafe FailedPeerUnsafe FailedDiskUnsafe FailedCompletionUnsafe; do
  publication_name="RetryQuarantineEntry$publication_variant"
  mkdir -p "$publication_evidence/$publication_name"
  set +e
  timeout --signal=TERM --kill-after=10s 180s \
    java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
      -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
      -metadir "$publication_evidence/$publication_name" \
      -config "formal/tlaplus/block_admission/$publication_name.cfg" \
      formal/tlaplus/block_admission/RetryQuarantineEntry.tla \
      > "$publication_evidence/$publication_name.log" 2>&1
  publication_result=$?
  set -e
  tail -n 10 "$publication_evidence/$publication_name.log"
  case "$publication_variant" in
    OverreserveUnsafe) publication_expected=Inv_ReservationBound ;;
    TotalResetUnsafe) publication_expected=Inv_Accounting ;;
    FailureUnsafe|Failed*Unsafe) publication_expected=Inv_FailedEntry ;;
    *Unsafe) publication_expected=Inv_EntryProjection ;;
    *) publication_expected='' ;;
  esac
  if [[ -n "$publication_expected" ]]; then
    [[ "$publication_result" == 12 ]]
    rg "Invariant $publication_expected is violated" "$publication_evidence/$publication_name.log"
  else
    [[ "$publication_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$publication_evidence/$publication_name.log"
  fi
done
fi
if [[ "$publication_mode" == all || "$publication_mode" == tla || "$publication_mode" == row-guard ]]; then
for publication_variant in '' ReencodedUnsafe; do
  publication_name="PendingPolicyRowGuard$publication_variant"
  mkdir -p "$publication_evidence/$publication_name"
  set +e
  timeout --signal=TERM --kill-after=10s 180s \
    java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
      -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
      -metadir "$publication_evidence/$publication_name" \
      -config "formal/tlaplus/block_admission/$publication_name.cfg" \
      formal/tlaplus/block_admission/PendingPolicyRowGuard.tla \
      > "$publication_evidence/$publication_name.log" 2>&1
  publication_result=$?
  set -e
  tail -n 10 "$publication_evidence/$publication_name.log"
  if [[ -n "$publication_variant" ]]; then
    [[ "$publication_result" == 12 ]]
    rg 'Invariant Inv_ExactRowGuard is violated' "$publication_evidence/$publication_name.log"
  else
    [[ "$publication_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$publication_evidence/$publication_name.log"
  fi
done
fi
if [[ "$publication_mode" == all || "$publication_mode" == tla || "$publication_mode" == retry-renewal ]]; then
for publication_variant in '' Volatile IndependentHashes ConcurrentHashes TwoCycles Unknown UnknownConcurrent EarlyUnsafe WallClockUnsafe StaleUnsafe \
  ProvenanceUnsafe TokensUnsafe PartialFailureUnsafe FailedTokensUnsafe FailedMetadataUnsafe \
  CancelledCompletionUnsafe ProbeUnsafe OtherHashUnsafe UnknownResetUnsafe UnknownTokensUnsafe; do
  publication_name="RetryBudgetRenewal$publication_variant"
  mkdir -p "$publication_evidence/$publication_name"
  set +e
  timeout --signal=TERM --kill-after=10s 180s \
    java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
      -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
      -metadir "$publication_evidence/$publication_name" \
      -config "formal/tlaplus/block_admission/$publication_name.cfg" \
      formal/tlaplus/block_admission/RetryBudgetRenewal.tla \
      > "$publication_evidence/$publication_name.log" 2>&1
  publication_result=$?
  set -e
  tail -n 10 "$publication_evidence/$publication_name.log"
  case "$publication_variant" in
    EarlyUnsafe|WallClockUnsafe) publication_expected=Inv_SweepTime ;;
    StaleUnsafe) publication_expected=Inv_FreshRenewal ;;
    ProvenanceUnsafe) publication_expected=Inv_Provenance ;;
    TokensUnsafe) publication_expected=Inv_CurrentTokens ;;
    PartialFailureUnsafe|FailedTokensUnsafe|FailedMetadataUnsafe) publication_expected=Inv_AtomicFailure ;;
    CancelledCompletionUnsafe|OtherHashUnsafe) publication_expected=Inv_CycleAccounting ;;
    ProbeUnsafe) publication_expected=Inv_NoAutonomousProbe ;;
    UnknownResetUnsafe|UnknownTokensUnsafe) publication_expected=Inv_UnconfirmedOwner ;;
    *) publication_expected='' ;;
  esac
  if [[ -n "$publication_expected" ]]; then
    [[ "$publication_result" == 12 ]]
    rg "Invariant $publication_expected is violated" "$publication_evidence/$publication_name.log"
  else
    [[ "$publication_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$publication_evidence/$publication_name.log"
  fi
done
fi
if [[ "$publication_mode" == all || "$publication_mode" == tla || "$publication_mode" == retry-selection ]]; then
for publication_variant in '' Volatile NoPeers ExpiryClockUnsafe InclusiveClockUnsafe DropSuppressedUnsafe \
  SkipBudgetCursorUnsafe PartialFailureUnsafe SuppressedPermitUnsafe LateActionMetricUnsafe DuplicateCompletionMetricUnsafe \
  StaleEpochUnsafe StaleRevisionUnsafe; do
  publication_name="RetrySelectionPolicy$publication_variant"
  mkdir -p "$publication_evidence/$publication_name"
  set +e
  timeout --signal=TERM --kill-after=10s 180s \
    java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
      -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
      -metadir "$publication_evidence/$publication_name" \
      -config "formal/tlaplus/block_admission/$publication_name.cfg" \
      formal/tlaplus/block_admission/RetrySelectionPolicy.tla \
      > "$publication_evidence/$publication_name.log" 2>&1
  publication_result=$?
  set -e
  tail -n 10 "$publication_evidence/$publication_name.log"
  case "$publication_variant" in
    ExpiryClockUnsafe|InclusiveClockUnsafe) publication_expected=Inv_ClockEquivalent ;;
    DropSuppressedUnsafe|SkipBudgetCursorUnsafe|PartialFailureUnsafe) publication_expected=Inv_PolicyCorrespondence ;;
    SuppressedPermitUnsafe) publication_expected=Inv_OperationOwnership ;;
    LateActionMetricUnsafe|DuplicateCompletionMetricUnsafe) publication_expected=Inv_MetricBoundary ;;
    StaleEpochUnsafe|StaleRevisionUnsafe) publication_expected=Inv_NoStalePublication ;;
    *) publication_expected='' ;;
  esac
  if [[ -n "$publication_expected" ]]; then
    [[ "$publication_result" == 12 ]]
    rg "Invariant $publication_expected is violated" "$publication_evidence/$publication_name.log"
  else
    [[ "$publication_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$publication_evidence/$publication_name.log"
  fi
done
fi
if [[ "$publication_mode" == all || "$publication_mode" == tla ]]; then
for publication_variant in AckUnsafe CacheUnsafe ReleaseUnsafe EvictUnsafe RestartUnsafe '' ZeroCache; do
  publication_name="BufferPublicationOwnership$publication_variant"
  mkdir -p "$publication_evidence/$publication_name"
  set +e
  timeout --signal=TERM --kill-after=10s 180s \
    java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
      -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
      -metadir "$publication_evidence/$publication_name" \
      -config "formal/tlaplus/block_admission/$publication_name.cfg" \
      formal/tlaplus/block_admission/BufferPublicationOwnership.tla \
      > "$publication_evidence/$publication_name.log" 2>&1
  publication_result=$?
  set -e
  tail -n 10 "$publication_evidence/$publication_name.log"
  case "$publication_variant" in
    AckUnsafe|EvictUnsafe) publication_expected=Inv_AcknowledgedDurability ;;
    CacheUnsafe) publication_expected=Inv_CacheOwnership ;;
    ReleaseUnsafe) publication_expected=Inv_LiveRetry ;;
    RestartUnsafe) publication_expected=Inv_ResidentBound ;;
    *) publication_expected='' ;;
  esac
  if [[ -n "$publication_expected" ]]; then
    [[ "$publication_result" == 12 ]]
    rg "Invariant $publication_expected is violated" "$publication_evidence/$publication_name.log"
  else
    [[ "$publication_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$publication_evidence/$publication_name.log"
  fi
done
fi
if [[ "$publication_mode" == all || "$publication_mode" == tla || "$publication_mode" == handoff ]]; then
for publication_variant in CapacityUnsafe LateReceiptUnsafe EligibilityUnsafe WaitUnsafe \
  AuthorityUnsafe BudgetUnsafe ReceiptBudgetUnsafe QuarantineUnsafe CooldownUnsafe AgeUnsafe \
  ZeroTracker ''; do
  publication_name="BufferRetryHandoff$publication_variant"
  mkdir -p "$publication_evidence/$publication_name"
  set +e
  timeout --signal=TERM --kill-after=10s 180s \
    java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
      -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
      -metadir "$publication_evidence/$publication_name" \
      -config "formal/tlaplus/block_admission/$publication_name.cfg" \
      formal/tlaplus/block_admission/BufferRetryHandoff.tla \
      > "$publication_evidence/$publication_name.log" 2>&1
  publication_result=$?
  set -e
  tail -n 10 "$publication_evidence/$publication_name.log"
  case "$publication_variant" in
    CapacityUnsafe|LateReceiptUnsafe|EligibilityUnsafe) publication_expected=Inv_LiveOwner ;;
    WaitUnsafe) publication_expected=Inv_LocalRetryAvailable ;;
    AuthorityUnsafe|BudgetUnsafe|ReceiptBudgetUnsafe|QuarantineUnsafe|CooldownUnsafe|AgeUnsafe)
      publication_expected=Inv_RetryPolicy ;;
    *) publication_expected='' ;;
  esac
  if [[ -n "$publication_expected" ]]; then
    [[ "$publication_result" == 12 ]]
    rg "Invariant $publication_expected is violated" "$publication_evidence/$publication_name.log"
  else
    [[ "$publication_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$publication_evidence/$publication_name.log"
  fi
done
fi
if [[ "$publication_mode" == all || "$publication_mode" == tla || "$publication_mode" == boundaries ]]; then
for publication_variant in PartialRowUnsafe EmptyRowUnsafe OldRowUnsafe IncomingBodyUnsafe StaleMaintenanceUnsafe ZeroTracker ''; do
  publication_name="BufferHandoffBoundaries$publication_variant"
  mkdir -p "$publication_evidence/$publication_name"
  set +e
  timeout --signal=TERM --kill-after=10s 180s \
    java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
      -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
      -metadir "$publication_evidence/$publication_name" \
      -config "formal/tlaplus/block_admission/$publication_name.cfg" \
      formal/tlaplus/block_admission/BufferHandoffBoundaries.tla \
      > "$publication_evidence/$publication_name.log" 2>&1
  publication_result=$?
  tail -n 10 "$publication_evidence/$publication_name.log"
  set -e
  case "$publication_variant" in
    PartialRowUnsafe) publication_expected=Inv_CompleteRows ;;
    EmptyRowUnsafe) publication_expected=Inv_DurableReleaseEnabled ;;
    OldRowUnsafe) publication_expected=Inv_LiveOwner ;;
    IncomingBodyUnsafe) publication_expected=Inv_CanonicalDependencies ;;
    StaleMaintenanceUnsafe) publication_expected=Inv_Policy ;;
    *) publication_expected='' ;;
  esac
  if [[ -n "$publication_expected" ]]; then
    [[ "$publication_result" == 12 ]]
    rg "Invariant $publication_expected is violated" "$publication_evidence/$publication_name.log"
  else
    [[ "$publication_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$publication_evidence/$publication_name.log"
  fi
done
fi
if [[ "$publication_mode" == all || "$publication_mode" == tla || "$publication_mode" == provenance ]]; then
for publication_variant in DeleteUnsafe CapacityUnsafe RestartUnsafe ''; do
  publication_name="BufferPendingProvenance$publication_variant"
  mkdir -p "$publication_evidence/$publication_name"
  set +e
  timeout --signal=TERM --kill-after=10s 180s \
    java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
      -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
      -metadir "$publication_evidence/$publication_name" \
      -config "formal/tlaplus/block_admission/$publication_name.cfg" \
      formal/tlaplus/block_admission/BufferPendingProvenance.tla \
      > "$publication_evidence/$publication_name.log" 2>&1
  publication_result=$?
  set -e
  tail -n 10 "$publication_evidence/$publication_name.log"
  case "$publication_variant" in
    DeleteUnsafe|RestartUnsafe) publication_expected=Inv_PendingEligibility ;;
    CapacityUnsafe) publication_expected=Inv_DependencyProgressAvailable ;;
    *) publication_expected='' ;;
  esac
  if [[ -n "$publication_expected" ]]; then
    [[ "$publication_result" == 12 ]]
    rg "Invariant $publication_expected is violated" "$publication_evidence/$publication_name.log"
  else
    [[ "$publication_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$publication_evidence/$publication_name.log"
  fi
done
fi
for publication_model in DependencyRequestProvenance DurablePendingHandoff RetryOperationOwnership PendingOwnerRegistry DeferredRetryCompletion; do
  publication_selected=promotion
  publication_variants=(NewOnlyUnsafe OverwriteUnsafe BudgetUnsafe StaleUnsafe '')
  if [[ "$publication_model" == DurablePendingHandoff ]]; then
    publication_selected=durable
    publication_variants=(StaleCommitUnsafe StaleCleanupUnsafe PartialUnsafe TerminalUnsafe EvictionUnsafe RestartUnsafe '')
  fi
  if [[ "$publication_model" == RetryOperationOwnership ]]; then
    publication_selected=retry-operations
    publication_variants=(HashUnsafe PrechargeUnsafe PendingUnsafe DoubleUnsafe BudgetUnsafe ProbeUnsafe SuccessOnlyUnsafe '')
  fi
  if [[ "$publication_model" == PendingOwnerRegistry ]]; then
    publication_selected=owner-registry
    publication_variants=(DuplicateUnsafe WeakUnsafe TerminalUnsafe DetachedUnsafe '')
  fi
  if [[ "$publication_model" == DeferredRetryCompletion ]]; then
    publication_selected=deferred-completion
    publication_variants=(LostUnsafe ReloadUnsafe ChargeUnsafe TerminalUnsafe NetworkUnsafe PermitsUnsafe SuccessOnlyUnsafe '')
  fi
  if [[ "$publication_mode" != all && "$publication_mode" != tla && "$publication_mode" != "$publication_selected" ]]; then continue; fi
  for publication_variant in "${publication_variants[@]}"; do
    publication_name="$publication_model$publication_variant"
    mkdir -p "$publication_evidence/$publication_name"
    set +e
    timeout --signal=TERM --kill-after=10s 180s \
      java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
        -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
        -metadir "$publication_evidence/$publication_name" \
        -config "formal/tlaplus/block_admission/$publication_name.cfg" \
        "formal/tlaplus/block_admission/$publication_model.tla" \
        > "$publication_evidence/$publication_name.log" 2>&1
    publication_result=$?
    set -e
    tail -n 10 "$publication_evidence/$publication_name.log"
    case "$publication_variant" in
      NewOnlyUnsafe|OverwriteUnsafe|StaleUnsafe) publication_expected=Inv_ExactAuthority ;;
      BudgetUnsafe) publication_expected=Inv_BudgetPreserved ;;
      StaleCommitUnsafe|RestartUnsafe) publication_expected=Inv_DurablePolicy ;;
      StaleCleanupUnsafe) publication_expected=Inv_NoStaleCleanup ;;
      PartialUnsafe|EvictionUnsafe) publication_expected=Inv_CompletePublication ;;
      TerminalUnsafe) publication_expected=Inv_TerminalDominance ;;
      *) publication_expected='' ;;
    esac
    if [[ "$publication_model" == RetryOperationOwnership ]]; then
      case "$publication_variant" in
        HashUnsafe|PrechargeUnsafe|PendingUnsafe|DoubleUnsafe|SuccessOnlyUnsafe) publication_expected=Inv_ExactCounters ;;
        BudgetUnsafe) publication_expected=Inv_ReservedBudget ;;
        ProbeUnsafe) publication_expected=Inv_NoExhaustedReservation ;;
        *) publication_expected='' ;;
      esac
    fi
    if [[ "$publication_model" == PendingOwnerRegistry ]]; then
      case "$publication_variant" in
        DuplicateUnsafe) publication_expected=Inv_CanonicalOwner ;;
        WeakUnsafe) publication_expected=Inv_WeakBound ;;
        TerminalUnsafe) publication_expected=Inv_CurrentWriter ;;
        DetachedUnsafe) publication_expected=Inv_OwnerWitness ;;
        *) publication_expected='' ;;
      esac
    fi
    if [[ "$publication_model" == DeferredRetryCompletion ]]; then
      case "$publication_variant" in
        LostUnsafe) publication_expected=Inv_CompletionRetained ;;
        ReloadUnsafe|ChargeUnsafe|SuccessOnlyUnsafe) publication_expected=Inv_ExactEffective ;;
        TerminalUnsafe) publication_expected=Inv_CurrentWriter ;;
        NetworkUnsafe) publication_expected=Inv_NoNetworkOnDrain ;;
        PermitsUnsafe) publication_expected=Inv_PermitBound ;;
        *) publication_expected='' ;;
      esac
    fi
    if [[ -n "$publication_expected" ]]; then
      [[ "$publication_result" == 12 ]]
      rg "Invariant $publication_expected is violated" "$publication_evidence/$publication_name.log"
    else
      [[ "$publication_result" == 0 ]]
      rg -q 'Model checking completed. No error has been found.' "$publication_evidence/$publication_name.log"
    fi
  done
done
if [[ "$publication_mode" == all || "$publication_mode" == rocq || "$publication_mode" == boundaries || "$publication_mode" == promotion || "$publication_mode" == retry-operations ]]; then
  mkdir -p "$publication_evidence/rocq"
  publication_modules=(BufferPublicationOwnership BufferRetryHandoff BufferHandoffBoundaries DependencyRequestProvenance RetryOperationOwnership)
  if [[ "$publication_mode" == boundaries ]]; then publication_modules=(BufferHandoffBoundaries); fi
  if [[ "$publication_mode" == promotion ]]; then publication_modules=(DependencyRequestProvenance); fi
  if [[ "$publication_mode" == retry-operations ]]; then publication_modules=(RetryOperationOwnership); fi
  for publication_module in "${publication_modules[@]}"; do
  timeout --signal=TERM --kill-after=10s 180s \
    coqc -Q "$publication_evidence/rocq" FinalizedFloor \
      -o "$publication_evidence/rocq/$publication_module.vo" \
      "formal/rocq/finalized_floor/theories/$publication_module.v" \
      > "$publication_evidence/rocq/$publication_module.compile.log" 2>&1 || {
        tail -n 35 "$publication_evidence/rocq/$publication_module.compile.log"
        exit 1
      }
  publication_theorems=14
  if [[ "$publication_module" == BufferRetryHandoff ]]; then publication_theorems=19; fi
  if [[ "$publication_module" == BufferHandoffBoundaries ]]; then publication_theorems=13; fi
  if [[ "$publication_module" == DependencyRequestProvenance ]]; then publication_theorems=6; fi
  if [[ "$publication_module" == RetryOperationOwnership ]]; then publication_theorems=16; fi
  [[ $(rg -c '^Closed under the global context$' "$publication_evidence/rocq/$publication_module.compile.log") == "$publication_theorems" ]]
  timeout --signal=TERM --kill-after=10s 180s \
    coqchk -Q "$publication_evidence/rocq" FinalizedFloor "FinalizedFloor.$publication_module" \
      > "$publication_evidence/rocq/$publication_module.kernel.log" 2>&1
  rg 'Modules were successfully checked' "$publication_evidence/rocq/$publication_module.kernel.log"
  done
fi

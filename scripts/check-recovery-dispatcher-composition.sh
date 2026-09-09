#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
dispatcher_mode=${1:-all}
case "$dispatcher_mode" in all|syntax|controls|retry-controls|retry-formal|safe|control|worker-formal|worker-native|integration-native|symbolic-base|symbolic-step|symbolic-group) ;; *) exit 2 ;; esac
dispatcher_cgroup=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
dispatcher_memory=$(<"/sys/fs/cgroup$dispatcher_cgroup/memory.max")
dispatcher_swap=$(<"/sys/fs/cgroup$dispatcher_cgroup/memory.swap.max")
if [[ ! "$dispatcher_memory" =~ ^[1-9][0-9]*$ ]] \
   || (( dispatcher_memory > 8589934592 )) || [[ "$dispatcher_swap" != 0 ]]; then
  echo 'Use a systemd scope with MemoryMax at most 8 GiB and MemorySwapMax=0.' >&2
  exit 2
fi
mkdir -p target/verification/recovery-dispatcher
dispatcher_evidence=$(mktemp -d "$PWD/target/verification/recovery-dispatcher/run.XXXXXX")
mkdir -p "$dispatcher_evidence/tmp"
export TMPDIR="$dispatcher_evidence/tmp" CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1
printf 'Evidence: %s\nMemoryMax: %s\n' "$dispatcher_evidence" "$dispatcher_memory"
sha256sum scripts/check-recovery-dispatcher-composition.sh \
  formal/tlaplus/block_admission/RecoveryDispatcherComposition* \
  formal/tlaplus/block_admission/MC_RecoveryDispatcherComposition.tla \
  formal/rocq/finalized_floor/theories/RecoveryWorkerLifecycle.v \
  formal/rocq/finalized_floor/theories/RecoveryRetryAccounting.v \
  scripts/check-recovery-input-closure.sh \
  formal/tlaplus/block_admission/RecoveryInputClosure* \
  casper/tests/engine/admission_ownership_spec.rs \
  casper/src/rust/blocks/block_processing_queue.rs \
  casper/src/rust/blocks/block_processing_queue/{admission_budget,admission_identity,recovery_control}.rs \
  node/src/rust/instances/block_processor_instance.rs \
  node/src/rust/runtime/runtime_supervision.rs \
  casper/src/rust/blocks/block_processing_queue/recovery_signal_state.rs \
  casper/src/rust/blocks/block_processing_queue/{startup_owner,startup_owner_tests}.rs \
  casper/src/rust/{casper,blocks/block_processor}.rs \
  casper/src/rust/engine/{engine,casper_launch,running,initializing,genesis_ceremony_master}.rs \
  casper/src/rust/engine/multi_parent_casper/{dispatch,buffer_resolver}.rs \
  casper/src/rust/engine/multi_parent_casper/buffer_resolver/recovery_metadata_tests.rs \
  casper/tests/helper/no_ops_casper_effect.rs casper/tests/engine/running_spec.rs \
  block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage.rs \
  node/src/rust/instances/block_processor_instance/{recovery_driver,recovery_driver_tests,ownership_tests,input_closure_tests,acknowledgment_tests}.rs \
  node/src/rust/runtime/{node_runtime,setup}.rs \
  > "$dispatcher_evidence/inputs.sha256"
dispatcher_finish() {
  local dispatcher_result=$?
  trap - EXIT
  if ! sha256sum --check "$dispatcher_evidence/inputs.sha256" > "$dispatcher_evidence/inputs-check.log"; then
    cat "$dispatcher_evidence/inputs-check.log"
    dispatcher_result=1
  fi
  printf 'Gate exit: %s\n' "$dispatcher_result"
  exit "$dispatcher_result"
}
trap dispatcher_finish EXIT
dispatcher_syntax() {
  java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
    -cp /usr/share/java/tla2tools.jar tla2sany.SANY \
    formal/tlaplus/block_admission/RecoveryDispatcherComposition.tla \
    > "$dispatcher_evidence/syntax.log" 2>&1
  cat "$dispatcher_evidence/syntax.log"
  ! rg -q 'Fatal errors|Semantic errors|Parse Error|Lexical error|Error:' "$dispatcher_evidence/syntax.log"
}
dispatcher_check() {
  local dispatcher_variant="$1" dispatcher_expected="$2" dispatcher_result
  local dispatcher_name="RecoveryDispatcherComposition$dispatcher_variant"
  mkdir -p "$dispatcher_evidence/$dispatcher_name"
  set +e
  java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
    -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
    -metadir "$dispatcher_evidence/$dispatcher_name" \
    -config "formal/tlaplus/block_admission/$dispatcher_name.cfg" \
    formal/tlaplus/block_admission/RecoveryDispatcherComposition.tla \
    > "$dispatcher_evidence/$dispatcher_name.log" 2>&1
  dispatcher_result=$?
  set -e
  tail -n 18 "$dispatcher_evidence/$dispatcher_name.log"
  rg 'Invariant .* is violated|Model checking completed' "$dispatcher_evidence/$dispatcher_name.log" || true
  if [[ -z "$dispatcher_expected" ]]; then
    [[ "$dispatcher_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$dispatcher_evidence/$dispatcher_name.log"
  else
    [[ "$dispatcher_result" == 12 ]]
    rg -q "Invariant $dispatcher_expected is violated" "$dispatcher_evidence/$dispatcher_name.log"
  fi
}
dispatcher_control() {
  case "$1" in
    DetachedWorker) dispatcher_check DetachedWorkerUnsafe Inv_WorkersOwned ;;
    DetachedService) dispatcher_check DetachedServiceUnsafe Inv_ServicesOwned ;;
    WorkerCap) dispatcher_check WorkerCapUnsafe Inv_WorkerLimit ;;
    EarlyBytes) dispatcher_check EarlyBytesUnsafe Inv_PayloadCharged ;;
    EarlyIdentity) dispatcher_check EarlyIdentityUnsafe Inv_IdentityRetained ;;
    EarlyWake) dispatcher_check EarlyWakeUnsafe Inv_ReleaseOrder ;;
    RejectedWake) dispatcher_check RejectedWakeUnsafe Inv_RejectionCannotSelfWake ;;
    SenderAwait) dispatcher_check SenderAwaitUnsafe Inv_NoSenderAcrossWait ;;
    DroppedWake) dispatcher_check DroppedWakeUnsafe Inv_SuccessorDemand ;;
    LostPendingOffer) dispatcher_check LostPendingOfferUnsafe Inv_PendingOffer ;;
    StaleAuthorization) dispatcher_check StaleAuthorizationUnsafe Inv_AuthorizationWitness ;;
    AbortRetires) dispatcher_check AbortRetiresUnsafe Inv_AbortRetirement ;;
    CaptureReuse) dispatcher_check CaptureReuseUnsafe Inv_PendingCaptureBound ;;
    CountBeforeLoad) dispatcher_check CountBeforeLoadUnsafe Inv_CountBeforeLoad ;;
    BodyAcrossWait) dispatcher_check BodyAcrossWaitUnsafe Inv_NoBodyAcrossWait ;;
    PresenceBypass) dispatcher_check PresenceBypassUnsafe Inv_NoPresenceBypass ;;
    LoadWithoutVisit) dispatcher_check LoadWithoutVisitUnsafe Inv_NoLoadWithoutVisit ;;
    ParentAbortLost) dispatcher_check ParentAbortLostUnsafe Inv_AbortRetirement ;;
    DropPendingOnTemporaryRejection) dispatcher_check DropPendingOnTemporaryRejectionUnsafe Inv_CandidateRetained ;;
    CompleteWithPending) dispatcher_check CompleteWithPendingUnsafe Inv_NoPendingCompletion ;;
    RefuelWithoutSuspension) dispatcher_check RefuelWithoutSuspensionUnsafe Inv_PageConservation ;;
    LoadWithoutAttempt) dispatcher_check LoadWithoutAttemptUnsafe Inv_NoLoadWithoutAttempt ;;
    ActiveRetryDemand) dispatcher_check ActiveRetryDemandUnsafe Inv_SuccessorDemand ;;
    IgnoreAcknowledgmentError) dispatcher_check IgnoreAcknowledgmentErrorUnsafe Inv_ErrorPreventsProposal ;;
    *) return 2 ;;
  esac
}
dispatcher_worker_formal() {
  mkdir -p "$dispatcher_evidence/rocq"
  coqc -Q "$dispatcher_evidence/rocq" FinalizedFloor \
    -o "$dispatcher_evidence/rocq/RecoveryWorkerLifecycle.vo" \
    formal/rocq/finalized_floor/theories/RecoveryWorkerLifecycle.v \
    > "$dispatcher_evidence/worker-compile.log" 2>&1 || {
      tail -n 35 "$dispatcher_evidence/worker-compile.log"
      return 1
    }
  [[ $(rg -c '^Closed under the global context$' "$dispatcher_evidence/worker-compile.log") == 17 ]]
  coqchk -Q "$dispatcher_evidence/rocq" FinalizedFloor FinalizedFloor.RecoveryWorkerLifecycle \
    > "$dispatcher_evidence/worker-kernel.log" 2>&1
  rg 'Modules were successfully checked' "$dispatcher_evidence/worker-kernel.log"
}
dispatcher_worker_native() {
  cargo test --offline -p casper --test mod engine::admission_ownership_spec:: -- --test-threads=1 \
    > "$dispatcher_evidence/worker-tests.log" 2>&1 || {
      tail -n 55 "$dispatcher_evidence/worker-tests.log"
      return 1
    }
  rg 'test result: ok\. 5 passed; 0 failed; 0 ignored' "$dispatcher_evidence/worker-tests.log"
  cargo clippy --offline -p casper --test mod -- -D warnings \
    > "$dispatcher_evidence/worker-lint.log" 2>&1 || {
      tail -n 45 "$dispatcher_evidence/worker-lint.log"
      return 1
    }
}
dispatcher_retry_formal() {
  mkdir -p "$dispatcher_evidence/rocq"
  coqc -Q "$dispatcher_evidence/rocq" FinalizedFloor \
    -o "$dispatcher_evidence/rocq/RecoveryRetryAccounting.vo" \
    formal/rocq/finalized_floor/theories/RecoveryRetryAccounting.v \
    > "$dispatcher_evidence/retry-compile.log" 2>&1 || {
      tail -n 35 "$dispatcher_evidence/retry-compile.log"
      return 1
    }
  [[ $(rg -c '^Closed under the global context$' "$dispatcher_evidence/retry-compile.log") == 21 ]]
  coqchk -Q "$dispatcher_evidence/rocq" FinalizedFloor FinalizedFloor.RecoveryRetryAccounting \
    > "$dispatcher_evidence/retry-kernel.log" 2>&1
  rg 'Modules were successfully checked' "$dispatcher_evidence/retry-kernel.log"
}
dispatcher_symbolic() {
  local dispatcher_init="$1" dispatcher_invariant="$2" dispatcher_length="$3"
  local dispatcher_name="symbolic-$dispatcher_init-$dispatcher_invariant"
  JVM_ARGS=-Xmx1536m JVM_GC_ARGS=-XX:+UseSerialGC \
    apalache-mc --out-dir="$dispatcher_evidence/$dispatcher_name" check \
    --init="$dispatcher_init" --next=Next --inv="$dispatcher_invariant" \
    --length="$dispatcher_length" --no-deadlock=true \
    formal/tlaplus/block_admission/MC_RecoveryDispatcherComposition.tla \
    > "$dispatcher_evidence/$dispatcher_name.log" 2>&1 || {
      tail -n 35 "$dispatcher_evidence/$dispatcher_name.log"
      return 1
    }
  tail -n 10 "$dispatcher_evidence/$dispatcher_name.log"
  rg -q 'The outcome is: NoError' "$dispatcher_evidence/$dispatcher_name.log"
  rg -q 'EXITCODE: OK' "$dispatcher_evidence/$dispatcher_name.log"
}
dispatcher_syntax
if [[ "$dispatcher_mode" == all ]]; then
  bash scripts/check-recovery-input-closure.sh
fi
if [[ "$dispatcher_mode" == integration-native || "$dispatcher_mode" == all ]]; then
  cargo test --offline -p node --lib rust::instances::block_processor_instance:: -- --test-threads=1 \
    > "$dispatcher_evidence/dispatcher-tests.log" 2>&1 || { tail -n 65 "$dispatcher_evidence/dispatcher-tests.log"; exit 1; }
  rg 'test result: ok\. 24 passed; 0 failed; 0 ignored' "$dispatcher_evidence/dispatcher-tests.log"
  cargo test --offline -p casper --lib recovery_metadata_tests:: -- --test-threads=1 \
    > "$dispatcher_evidence/resolver-tests.log" 2>&1 || { tail -n 65 "$dispatcher_evidence/resolver-tests.log"; exit 1; }
  rg 'test result: ok\. 7 passed; 0 failed; 0 ignored' "$dispatcher_evidence/resolver-tests.log"
  cargo test --offline -p casper --lib startup_owner::tests:: -- --test-threads=1 \
    > "$dispatcher_evidence/context-tests.log" 2>&1 || { tail -n 65 "$dispatcher_evidence/context-tests.log"; exit 1; }
  rg 'test result: ok\. 26 passed; 0 failed; 0 ignored' "$dispatcher_evidence/context-tests.log"
  cargo clippy --offline -p node --lib --tests -- -D warnings \
    > "$dispatcher_evidence/dispatcher-lint.log" 2>&1 || { tail -n 65 "$dispatcher_evidence/dispatcher-lint.log"; exit 1; }
fi
if [[ "$dispatcher_mode" == all || "$dispatcher_mode" == controls ]]; then
  for dispatcher_variant in DetachedWorker DetachedService EarlyBytes EarlyIdentity EarlyWake \
      RejectedWake SenderAwait DroppedWake AbortRetires CaptureReuse CountBeforeLoad \
      WorkerCap StaleAuthorization LostPendingOffer BodyAcrossWait PresenceBypass \
      LoadWithoutVisit ParentAbortLost; do
    dispatcher_control "$dispatcher_variant"
  done
fi
if [[ "$dispatcher_mode" == all || "$dispatcher_mode" == controls || "$dispatcher_mode" == retry-controls ]]; then
  for dispatcher_variant in DropPendingOnTemporaryRejection CompleteWithPending \
      RefuelWithoutSuspension LoadWithoutAttempt ActiveRetryDemand IgnoreAcknowledgmentError; do
    dispatcher_control "$dispatcher_variant"
  done
fi
if [[ "$dispatcher_mode" == control ]]; then
  dispatcher_control "$2"
fi
if [[ "$dispatcher_mode" == safe ]]; then
  dispatcher_check '' ''
fi
if [[ "$dispatcher_mode" == all || "$dispatcher_mode" == worker-formal ]]; then
  dispatcher_worker_formal
fi
if [[ "$dispatcher_mode" == all || "$dispatcher_mode" == retry-formal ]]; then
  dispatcher_retry_formal
fi
if [[ "$dispatcher_mode" == all || "$dispatcher_mode" == symbolic-base ]]; then
  dispatcher_symbolic Init IndInv 0
fi
if [[ "$dispatcher_mode" == all || "$dispatcher_mode" == symbolic-step ]]; then
  for dispatcher_group in StateDomain Safety WorkerState SupervisorState ServiceState ProposalState PassState CaptureState; do
    dispatcher_symbolic IndInv "$dispatcher_group" 1
  done
fi
if [[ "$dispatcher_mode" == symbolic-group ]]; then
  case "${2:-}" in
    StateDomain|Safety|WorkerState|SupervisorState|ServiceState|ProposalState|PassState|CaptureState)
      dispatcher_symbolic IndInv "$2" 1 ;;
    *) exit 2 ;;
  esac
fi
if [[ "$dispatcher_mode" == all || "$dispatcher_mode" == worker-native ]]; then
  dispatcher_worker_native
fi

#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
pump_mode=${1:-all}
case "$pump_mode" in all|formal|native|initializer|initializer-native|snapshot-formal|snapshot-native|scan-native|completion-formal|completion-native|publication-formal|startup-native|lease-formal|lease-native|metadata-formal|metadata-native|handoff-formal|handoff-native) ;; *) exit 2 ;; esac
pump_cgroup=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
pump_memory=$(<"/sys/fs/cgroup$pump_cgroup/memory.max")
pump_swap=$(<"/sys/fs/cgroup$pump_cgroup/memory.swap.max")
if [[ ! "$pump_memory" =~ ^[1-9][0-9]*$ ]] \
   || (( pump_memory > 8589934592 )) || [[ "$pump_swap" != 0 ]]; then
  echo 'Use a systemd scope with MemoryMax at most 8 GiB and MemorySwapMax=0.' >&2
  exit 2
fi
mkdir -p target/verification/recovery-pump
pump_evidence=$(mktemp -d "$PWD/target/verification/recovery-pump/run.XXXXXX")
mkdir -p "$pump_evidence/rocq" "$pump_evidence/tmp"
export TMPDIR="$pump_evidence/tmp" CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1
unset LOOM_MAX_PREEMPTIONS LOOM_MAX_PERMUTATIONS LOOM_MAX_DURATION LOOM_CHECKPOINT_FILE
printf 'Evidence: %s\nMemoryMax: %s\n' "$pump_evidence" "$pump_memory"
sha256sum scripts/check-recovery-pump-control.sh \
  formal/tlaplus/block_admission/RecoveryPump* \
  formal/tlaplus/block_admission/RecoveryMetadataReadiness* \
  formal/tlaplus/block_admission/RecoveryDispatcherOwnership* \
  formal/tlaplus/block_admission/InitializerOwnership* \
  formal/tlaplus/block_admission/StartupSnapshot* \
  formal/tlaplus/block_admission/StartupCompletion* \
  formal/tlaplus/block_admission/StartupPublication* \
  formal/rocq/finalized_floor/theories/RecoveryPumpControl.v \
  formal/rocq/finalized_floor/theories/RecoveryDemandHandoff.v \
  formal/rocq/finalized_floor/theories/RecoveryMetadataReadiness.v \
  block-storage/src/rust/dag/{admitted_metadata,block_metadata_store,block_dag_key_value_storage}.rs \
  shared/src/rust/store/{key_value_store,key_value_typed_store_impl}.rs \
  block-storage/src/rust/dag/block_dag_key_value_storage/finalization_snapshot_tests/recovery_metadata_tests.rs \
  casper/src/rust/util/proto_util.rs \
  casper/src/rust/util/proto_util/{dependency_readiness,recovery_dependency_tests}.rs \
  casper/src/rust/engine/multi_parent_casper/buffer_resolver.rs \
  casper/src/rust/engine/multi_parent_casper/buffer_resolver/recovery_metadata_tests.rs \
  formal/loom/cost_accounting/tests/{property_recovery_metadata,loom_recovery_metadata}.rs \
  formal/rocq/finalized_floor/theories/InitializerOwnership.v \
  formal/rocq/finalized_floor/theories/StartupSnapshot.v \
  formal/rocq/finalized_floor/theories/StartupCompletion.v \
  formal/rocq/finalized_floor/theories/StartupSnapshotLease.v \
  casper/src/rust/blocks/block_processing_queue.rs \
  casper/src/rust/blocks/block_processing_queue/{admission_budget,admission_identity,recovery_signal_state,recovery_pass,recovery_control}.rs \
  casper/tests/engine/admission_ownership_spec.rs \
  formal/loom/cost_accounting/tests/{loom_recovery_pump_control,property_recovery_pump_control}.rs \
  formal/loom/cost_accounting/tests/initializer_ownership.rs \
  node/src/rust/runtime/{mod,node_runtime,runtime_supervision}.rs \
  block-storage/src/rust/util/{ordered_snapshot,startup_scan,doubly_linked_dag_operations}.rs \
  block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage.rs \
  block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage/persistence_tests.rs \
  formal/loom/cost_accounting/tests/{property_startup_snapshot,loom_startup_snapshot,property_startup_scan}.rs \
  casper/src/rust/blocks/block_processing_queue/{startup_completion,startup_owner,startup_owner_tests}.rs \
  casper/src/rust/blocks/block_processing_queue/startup_snapshot_lease.rs \
  formal/loom/cost_accounting/tests/{property_startup_snapshot_lease,loom_startup_snapshot_lease}.rs \
  casper/src/rust/engine/{engine_cell,engine}.rs \
  formal/loom/cost_accounting/tests/startup_runtime.rs \
  formal/loom/cost_accounting/tests/{property_startup_completion,loom_startup_completion}.rs \
  formal/loom/cost_accounting/Cargo.toml \
  Cargo.lock \
  > "$pump_evidence/inputs.sha256"
trap 'sha256sum --check "$pump_evidence/inputs.sha256"' EXIT
formal() {
coqc -Q "$pump_evidence/rocq" FinalizedFloor \
  -o "$pump_evidence/rocq/RecoveryPumpControl.vo" \
  formal/rocq/finalized_floor/theories/RecoveryPumpControl.v \
  > "$pump_evidence/rocq/compile.log" 2>&1 || {
  tail -n 30 "$pump_evidence/rocq/compile.log"
  exit 1
}
[[ $(rg -c '^Closed under the global context$' "$pump_evidence/rocq/compile.log") == 20 ]]
coqchk -Q "$pump_evidence/rocq" FinalizedFloor FinalizedFloor.RecoveryPumpControl \
  > "$pump_evidence/rocq/kernel.log" 2>&1
rg -q 'Modules were successfully checked' "$pump_evidence/rocq/kernel.log"
check_model RecoveryPumpWake LostUnsafe Inv_NoLostWake
check_model RecoveryPumpWake PendingUnsafe Inv_PendingRetained
check_model RecoveryPumpPass FailureUnsafe Inv_FailureSticky
check_model RecoveryPumpPass CapacityUnsafe Inv_CompletePass
check_model RecoveryPumpPass SelfWakeUnsafe Inv_RejectionCannotSelfWake
check_model RecoveryDispatcherOwnership DetachedUnsafe Inv_ChildrenOwned
check_model RecoveryPumpWake '' ''
check_model RecoveryPumpPass '' ''
check_model RecoveryDispatcherOwnership '' ''
handoff_formal
initializer_formal
snapshot_formal
completion_formal
publication_formal
lease_formal
metadata_formal
}

handoff_formal() {
  coqc -Q "$pump_evidence/rocq" FinalizedFloor \
    -o "$pump_evidence/rocq/RecoveryPumpControl.vo" \
    formal/rocq/finalized_floor/theories/RecoveryPumpControl.v \
    > "$pump_evidence/rocq/handoff-base.log" 2>&1
  [[ $(rg -c '^Closed under the global context$' "$pump_evidence/rocq/handoff-base.log") == 20 ]]
  coqc -Q "$pump_evidence/rocq" FinalizedFloor \
    -o "$pump_evidence/rocq/RecoveryDemandHandoff.vo" \
    formal/rocq/finalized_floor/theories/RecoveryDemandHandoff.v \
    > "$pump_evidence/rocq/handoff-compile.log" 2>&1 || {
    tail -n 30 "$pump_evidence/rocq/handoff-compile.log"
    return 1
  }
  [[ $(rg -c '^Closed under the global context$' "$pump_evidence/rocq/handoff-compile.log") == 10 ]]
  coqchk -Q "$pump_evidence/rocq" FinalizedFloor FinalizedFloor.RecoveryDemandHandoff \
    > "$pump_evidence/rocq/handoff-kernel.log" 2>&1
  rg -q 'Modules were successfully checked' "$pump_evidence/rocq/handoff-kernel.log"
  check_model RecoveryPumpDemand ContinueUnsafe Inv_ExactDemand
  check_model RecoveryPumpDemand ProposalUnsafe Inv_ExactDemand
  check_model RecoveryPumpDemand StopUnsafe Inv_StopIsTerminal
  check_model RecoveryPumpDemand TakenUnsafe Inv_ExactNextDemand
  check_model RecoveryPumpDemand '' ''
}

metadata_formal() {
  coqc -Q "$pump_evidence/rocq" FinalizedFloor \
    -o "$pump_evidence/rocq/RecoveryMetadataReadiness.vo" \
    formal/rocq/finalized_floor/theories/RecoveryMetadataReadiness.v \
    > "$pump_evidence/rocq/metadata-compile.log" 2>&1 || {
    tail -n 30 "$pump_evidence/rocq/metadata-compile.log"
    return 1
  }
  [[ $(rg -c '^Closed under the global context$' "$pump_evidence/rocq/metadata-compile.log") == 8 ]]
  coqchk -Q "$pump_evidence/rocq" FinalizedFloor FinalizedFloor.RecoveryMetadataReadiness \
    > "$pump_evidence/rocq/metadata-kernel.log" 2>&1
  rg -q 'Modules were successfully checked' "$pump_evidence/rocq/metadata-kernel.log"
  check_model RecoveryMetadataReadiness VisibilityUnsafe Inv_ReadWitness
  check_model RecoveryMetadataReadiness RowUnsafe Inv_ReadWitness
  check_model RecoveryMetadataReadiness ErrorUnsafe Inv_ErrorPreserved
  check_model RecoveryMetadataReadiness EarlyUnsafe Inv_CompleteOrError
  check_model RecoveryMetadataReadiness '' ''
}

metadata_native() {
  native_check metadata-property cargo test --offline --release -p cost-accounting-loom-models --test property_recovery_metadata
  rg -q 'test result: ok\. 4 passed; 0 failed; 0 ignored' "$pump_evidence/metadata-property.log"
  native_check metadata-loom cargo test --offline --release -p cost-accounting-loom-models --test loom_recovery_metadata
  rg -q 'test result: ok\. 4 passed; 0 failed; 0 ignored' "$pump_evidence/metadata-loom.log"
  native_check metadata-storage cargo test --offline -p block-storage --lib recovery_metadata_tests
  rg -q 'test result: ok\. 5 passed; 0 failed; 0 ignored' "$pump_evidence/metadata-storage.log"
  native_check metadata-dependencies cargo test --offline -p casper --lib recovery_dependency_tests
  rg -q 'test result: ok\. 6 passed; 0 failed; 0 ignored' "$pump_evidence/metadata-dependencies.log"
  native_check metadata-resolver cargo test --offline -p casper --lib buffer_resolver::recovery_metadata_tests
  rg -q 'test result: ok\. 7 passed; 0 failed; 0 ignored' "$pump_evidence/metadata-resolver.log"
  native_check metadata-buffer cargo test --offline -p casper --lib buffer_resolver::tests
  rg -q 'test result: ok\. 3 passed; 0 failed; 0 ignored' "$pump_evidence/metadata-buffer.log"
  native_check metadata-lint cargo clippy --offline -p block-storage -p casper -p node --lib --tests -- -D warnings
  native_check metadata-standalone-lint cargo clippy --offline --release -p cost-accounting-loom-models --test property_recovery_metadata --test loom_recovery_metadata -- -D warnings
}

lease_formal() {
  coqc -Q "$pump_evidence/rocq" FinalizedFloor \
    -o "$pump_evidence/rocq/StartupSnapshotLease.vo" \
    formal/rocq/finalized_floor/theories/StartupSnapshotLease.v \
    > "$pump_evidence/rocq/lease-compile.log" 2>&1 || {
    tail -n 30 "$pump_evidence/rocq/lease-compile.log"
    return 1
  }
  [[ $(rg -c '^Closed under the global context$' "$pump_evidence/rocq/lease-compile.log") == 28 ]]
  coqchk -Q "$pump_evidence/rocq" FinalizedFloor FinalizedFloor.StartupSnapshotLease \
    > "$pump_evidence/rocq/lease-kernel.log" 2>&1
  rg -q 'Modules were successfully checked' "$pump_evidence/rocq/lease-kernel.log"
  check_model StartupSnapshotLease CaptureUnsafe Inv_EverySnapshotLeased
  check_model StartupSnapshotLease CancellationUnsafe Inv_AtMostTwoSnapshotOwners
  check_model StartupSnapshotLease PendingReleaseUnsafe Inv_ReleaseAfterDestruction
  check_model StartupSnapshotLease PublicationUnsafe Inv_ReleaseAfterDestruction
  check_model StartupSnapshotLease DoublePendingUnsafe Inv_OnePendingOwner
  check_model StartupSnapshotLease ActivationUnsafe Inv_ActivationRequiresStoredWork
  check_model StartupSnapshotLease StaleReleaseUnsafe Inv_ExactLeaseRelease
  check_model StartupSnapshotLease StaleRoleUnsafe Inv_ExactLeaseRelease
  check_model StartupSnapshotLease ActiveReleaseUnsafe Inv_OneActiveOwner
  check_model StartupSnapshotLease BusyWaiterUnsafe Inv_BusyAllocatesNothing
  check_model StartupSnapshotLease '' ''
}

publication_formal() {
  check_model StartupPublication LockUnsafe Inv_PublicationVisible
  check_model StartupPublication DropUnsafe Inv_DestructionOutsideGuards
  check_model StartupPublication StopUnsafe Inv_StopPublishedBeforeDestruction
  check_model StartupPublication EventUnsafe Inv_OnlyCommittedRunningEvents
  check_model StartupPublication '' ''
}

completion_formal() {
  coqc -Q "$pump_evidence/rocq" FinalizedFloor \
    -o "$pump_evidence/rocq/StartupCompletion.vo" \
    formal/rocq/finalized_floor/theories/StartupCompletion.v \
    > "$pump_evidence/rocq/completion-compile.log" 2>&1 || {
    tail -n 30 "$pump_evidence/rocq/completion-compile.log"
    return 1
  }
  [[ $(rg -c '^Closed under the global context$' "$pump_evidence/rocq/completion-compile.log") == 17 ]]
  coqchk -Q "$pump_evidence/rocq" FinalizedFloor FinalizedFloor.StartupCompletion \
    > "$pump_evidence/rocq/completion-kernel.log" 2>&1
  rg -q 'Modules were successfully checked' "$pump_evidence/rocq/completion-kernel.log"
  check_model StartupCompletion CompletionUnsafe Inv_StartupCompletionIdentity
  check_model StartupCompletion AuthorizationUnsafe Inv_StartupAuthorizationOrigin
  check_model StartupCompletion SuccessUnsafe Inv_StartupSuccessOrigin
  check_model StartupCompletion DropUnsafe Inv_StartupRetirementOwnership
  check_model StartupCompletion StopUnsafe Inv_RecoveryStopTerminal
  check_model StartupCompletion PublicationUnsafe Inv_ContextPublication
  check_model StartupCompletion PendingUnsafe Inv_StartupOwnershipBound
  check_model StartupCompletion PhaseUnsafe Inv_StartupCompletionIdentity
  check_model StartupCompletion CallbackUnsafe Inv_StartupSuccessOrigin
  check_model StartupCompletion RetirementUnsafe Inv_StartupRetirementOwnership
  check_model StartupCompletion '' ''
}

snapshot_formal() {
  coqc -Q "$pump_evidence/rocq" FinalizedFloor \
    -o "$pump_evidence/rocq/StartupSnapshot.vo" \
    formal/rocq/finalized_floor/theories/StartupSnapshot.v \
    > "$pump_evidence/rocq/snapshot-compile.log" 2>&1 || {
    tail -n 30 "$pump_evidence/rocq/snapshot-compile.log"
    return 1
  }
  [[ $(rg -c '^Closed under the global context$' "$pump_evidence/rocq/snapshot-compile.log") == 6 ]]
  coqchk -Q "$pump_evidence/rocq" FinalizedFloor FinalizedFloor.StartupSnapshot \
    > "$pump_evidence/rocq/snapshot-kernel.log" 2>&1
  rg -q 'Modules were successfully checked' "$pump_evidence/rocq/snapshot-kernel.log"
  check_model StartupSnapshot MutableUnsafe Inv_ImmutableSnapshot
  check_model StartupSnapshot EarlyUnsafe Inv_FilterBeforeAdmission
  check_model StartupSnapshot PruneUnsafe Inv_SelectionPreserved
  check_model StartupSnapshot '' ''
}

initializer_formal() {
  coqc -Q "$pump_evidence/rocq" FinalizedFloor \
    -o "$pump_evidence/rocq/InitializerOwnership.vo" \
    formal/rocq/finalized_floor/theories/InitializerOwnership.v \
    > "$pump_evidence/rocq/initializer-compile.log" 2>&1 || {
    tail -n 30 "$pump_evidence/rocq/initializer-compile.log"
    return 1
  }
  [[ $(rg -c '^Closed under the global context$' "$pump_evidence/rocq/initializer-compile.log") == 8 ]]
  coqchk -Q "$pump_evidence/rocq" FinalizedFloor FinalizedFloor.InitializerOwnership \
    > "$pump_evidence/rocq/initializer-kernel.log" 2>&1
  rg -q 'Modules were successfully checked' "$pump_evidence/rocq/initializer-kernel.log"
  check_model InitializerOwnership TakeUnsafe Inv_NoDetachedChild
  check_model InitializerOwnership DropUnsafe Inv_NoDetachedChild
  check_model InitializerOwnership CleanupUnsafe Inv_CleanupAfterRetirement
  check_model InitializerOwnership '' ''
}

check_model() {
  local pump_module=$1 pump_variant=$2 pump_expected=$3
  local pump_name="$pump_module$pump_variant"
  mkdir -p "$pump_evidence/$pump_name"
  set +e
  java -Xmx1g -XX:+UseSerialGC -cp /usr/share/java/tla2tools.jar tlc2.TLC \
    -workers 1 -metadir "$pump_evidence/$pump_name" \
    -config "formal/tlaplus/block_admission/$pump_name.cfg" \
    "formal/tlaplus/block_admission/$pump_module.tla" \
    > "$pump_evidence/$pump_name.log" 2>&1
  pump_result=$?
  set -e
  tail -n 12 "$pump_evidence/$pump_name.log"
  if [[ -z "$pump_expected" ]]; then
    [[ "$pump_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$pump_evidence/$pump_name.log"
  else
    [[ "$pump_result" == 12 ]]
    rg -q "Invariant $pump_expected is violated" "$pump_evidence/$pump_name.log"
  fi
}

native_check() {
  local pump_name=$1
  shift
  if "$@" > "$pump_evidence/$pump_name.log" 2>&1; then
    tail -n 12 "$pump_evidence/$pump_name.log"
  else
    tail -n 65 "$pump_evidence/$pump_name.log"
    return 1
  fi
}

native() {
  metadata_native
  initializer_native
  snapshot_native
  completion_native
  startup_native
  native_check loom cargo test --offline --release -p cost-accounting-loom-models --test loom_recovery_pump_control
  rg -q 'test result: ok\. 6 passed; 0 failed; 0 ignored' "$pump_evidence/loom.log"
  native_check properties cargo test --offline --release -p cost-accounting-loom-models --test property_recovery_pump_control
  rg -q 'test result: ok\. 7 passed; 0 failed; 0 ignored' "$pump_evidence/properties.log"
  native_check queue cargo test --offline -p casper --lib rust::blocks::block_processing_queue
  rg -q 'test result: ok\. [1-9][0-9]* passed; 0 failed' "$pump_evidence/queue.log"
  native_check lifecycle cargo test --offline -p casper --test mod engine::admission_ownership_spec
  rg -q 'test result: ok\. 2 passed; 0 failed; 0 ignored' "$pump_evidence/lifecycle.log"
  native_check lint cargo clippy --offline -p casper -p node --lib --tests -- -D warnings
  native_check loom-lint cargo clippy --offline --release -p cost-accounting-loom-models --test loom_recovery_pump_control --test property_recovery_pump_control -- -D warnings
}

handoff_native() {
  native_check handoff-properties cargo test --offline --release -p cost-accounting-loom-models --test property_recovery_pump_control
  rg -q 'test result: ok\. 7 passed; 0 failed; 0 ignored' "$pump_evidence/handoff-properties.log"
  native_check handoff-loom cargo test --offline --release -p cost-accounting-loom-models --test loom_recovery_pump_control
  rg -q 'test result: ok\. 6 passed; 0 failed; 0 ignored' "$pump_evidence/handoff-loom.log"
  native_check handoff-lint cargo clippy --offline --release -p cost-accounting-loom-models --test property_recovery_pump_control --test loom_recovery_pump_control -- -D warnings
  native_check handoff-production-lint cargo clippy --offline -p casper -p node --lib --tests -- -D warnings
}

initializer_native() {
  native_check initializer cargo test --offline --release -p cost-accounting-loom-models --test initializer_ownership
  rg -q 'test result: ok\. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' "$pump_evidence/initializer.log"
  native_check initializer-lint cargo clippy --offline --release -p cost-accounting-loom-models --test initializer_ownership -- -D warnings
}

snapshot_native() {
  scan_native
  native_check snapshot-property cargo test --offline --release -p cost-accounting-loom-models --test property_startup_snapshot
  rg -q 'test result: ok\. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' "$pump_evidence/snapshot-property.log"
  native_check snapshot-loom cargo test --offline --release -p cost-accounting-loom-models --test loom_startup_snapshot
  rg -q 'test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' "$pump_evidence/snapshot-loom.log"
  native_check snapshot-buffer cargo test --offline -p block-storage --lib casper_buffer_key_value_storage
  rg -q 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored' "$pump_evidence/snapshot-buffer.log"
  native_check snapshot-dag cargo test --offline -p block-storage --lib doubly_linked_dag_operations
  rg -q 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored' "$pump_evidence/snapshot-dag.log"
  native_check snapshot-storage-lint cargo clippy --offline -p block-storage --lib --tests -- -D warnings
  native_check snapshot-lint cargo clippy --offline --release -p cost-accounting-loom-models --test property_startup_snapshot --test loom_startup_snapshot -- -D warnings
}

scan_native() {
  native_check scan-property cargo test --offline --release -p cost-accounting-loom-models --test property_startup_scan
  rg -q 'test result: ok\. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' "$pump_evidence/scan-property.log"
  native_check scan-lint cargo clippy --offline --release -p cost-accounting-loom-models --test property_startup_scan --test property_startup_snapshot --test loom_startup_snapshot -- -D warnings
}

completion_native() {
  native_check completion-property cargo test --offline --release -p cost-accounting-loom-models --test property_startup_completion
  rg -q 'test result: ok\. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' "$pump_evidence/completion-property.log"
  native_check completion-loom cargo test --offline --release -p cost-accounting-loom-models --test loom_startup_completion
  rg -q 'test result: ok\. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' "$pump_evidence/completion-loom.log"
  native_check completion-lint cargo clippy --offline --release -p cost-accounting-loom-models --test property_startup_completion --test loom_startup_completion -- -D warnings
}

startup_native() {
  native_check startup-runtime cargo test --offline --release -p cost-accounting-loom-models --test startup_runtime
  rg -q 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' "$pump_evidence/startup-runtime.log"
  native_check startup-runtime-lint cargo clippy --offline --release -p cost-accounting-loom-models --test startup_runtime -- -D warnings
  lease_native
}

lease_native() {
  native_check lease-property cargo test --offline --release -p cost-accounting-loom-models --test property_startup_snapshot_lease
  rg -q 'test result: ok\. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' "$pump_evidence/lease-property.log"
  native_check lease-loom cargo test --offline --release -p cost-accounting-loom-models --test loom_startup_snapshot_lease
  rg -q 'test result: ok\. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' "$pump_evidence/lease-loom.log"
  native_check lease-lint cargo clippy --offline --release -p cost-accounting-loom-models --test property_startup_snapshot_lease --test loom_startup_snapshot_lease -- -D warnings
}

case "$pump_mode" in
  all) formal; native ;;
  formal) formal ;;
  native) native ;;
  initializer) initializer_formal; initializer_native ;;
  initializer-native) initializer_native; native_check node-lint cargo clippy --offline -p node --lib --tests -- -D warnings ;;
  snapshot-formal) snapshot_formal ;;
  snapshot-native) snapshot_native ;;
  scan-native) scan_native ;;
  completion-formal) completion_formal ;;
  completion-native) scan_native; completion_native; native_check completion-casper-lint cargo clippy --offline -p casper --lib --tests -- -D warnings ;;
  publication-formal) publication_formal ;;
  lease-formal) lease_formal ;;
  metadata-formal) metadata_formal ;;
  metadata-native) metadata_native ;;
  handoff-formal) handoff_formal ;;
  handoff-native) handoff_native ;;
  lease-native) startup_native; native_check lease-casper-lint cargo clippy --offline -p casper -p node --lib --tests -- -D warnings ;;
  startup-native)
    startup_native
    native_check startup-publication-integration cargo test --offline -p casper --test mod engine::admission_ownership_spec::running_publication_announces_only_committed_engines
    rg -q 'test result: ok\. 1 passed; 0 failed; 0 ignored' "$pump_evidence/startup-publication-integration.log"
    native_check startup-casper-lint cargo clippy --offline -p casper -p node --lib --tests -- -D warnings
    ;;
esac

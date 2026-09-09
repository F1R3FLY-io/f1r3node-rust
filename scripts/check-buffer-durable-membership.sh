#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
mode=${1:-all}
case "$mode" in all|formal|tlc|rocq|native) ;; *) exit 2 ;; esac
buffer_cgroup=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
buffer_memory_file="/sys/fs/cgroup$buffer_cgroup/memory.max"
buffer_swap_file="/sys/fs/cgroup$buffer_cgroup/memory.swap.max"
if [[ ! -r "$buffer_memory_file" || ! -r "$buffer_swap_file" ]]; then
  echo 'Run this gate in a systemd scope with a finite memory limit and no swap.' >&2
  exit 2
fi
buffer_memory=$(<"$buffer_memory_file")
buffer_swap=$(<"$buffer_swap_file")
if [[ ! "$buffer_memory" =~ ^[1-9][0-9]*$ ]] \
   || (( buffer_memory > 8589934592 )) || [[ "$buffer_swap" != 0 ]]; then
  echo 'The scope must limit memory to at most 8 GiB and disable swap.' >&2
  exit 2
fi

mkdir -p target/verification/buffer-durable-membership
evidence=$(mktemp -d "$root/target/verification/buffer-durable-membership/run.XXXXXX")
mkdir -p "$evidence/tmp"
export TMPDIR="$evidence/tmp" CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1
unset LOOM_MAX_PREEMPTIONS LOOM_MAX_PERMUTATIONS LOOM_MAX_DURATION LOOM_CHECKPOINT_FILE
printf 'Evidence: %s\nMemoryMax: %s\nMemorySwapMax: %s\n' "$evidence" "$buffer_memory" "$buffer_swap"
sha256sum scripts/check-buffer-durable-membership.sh \
  formal/tlaplus/block_admission/BufferDurableMembership.tla \
  formal/tlaplus/block_admission/BufferDurableMembership*.cfg \
  formal/tlaplus/block_admission/BufferRetentionEpoch.tla \
  formal/tlaplus/block_admission/BufferRetentionEpoch*.cfg \
  formal/tlaplus/block_admission/BufferCandidateRotation.tla \
  formal/tlaplus/block_admission/BufferCandidateRotation*.cfg \
  formal/rocq/finalized_floor/theories/BufferDurableMembership.v \
  formal/rocq/finalized_floor/theories/BufferRetentionEpoch.v \
  formal/rocq/finalized_floor/theories/BufferCandidateRotation.v \
  block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage.rs \
  block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage/buffer_transaction.rs \
  block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage/candidate_rotation.rs \
  block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage/persistence_tests.rs \
  formal/loom/cost_accounting/tests/loom_buffer_transaction_publication.rs \
  formal/loom/cost_accounting/tests/loom_buffer_candidate_rotation.rs \
  Cargo.lock > "$evidence/inputs.sha256"
trap 'sha256sum --check "$evidence/inputs.sha256"' EXIT

rocq_check() {
  local pair proof expected
  mkdir -p "$evidence/rocq"
  for pair in BufferDurableMembership:15 BufferRetentionEpoch:6 BufferCandidateRotation:9; do
    proof=${pair%:*}
    expected=${pair#*:}
    coqc -Q "$evidence/rocq" FinalizedFloor \
      -o "$evidence/rocq/$proof.vo" "formal/rocq/finalized_floor/theories/$proof.v" \
      > "$evidence/rocq/$proof-compile.log" 2>&1 || {
      tail -n 30 "$evidence/rocq/$proof-compile.log"
      return 1
    }
    [[ $(rg -c '^Closed under the global context$' "$evidence/rocq/$proof-compile.log") == "$expected" ]]
    coqchk -Q "$evidence/rocq" FinalizedFloor "FinalizedFloor.$proof" \
      > "$evidence/rocq/$proof-kernel.log" 2>&1
    rg -q 'Modules were successfully checked' "$evidence/rocq/$proof-kernel.log"
    tail -n 12 "$evidence/rocq/$proof-kernel.log"
  done
}

tlc_case() {
  local name=$1 expected=$2 module=${3:-BufferDurableMembership} status
  mkdir -p "$evidence/$name"
  set +e
  java -Xmx1g -XX:+UseSerialGC -cp /usr/share/java/tla2tools.jar tlc2.TLC \
    -workers 1 -metadir "$evidence/$name" \
    -config "formal/tlaplus/block_admission/$name.cfg" \
    "formal/tlaplus/block_admission/$module.tla" \
    > "$evidence/$name.log" 2>&1
  status=$?
  set -e
  printf '%s exit=%s\n' "$name" "$status"
  tail -n 12 "$evidence/$name.log"
  if [[ "$expected" == safe ]]; then
    [[ "$status" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$evidence/$name.log"
  else
    [[ "$status" == 12 ]]
    rg -q "Invariant $expected is violated" "$evidence/$name.log"
  fi
}

tlc_check() {
  tlc_case BufferDurableMembershipReadyUnsafe Inv_DurableMembership
  tlc_case BufferDurableMembershipOrphanUnsafe Inv_DurableMembership
  tlc_case BufferDurableMembershipRestartUnsafe Inv_PublishedProjection
  tlc_case BufferDurableMembershipPublicationUnsafe Inv_PublishedProjection
  tlc_case BufferDurableMembershipAtomicUnsafe Inv_DurableMembership
  tlc_case BufferDurableMembership safe
  tlc_case BufferDurableMembershipThreeBlocks safe
  tlc_case BufferRetentionEpochAgeUnsafe Inv_StableAgeEpoch BufferRetentionEpoch
  tlc_case BufferRetentionEpochCountUnsafe Inv_CompletePressureCount BufferRetentionEpoch
  tlc_case BufferRetentionEpoch safe BufferRetentionEpoch
  tlc_case BufferCandidateRotationArrivalUnsafe Inv_NoOvertaking BufferCandidateRotation
  tlc_case BufferCandidateRotationDuplicateUnsafe Inv_NoOvertaking BufferCandidateRotation
  tlc_case BufferCandidateRotationPageUnsafe Inv_NoOvertaking BufferCandidateRotation
  tlc_case BufferCandidateRotation safe BufferCandidateRotation
}

run_check() {
  local name=$1
  shift
  if "$@" > "$evidence/$name.log" 2>&1; then
    tail -n 30 "$evidence/$name.log"
  else
    tail -n 90 "$evidence/$name.log"
    return 1
  fi
}

native_check() {
  run_check buffer-tests cargo test --offline -p block-storage --lib casper_buffer_key_value_storage
  rg -q 'test result: ok\. [1-9][0-9]* passed; 0 failed' "$evidence/buffer-tests.log"
  run_check atomic-transition cargo test --offline -p block-storage --test atomic_buffer_dag_transition
  rg -q 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' "$evidence/atomic-transition.log"
  run_check loom cargo test --offline --release -p cost-accounting-loom-models --test loom_buffer_transaction_publication
  rg -q 'test result: ok\. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' "$evidence/loom.log"
  run_check candidate-loom cargo test --offline --release -p cost-accounting-loom-models --test loom_buffer_candidate_rotation
  rg -q 'test result: ok\. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' "$evidence/candidate-loom.log"
  run_check storage-clippy cargo clippy --offline -p block-storage --lib --tests -- -D warnings
  run_check loom-clippy cargo clippy --offline --release -p cost-accounting-loom-models --test loom_buffer_transaction_publication --test loom_buffer_candidate_rotation -- -D warnings
}

case "$mode" in
  all) rocq_check; tlc_check; native_check ;;
  formal) rocq_check; tlc_check ;;
  rocq) rocq_check ;;
  tlc) tlc_check ;;
  native) native_check ;;
esac

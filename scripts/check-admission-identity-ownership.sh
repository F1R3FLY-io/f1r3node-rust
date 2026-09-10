#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
identity_mode=${1:-all}
case "$identity_mode" in all|formal|native|lifecycle) ;; *) exit 2 ;; esac
identity_cgroup=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
identity_memory=$(<"/sys/fs/cgroup$identity_cgroup/memory.max")
identity_swap=$(<"/sys/fs/cgroup$identity_cgroup/memory.swap.max")
if [[ ! "$identity_memory" =~ ^[1-9][0-9]*$ ]] \
   || (( identity_memory > 8589934592 )) || [[ "$identity_swap" != 0 ]]; then
  echo 'Use a systemd scope with MemoryMax at most 8 GiB and MemorySwapMax=0.' >&2
  exit 2
fi
mkdir -p target/verification/admission-identity
identity_evidence=$(mktemp -d "$PWD/target/verification/admission-identity/run.XXXXXX")
mkdir -p "$identity_evidence/rocq" "$identity_evidence/tmp"
export TMPDIR="$identity_evidence/tmp" CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1
unset LOOM_MAX_PREEMPTIONS LOOM_MAX_PERMUTATIONS LOOM_MAX_DURATION LOOM_CHECKPOINT_FILE
printf 'Evidence: %s\nMemoryMax: %s\n' "$identity_evidence" "$identity_memory"
sha256sum scripts/check-admission-identity-ownership.sh \
  formal/tlaplus/block_admission/AdmissionIdentityOwnership* \
  formal/rocq/finalized_floor/theories/AdmissionIdentityOwnership.v \
  casper/src/rust/blocks/block_processing_queue.rs \
  casper/src/rust/blocks/block_processing_queue/admission_identity.rs \
  casper/src/rust/blocks/block_processing_queue/admission_budget.rs \
  casper/src/rust/engine/{running,engine,casper_launch,initializing,genesis_validator,genesis_ceremony_master}.rs \
  casper/tests/engine/{setup,mod,running_spec,admission_ownership_spec}.rs \
  casper/tests/helper/test_node.rs \
  node/src/rust/instances/block_processor_instance.rs \
  node/src/rust/runtime/{node_runtime,setup}.rs \
  formal/loom/cost_accounting/tests/loom_admission_identity.rs \
  Cargo.lock \
  > "$identity_evidence/inputs.sha256"
trap 'sha256sum --check "$identity_evidence/inputs.sha256"' EXIT
formal() {
coqc -Q "$identity_evidence/rocq" FinalizedFloor \
  -o "$identity_evidence/rocq/AdmissionIdentityOwnership.vo" \
  formal/rocq/finalized_floor/theories/AdmissionIdentityOwnership.v \
  > "$identity_evidence/rocq/compile.log" 2>&1 || {
  tail -n 30 "$identity_evidence/rocq/compile.log"
  exit 1
}
[[ $(rg -c '^Closed under the global context$' "$identity_evidence/rocq/compile.log") == 12 ]]
coqchk -Q "$identity_evidence/rocq" FinalizedFloor FinalizedFloor.AdmissionIdentityOwnership \
  > "$identity_evidence/rocq/kernel.log" 2>&1
rg -q 'Modules were successfully checked' "$identity_evidence/rocq/kernel.log"
for identity_variant in QueuedUnsafe StaleUnsafe DuplicateUnsafe ProducerUnsafe ''; do
  identity_name="AdmissionIdentityOwnership$identity_variant"
  mkdir -p "$identity_evidence/$identity_name"
  set +e
  java -Xmx1g -XX:+UseSerialGC -cp /usr/share/java/tla2tools.jar tlc2.TLC \
    -workers 1 -metadir "$identity_evidence/$identity_name" \
    -config "formal/tlaplus/block_admission/$identity_name.cfg" \
    formal/tlaplus/block_admission/AdmissionIdentityOwnership.tla \
    > "$identity_evidence/$identity_name.log" 2>&1
  identity_result=$?
  set -e
  tail -n 12 "$identity_evidence/$identity_name.log"
  if [[ -z "$identity_variant" ]]; then
    [[ "$identity_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$identity_evidence/$identity_name.log"
  else
    [[ "$identity_result" == 12 ]]
    if [[ "$identity_variant" == ProducerUnsafe ]]; then
      rg -q 'Invariant Inv_PayloadCovered is violated' "$identity_evidence/$identity_name.log"
    else
      rg -q 'Invariant Inv_ExactLiveOwnership is violated' "$identity_evidence/$identity_name.log"
    fi
  fi
done
}

native_check() {
  local identity_name=$1
  shift
  if "$@" > "$identity_evidence/$identity_name.log" 2>&1; then
    tail -n 12 "$identity_evidence/$identity_name.log"
  else
    tail -n 65 "$identity_evidence/$identity_name.log"
    return 1
  fi
}

native() {
  native_check loom cargo test --offline --release -p cost-accounting-loom-models --test loom_admission_identity
  rg -q 'test result: ok\. 4 passed; 0 failed; 0 ignored' "$identity_evidence/loom.log"
  native_check queue cargo test --offline -p casper --lib rust::blocks::block_processing_queue
  rg -q 'test result: ok\. [1-9][0-9]* passed; 0 failed' "$identity_evidence/queue.log"
  native_check lifecycle cargo test --offline -p casper --test mod engine::admission_ownership_spec
  rg -q 'test result: ok\. 1 passed; 0 failed; 0 ignored' "$identity_evidence/lifecycle.log"
  producer_checks
  native_check lint cargo clippy --offline -p casper -p node --lib --tests -- -D warnings
  native_check loom-lint cargo clippy --offline --release -p cost-accounting-loom-models --test loom_admission_identity -- -D warnings
}

producer_checks() {
  native_check producer cargo test --offline -p casper --test mod engine::running_spec::tests::engine_should_enqueue_block_message_for_processing -- --exact
  rg -q 'test result: ok\. 1 passed; 0 failed; 0 ignored' "$identity_evidence/producer.log"
  native_check duplicate cargo test --offline -p casper --test mod engine::running_spec::tests::duplicate_block_message_is_enqueued_only_once -- --exact
  rg -q 'test result: ok\. 1 passed; 0 failed; 0 ignored' "$identity_evidence/duplicate.log"
}

lifecycle() {
  native_check lifecycle cargo test --offline -p casper --test mod engine::admission_ownership_spec
  rg -q 'test result: ok\. 1 passed; 0 failed; 0 ignored' "$identity_evidence/lifecycle.log"
  producer_checks
  native_check lint cargo clippy --offline -p casper --test mod -- -D warnings
}

case "$identity_mode" in
  all) formal; native ;;
  formal) formal ;;
  native) native ;;
  lifecycle) lifecycle ;;
esac

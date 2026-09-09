#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
pruning_mode=${1:-all}
case "$pruning_mode" in all|formal|native) ;; *) exit 2 ;; esac
pruning_cgroup=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
pruning_memory=$(<"/sys/fs/cgroup$pruning_cgroup/memory.max")
pruning_swap=$(<"/sys/fs/cgroup$pruning_cgroup/memory.swap.max")
if [[ ! "$pruning_memory" =~ ^[1-9][0-9]*$ ]] \
   || (( pruning_memory > 8589934592 )) || [[ "$pruning_swap" != 0 ]]; then
  echo 'Use a systemd scope with MemoryMax at most 8 GiB and MemorySwapMax=0.' >&2
  exit 2
fi
mkdir -p target/verification/buffer-pruning
pruning_evidence=$(mktemp -d "$PWD/target/verification/buffer-pruning/run.XXXXXX")
mkdir -p "$pruning_evidence/tmp"
export TMPDIR="$pruning_evidence/tmp" CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1
printf 'Evidence: %s\nMemoryMax: %s\n' "$pruning_evidence" "$pruning_memory"
sha256sum scripts/check-buffer-pruning-obligations.sh \
  formal/tlaplus/block_admission/BufferPruningObligations* \
  formal/rocq/finalized_floor/theories/BufferPruningPreservation.v \
  block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage.rs \
  block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage/persistence_tests.rs \
  casper/src/rust/blocks/block_processor.rs \
  casper/src/rust/engine/multi_parent_casper/dispatch.rs \
  casper/src/rust/engine/block_retriever.rs \
  > "$pruning_evidence/inputs.sha256"
pruning_finish() {
  local pruning_result=$?
  trap - EXIT
  if ! sha256sum --check "$pruning_evidence/inputs.sha256" > "$pruning_evidence/inputs-check.log"; then
    cat "$pruning_evidence/inputs-check.log"
    pruning_result=1
  fi
  printf 'Gate exit: %s\n' "$pruning_result"
  exit "$pruning_result"
}
trap pruning_finish EXIT
if [[ "$pruning_mode" != native ]]; then
for pruning_variant in ResolveUnsafe RetryUnsafe CertificateUnsafe RestartUnsafe '' Shared Isolated Join; do
  pruning_name="BufferPruningObligations$pruning_variant"
  mkdir -p "$pruning_evidence/$pruning_name"
  set +e
  java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
    -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
    -metadir "$pruning_evidence/$pruning_name" \
    -config "formal/tlaplus/block_admission/$pruning_name.cfg" \
    formal/tlaplus/block_admission/BufferPruningObligations.tla \
    > "$pruning_evidence/$pruning_name.log" 2>&1
  pruning_result=$?
  set -e
  tail -n 10 "$pruning_evidence/$pruning_name.log"
  case "$pruning_variant" in
    ResolveUnsafe) pruning_expected=Inv_Obligations ;;
    RetryUnsafe) pruning_expected=Inv_DurableRetry ;;
    CertificateUnsafe) pruning_expected=Inv_CertificateCoverage ;;
    RestartUnsafe) pruning_expected=Inv_ResidentBound ;;
    *) pruning_expected='' ;;
  esac
  if [[ -n "$pruning_expected" ]]; then
    [[ "$pruning_result" == 12 ]]
    rg "Invariant $pruning_expected is violated" "$pruning_evidence/$pruning_name.log"
  else
    [[ "$pruning_result" == 0 ]]
    rg -q 'Model checking completed. No error has been found.' "$pruning_evidence/$pruning_name.log"
  fi
done
mkdir -p "$pruning_evidence/rocq"
coqc -Q "$pruning_evidence/rocq" FinalizedFloor \
  -o "$pruning_evidence/rocq/BufferPruningPreservation.vo" \
  formal/rocq/finalized_floor/theories/BufferPruningPreservation.v \
  > "$pruning_evidence/rocq/compile.log" 2>&1 || {
  tail -n 35 "$pruning_evidence/rocq/compile.log"
  exit 1
}
[[ $(rg -c '^Closed under the global context$' "$pruning_evidence/rocq/compile.log") == 14 ]]
coqchk -Q "$pruning_evidence/rocq" FinalizedFloor FinalizedFloor.BufferPruningPreservation \
  > "$pruning_evidence/rocq/kernel.log" 2>&1
rg 'Modules were successfully checked' "$pruning_evidence/rocq/kernel.log"
fi
if [[ "$pruning_mode" != formal ]]; then
  cargo test --offline -p block-storage --lib persistence_tests::pruning_preserves_ -- --test-threads=1 \
    > "$pruning_evidence/native.log" 2>&1 || {
    tail -n 90 "$pruning_evidence/native.log"
    exit 1
  }
  rg 'test result: ok\. 4 passed; 0 failed; 0 ignored' "$pruning_evidence/native.log"
  cargo clippy --offline -p block-storage --lib --tests -- -D warnings \
    > "$pruning_evidence/lint.log" 2>&1 || {
    tail -n 45 "$pruning_evidence/lint.log"
    exit 1
  }
fi

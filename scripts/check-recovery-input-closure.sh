#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
closure_cgroup=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
closure_memory=$(<"/sys/fs/cgroup$closure_cgroup/memory.max")
closure_swap=$(<"/sys/fs/cgroup$closure_cgroup/memory.swap.max")
if [[ ! "$closure_memory" =~ ^[1-9][0-9]*$ ]] \
   || (( closure_memory > 8589934592 )) || [[ "$closure_swap" != 0 ]]; then
  echo 'Use a systemd scope with MemoryMax at most 8 GiB and MemorySwapMax=0.' >&2
  exit 2
fi
mkdir -p target/verification/recovery-dispatcher
closure_evidence=$(mktemp -d "$PWD/target/verification/recovery-dispatcher/closure.XXXXXX")
mkdir -p "$closure_evidence/tmp"
export TMPDIR="$closure_evidence/tmp"
printf 'Evidence: %s\nMemoryMax: %s\n' "$closure_evidence" "$closure_memory"
sha256sum scripts/check-recovery-input-closure.sh \
  formal/tlaplus/block_admission/RecoveryInputClosure* \
  node/src/rust/instances/block_processor_instance.rs \
  > "$closure_evidence/inputs.sha256"
closure_finish() {
  local closure_result=$?
  trap - EXIT
  if ! sha256sum --check "$closure_evidence/inputs.sha256" > "$closure_evidence/inputs-check.log"; then
    cat "$closure_evidence/inputs-check.log"
    closure_result=1
  fi
  printf 'Gate exit: %s\n' "$closure_result"
  exit "$closure_result"
}
trap closure_finish EXIT
for closure_variant in '' WorkerSlotUnsafe ResetDeadlineUnsafe DequeueUnsafe; do
  closure_name="RecoveryInputClosure$closure_variant"
  mkdir -p "$closure_evidence/$closure_name"
  set +e
  java -Xmx1g -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
    -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
    -metadir "$closure_evidence/$closure_name" \
    -config "formal/tlaplus/block_admission/$closure_name.cfg" \
    formal/tlaplus/block_admission/RecoveryInputClosure.tla \
    > "$closure_evidence/$closure_name.log" 2>&1
  closure_result=$?
  set -e
  tail -n 12 "$closure_evidence/$closure_name.log"
  case "$closure_variant" in
    '')
      [[ "$closure_result" == 0 ]]
      rg -q 'Model checking completed. No error has been found.' "$closure_evidence/$closure_name.log"
      ;;
    DequeueUnsafe)
      [[ "$closure_result" == 12 ]]
      rg -q 'Invariant Inv_NoExtraBody is violated' "$closure_evidence/$closure_name.log"
      ;;
    *)
      [[ "$closure_result" == 13 ]]
      rg -q 'Temporal properties were violated' "$closure_evidence/$closure_name.log"
      ;;
  esac
done

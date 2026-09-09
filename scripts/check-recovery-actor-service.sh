#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
mode=${1:-all}
case "$mode" in all|formal|rocq|tlc|apalache|native|casper) ;; *) exit 2 ;; esac
actor_cgroup=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
actor_memory_file="/sys/fs/cgroup$actor_cgroup/memory.max"
actor_swap_file="/sys/fs/cgroup$actor_cgroup/memory.swap.max"
if [[ ! -r "$actor_memory_file" || ! -r "$actor_swap_file" ]]; then
  echo 'Run this gate in a systemd scope with a finite memory limit and no swap.' >&2
  exit 2
fi
actor_memory=$(<"$actor_memory_file")
actor_swap=$(<"$actor_swap_file")
if [[ ! "$actor_memory" =~ ^[1-9][0-9]*$ ]] \
   || (( actor_memory > 8589934592 )) || [[ "$actor_swap" != 0 ]]; then
  echo 'The scope must limit memory to at most 8 GiB and disable swap.' >&2
  exit 2
fi

mkdir -p target/verification/recovery-actor-service
evidence=$(mktemp -d "$root/target/verification/recovery-actor-service/run.XXXXXX")
mkdir -p "$evidence/tmp"
export TMPDIR="$evidence/tmp" CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1
export JVM_ARGS=-Xmx2g
unset LOOM_MAX_PREEMPTIONS LOOM_MAX_PERMUTATIONS LOOM_MAX_DURATION LOOM_CHECKPOINT_FILE
printf 'Evidence: %s\nMemoryMax: %s\nMemorySwapMax: %s\n' "$evidence" "$actor_memory" "$actor_swap"
inputs=(
  scripts/check-recovery-actor-service.sh
  formal/rocq/finalized_floor/theories/RecoveryActorService.v
  formal/tlaplus/block_admission/RecoveryActorService.tla
  formal/tlaplus/block_admission/RecoveryActorService.cfg
  formal/tlaplus/block_admission/RecoveryActorServiceParallel.cfg
  formal/tlaplus/block_admission/RecoveryActorServicePriorityUnsafe.cfg
  formal/tlaplus/block_admission/RecoveryActorServiceShutdownUnsafe.cfg
  formal/tlaplus/block_admission/RecoveryActorServiceApalache.cfg
  casper/src/rust/engine/recovery_service_rotation.rs
  casper/src/rust/engine/recovery_actor_inbox.rs
  casper/src/rust/engine/runtime_state_requester.rs
  casper/src/rust/engine/mod.rs
  casper/Cargo.toml
  formal/loom/cost_accounting/Cargo.toml
  formal/loom/cost_accounting/tests/recovery_actor_service.rs
  formal/loom/cost_accounting/tests/loom_recovery_service_rotation.rs
  Cargo.lock
)
sha256sum "${inputs[@]}" > "$evidence/inputs.sha256"
trap 'sha256sum --check "$evidence/inputs.sha256"' EXIT

rocq_check() {
  mkdir -p "$evidence/rocq"
  coqc -Q "$evidence/rocq" FinalizedFloor \
    -o "$evidence/rocq/RecoveryActorService.vo" \
    formal/rocq/finalized_floor/theories/RecoveryActorService.v \
    2>&1 | tee "$evidence/rocq/compile.log"
  [[ $(rg -c '^Closed under the global context$' "$evidence/rocq/compile.log") == 11 ]]
  coqchk -Q "$evidence/rocq" FinalizedFloor FinalizedFloor.RecoveryActorService \
    > "$evidence/rocq/kernel.log" 2>&1
  rg -q 'Modules were successfully checked' "$evidence/rocq/kernel.log"
  tail -n 15 "$evidence/rocq/kernel.log"
}

tlc_case() {
  local name=$1 expected=$2 status
  mkdir -p "$evidence/$name"
  set +e
  java -Xmx1g -XX:+UseSerialGC -cp /usr/share/java/tla2tools.jar tlc2.TLC \
    -workers 1 -metadir "$evidence/$name" \
    -config "formal/tlaplus/block_admission/$name.cfg" \
    formal/tlaplus/block_admission/RecoveryActorService.tla \
    > "$evidence/$name.log" 2>&1
  status=$?
  set -e
  printf '%s exit=%s\n' "$name" "$status"
  case "$expected" in
    safe)
      [[ "$status" == 0 ]]
      rg -q 'Model checking completed. No error has been found.' "$evidence/$name.log"
      ;;
    gap)
      [[ "$status" == 12 ]]
      rg -q 'Invariant Inv_BoundedServiceGap is violated' "$evidence/$name.log"
      ;;
    shutdown)
      [[ "$status" == 13 ]]
      rg -q 'Temporal properties were violated' "$evidence/$name.log"
      rg -q '^    Live_ClosedInputsTerminate$' "formal/tlaplus/block_admission/$name.cfg"
      ;;
  esac
  tail -n 12 "$evidence/$name.log"
}

tlc_check() {
  tlc_case RecoveryActorService safe
  tlc_case RecoveryActorServicePriorityUnsafe gap
  tlc_case RecoveryActorServiceShutdownUnsafe shutdown
  tlc_case RecoveryActorServiceParallel safe
}

apalache_case() {
  local name=$1
  shift
  apalache-mc --out-dir="$evidence/$name" check \
    --config=formal/tlaplus/block_admission/RecoveryActorServiceApalache.cfg \
    "$@" formal/tlaplus/block_admission/RecoveryActorService.tla \
    2>&1 | tee "$evidence/$name.log"
  rg -q 'The outcome is: NoError' "$evidence/$name.log"
  rg -q 'EXITCODE: OK' "$evidence/$name.log"
}

apalache_check() {
  apalache_case reachable --length=8
  apalache_case inductive --init=Safety --length=1
}

native_check() {
  cargo test --offline --release -p cost-accounting-loom-models \
    --test recovery_actor_service --test loom_recovery_service_rotation \
    2>&1 | tee "$evidence/native.log"
  [[ $(rg -c 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' "$evidence/native.log") == 2 ]]
  cargo clippy --offline --release -p cost-accounting-loom-models \
    --test recovery_actor_service --test loom_recovery_service_rotation -- -D warnings \
    2>&1 | tee "$evidence/native-clippy.log"
}

casper_check() {
  cargo test --offline -p casper --lib runtime_state_requester::tests \
    2>&1 | tee "$evidence/casper.log"
  rg -q 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored;' "$evidence/casper.log"
  cargo clippy --offline -p casper --lib -- -D warnings \
    2>&1 | tee "$evidence/casper-clippy.log"
}

case "$mode" in
  all) rocq_check; tlc_check; apalache_check; native_check; casper_check ;;
  formal) rocq_check; tlc_check; apalache_check ;;
  rocq) rocq_check ;;
  tlc) tlc_check ;;
  apalache) apalache_check ;;
  native) native_check ;;
  casper) casper_check ;;
esac

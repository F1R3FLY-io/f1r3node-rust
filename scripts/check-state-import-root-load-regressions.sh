#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
import_expected=${1:-green}
case "$import_expected" in red|green) ;; *) echo 'Expected mode: red or green.' >&2; exit 2 ;; esac
import_cgroup=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
import_memory=$(<"/sys/fs/cgroup$import_cgroup/memory.max")
import_swap=$(<"/sys/fs/cgroup$import_cgroup/memory.swap.max")
if [[ ! "$import_memory" =~ ^[1-9][0-9]*$ ]] \
    || (( import_memory > 2147483648 )) || [[ "$import_swap" != 0 ]]; then
    echo 'Use a systemd scope with MemoryMax at most 2 GiB and MemorySwapMax=0.' >&2
    exit 2
fi
mkdir -p target/verification/state-import
import_evidence=$(mktemp -d "$PWD/target/verification/state-import/root-load.XXXXXX")
mkdir -p "$import_evidence/tmp"
export TMPDIR="$import_evidence/tmp"
export CARGO_BUILD_JOBS=1
printf 'Evidence: %s\nMode: %s\nMemoryMax: %s\n' "$import_evidence" "$import_expected" "$import_memory"
printf '%s\n' "$import_expected" > "$import_evidence/mode"
sha256sum scripts/check-state-import-root-load-regressions.sh \
    formal/rocq/cost_accounted_rho/theories/StateImportCodec.v \
    casper/src/rust/engine/horizon_state_import_tests.rs \
    rspace++/src/rspace/history/history_repository_impl.rs \
    rspace++/src/rspace/history/instances/radix_history.rs \
    rspace++/src/rspace/history/radix_tree.rs \
    rspace++/src/rspace/history/history_repository.rs \
    rspace++/src/rspace/history/root_repository.rs \
    rspace++/src/rspace/history/roots_store.rs \
    Cargo.lock > "$import_evidence/inputs.sha256"
import_finish() {
    local import_result=$?
    trap - EXIT
    if ! sha256sum -c "$import_evidence/inputs.sha256" > "$import_evidence/inputs-check.log"; then
        import_result=1
    fi
    printf '%s\n' "$import_result" > "$import_evidence/gate.exit"
    printf 'Gate exit: %s\n' "$import_result"
    exit "$import_result"
}
trap import_finish EXIT
set +e
cargo test --locked -p casper --lib \
    rust::engine::lfs_horizon_requester::import_validity_tests::state_import_checkpoint_ \
    -- --test-threads=1 > "$import_evidence/native.log" 2>&1
import_native=$?
set -e
printf '%s\n' "$import_native" > "$import_evidence/native.exit"
tail -n 65 "$import_evidence/native.log"
if [[ "$import_expected" == red ]]; then
    test "$import_native" -eq 101
    rg -Fq 'test result: FAILED. 1 passed; 5 failed' "$import_evidence/native.log"
    rg -Fq 'wrong-hash: accepted invalid history' "$import_evidence/native.log"
    rg -Fq 'malformed-node: panicked instead of returning a storage error' "$import_evidence/native.log"
else
    test "$import_native" -eq 0
    rg -Fq 'test result: ok. 6 passed; 0 failed; 0 ignored' "$import_evidence/native.log"
fi
set +e
cargo test --locked -p casper --lib \
    rust::engine::lfs_horizon_requester::import_validity_tests::state_import_codec_ \
    -- --test-threads=1 > "$import_evidence/codec.log" 2>&1
import_codec=$?
set -e
printf '%s\n' "$import_codec" > "$import_evidence/codec.exit"
tail -n 45 "$import_evidence/codec.log"
if [[ "$import_expected" == red ]]; then
    test "$import_codec" -eq 101
    rg -Fq 'test result: FAILED. 2 passed; 3 failed' "$import_evidence/codec.log"
    rg -Fq 'invalid radix input must return an error, not panic' "$import_evidence/codec.log"
    rg -Fq 'invalid radix input must not authenticate' "$import_evidence/codec.log"
else
    test "$import_codec" -eq 0
    rg -Fq 'test result: ok. 5 passed; 0 failed; 0 ignored' "$import_evidence/codec.log"
fi
cargo clippy --locked -p casper --lib --tests -- -D warnings > "$import_evidence/clippy.log" 2>&1
cargo fmt --all -- --check > "$import_evidence/format.log" 2>&1

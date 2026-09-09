#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
handoff_expected=${1:-green}
case "$handoff_expected" in red|green) ;; *) echo 'Expected mode: red or green.' >&2; exit 2 ;; esac
handoff_cgroup=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
handoff_memory=$(<"/sys/fs/cgroup$handoff_cgroup/memory.max")
handoff_swap=$(<"/sys/fs/cgroup$handoff_cgroup/memory.swap.max")
if [[ ! "$handoff_memory" =~ ^[1-9][0-9]*$ ]] \
    || (( handoff_memory > 2147483648 )) || [[ "$handoff_swap" != 0 ]]; then
    echo 'Use a systemd scope with MemoryMax at most 2 GiB and MemorySwapMax=0.' >&2
    exit 2
fi
mkdir -p target/verification/state-import
handoff_evidence=$(mktemp -d "$PWD/target/verification/state-import/checkpoint-handoff.XXXXXX")
mkdir -p "$handoff_evidence/tmp"
export TMPDIR="$handoff_evidence/tmp"
export CARGO_BUILD_JOBS=1
printf 'Evidence: %s\nMode: %s\nMemoryMax: %s\n' "$handoff_evidence" "$handoff_expected" "$handoff_memory"
printf '%s\n' "$handoff_expected" > "$handoff_evidence/mode"
sha256sum scripts/check-state-import-checkpoint-handoff-regressions.sh \
    rspace++/src/rspace/rspace/tests.rs \
    rspace++/src/rspace/rspace/tests/checkpoint_failure_tests.rs \
    rspace++/src/rspace/rspace.rs \
    rspace++/src/rspace/rspace/ispace_impl.rs \
    rspace++/src/rspace/rspace/setup.rs \
    rspace++/src/rspace/rspace/trace_log.rs \
    rspace++/src/rspace/rspace/ops_install.rs \
    rspace++/src/rspace/replay_rspace.rs \
    rspace++/src/rspace/hot_store.rs \
    rspace++/src/rspace/history/history_repository_impl.rs \
    rspace++/src/rspace/history/instances/radix_history.rs \
    rspace++/src/rspace/history/radix_tree.rs \
    rspace++/src/rspace/history/history_repository.rs \
    rspace++/src/rspace/history/root_repository.rs \
    rspace++/src/rspace/history/roots_store.rs \
    shared/src/rust/store/key_value_store.rs \
    shared/src/rust/store/in_memory_key_value_store.rs \
    Cargo.lock > "$handoff_evidence/inputs.sha256"
handoff_finish() {
    local handoff_result=$?
    trap - EXIT
    if ! sha256sum -c "$handoff_evidence/inputs.sha256" > "$handoff_evidence/inputs-check.log"; then
        handoff_result=1
    fi
    printf '%s\n' "$handoff_result" > "$handoff_evidence/gate.exit"
    printf 'Gate exit: %s\n' "$handoff_result"
    exit "$handoff_result"
}
trap handoff_finish EXIT
set +e
cargo test --locked -p rspace_plus_plus --lib checkpoint_handoff_ \
    -- --test-threads=1 > "$handoff_evidence/native.log" 2>&1
handoff_native=$?
set -e
printf '%s\n' "$handoff_native" > "$handoff_evidence/native.exit"
tail -n 65 "$handoff_evidence/native.log"
if [[ "$handoff_expected" == red ]]; then
    test "$handoff_native" -eq 101
    rg -Fq 'test result: FAILED. 1 passed; 2 failed' "$handoff_evidence/native.log"
    rg -Fq 'checkpoint read failure lost local state:' "$handoff_evidence/native.log"
    rg -Fq 'checkpoint read failure lost replay state:' "$handoff_evidence/native.log"
else
    test "$handoff_native" -eq 0
    rg -Fq 'test result: ok. 3 passed; 0 failed; 0 ignored' "$handoff_evidence/native.log"
fi
cargo clippy --locked -p rspace_plus_plus --lib --tests -- -D warnings > "$handoff_evidence/clippy.log" 2>&1
cargo fmt --all -- --check > "$handoff_evidence/format.log" 2>&1

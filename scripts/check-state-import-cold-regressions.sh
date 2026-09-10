#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
import_cgroup=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
import_memory=$(<"/sys/fs/cgroup$import_cgroup/memory.max")
import_swap=$(<"/sys/fs/cgroup$import_cgroup/memory.swap.max")
if [[ ! "$import_memory" =~ ^[1-9][0-9]*$ ]] \
    || (( import_memory > 5368709120 )) || [[ "$import_swap" != 0 ]]; then
    echo 'Use a systemd scope with MemoryMax at most 5 GiB and MemorySwapMax=0.' >&2
    exit 2
fi
mkdir -p target/verification/state-import
import_evidence=$(mktemp -d "$PWD/target/verification/state-import/cold-regressions.XXXXXX")
mkdir -p "$import_evidence/tmp"
export TMPDIR="$import_evidence/tmp"
export CARGO_BUILD_JOBS=1
printf 'Evidence: %s\nMemoryMax: %s\n' "$import_evidence" "$import_memory"
sha256sum scripts/check-state-import-cold-regressions.sh \
    casper/src/rust/engine/runtime_state_requester.rs \
    casper/src/rust/engine/runtime_state_import_tests.rs \
    rspace++/src/rspace/state/rspace_importer.rs \
    rspace++/src/rspace/state/instances/rspace_importer_store.rs \
    rspace++/src/rspace/history/cold_store.rs \
    rspace++/src/rspace/history/instances/rspace_history_reader_impl.rs \
    rspace++/src/rspace/history/history_repository_impl.rs \
    rspace++/src/rspace/serializers/serializers.rs \
    Cargo.lock > "$import_evidence/inputs.sha256"
set +e
cargo test --locked -p casper --lib \
    rust::engine::runtime_state_requester::import_validity_tests:: \
    -- --test-threads=1 > "$import_evidence/native.log" 2>&1
import_tests=$?
cargo clippy --locked -p casper --lib --tests -- -D warnings \
    > "$import_evidence/clippy.log" 2>&1
import_clippy=$?
rustfmt --edition 2021 --check casper/src/rust/engine/runtime_state_import_tests.rs \
    > "$import_evidence/format.log" 2>&1
import_format=$?
sha256sum -c "$import_evidence/inputs.sha256" > "$import_evidence/inputs-check.log"
import_inputs=$?
set -e
printf '%s\n' "$import_tests" > "$import_evidence/test-exit-code"
printf '%s\n' "$import_clippy" > "$import_evidence/clippy-exit-code"
printf '%s\n' "$import_format" > "$import_evidence/format-exit-code"
cat "$import_evidence/native.log" "$import_evidence/clippy.log" \
    "$import_evidence/format.log" "$import_evidence/inputs-check.log"
printf 'Tests: %s\nClippy: %s\nFormat: %s\nInputs: %s\n' \
    "$import_tests" "$import_clippy" "$import_format" "$import_inputs"
if (( import_tests != 0 || import_clippy != 0 || import_format != 0 || import_inputs != 0 )); then
    exit 1
fi

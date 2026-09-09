#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
import_cgroup=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
import_memory=$(<"/sys/fs/cgroup$import_cgroup/memory.max")
import_swap=$(<"/sys/fs/cgroup$import_cgroup/memory.swap.max")
if [[ ! "$import_memory" =~ ^[1-9][0-9]*$ ]] \
    || (( import_memory > 2147483648 )) || [[ "$import_swap" != 0 ]]; then
    echo 'Use a systemd scope with MemoryMax at most 2 GiB and MemorySwapMax=0.' >&2
    exit 2
fi
for partial_limit in LOOM_MAX_PERMUTATIONS LOOM_MAX_DURATION LOOM_CHECKPOINT_FILE; do
    if [[ -v $partial_limit ]]; then
        echo "Qualification rejects partial exploration: $partial_limit" >&2
        exit 2
    fi
done
mkdir -p target/verification/state-import
import_evidence=$(mktemp -d "$PWD/target/verification/state-import/retry-regressions.XXXXXX")
mkdir -p "$import_evidence/tmp"
export TMPDIR="$import_evidence/tmp"
export CARGO_BUILD_JOBS=1
printf 'Evidence: %s\nMemoryMax: %s\n' "$import_evidence" "$import_memory"
sha256sum scripts/check-state-import-retry-regressions.sh \
    formal/rocq/cost_accounted_rho/theories/StateImportRetry.v \
    formal/loom/cost_accounting/tests/loom_production_sparse_transaction.rs \
    rspace++/src/rspace/shared/sparse_transaction.rs \
    rspace++/src/rspace/shared/in_mem_key_value_store.rs \
    rspace++/src/rspace/hashing/blake2b256_hash.rs \
    shared/src/rust/store/key_value_store.rs \
    shared/src/rust/store/lmdb_key_value_store.rs Cargo.lock \
    > "$import_evidence/inputs.sha256"
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
cargo test --locked -p shared --lib state_import_duplicate_cas_guards_ \
    -- --test-threads=1 > "$import_evidence/lmdb.log" 2>&1
rg -Fq 'test result: ok. 1 passed' "$import_evidence/lmdb.log"
cargo test --locked -p rspace_plus_plus --lib state_import_retry_budget_ \
    -- --test-threads=1 > "$import_evidence/property.log" 2>&1
rg -Fq 'test result: ok. 2 passed' "$import_evidence/property.log"
RUSTFLAGS='--cfg loom -C target-cpu=native' cargo test --locked -p cost-accounting-loom-models \
    --test loom_production_sparse_transaction state_import_retry_ \
    -- --test-threads=1 > "$import_evidence/loom.log" 2>&1
rg -Fq 'test result: ok. 2 passed' "$import_evidence/loom.log"
cargo clippy --locked -p shared -p rspace_plus_plus --lib --tests -- -D warnings \
    > "$import_evidence/clippy.log" 2>&1
RUSTFLAGS='--cfg loom -C target-cpu=native' cargo clippy --locked -p cost-accounting-loom-models \
    --test loom_production_sparse_transaction -- -D warnings \
    > "$import_evidence/loom-clippy.log" 2>&1
rustfmt --edition 2021 --check rspace++/src/rspace/shared/in_mem_key_value_store.rs \
    shared/src/rust/store/lmdb_key_value_store.rs \
    formal/loom/cost_accounting/tests/loom_production_sparse_transaction.rs \
    > "$import_evidence/format.log" 2>&1
tail -n 8 "$import_evidence/lmdb.log" "$import_evidence/property.log" "$import_evidence/loom.log"

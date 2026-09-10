#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cache_cgroup=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
cache_memory=$(<"/sys/fs/cgroup$cache_cgroup/memory.max")
cache_swap=$(<"/sys/fs/cgroup$cache_cgroup/memory.swap.max")
if [[ ! "$cache_memory" =~ ^[1-9][0-9]*$ ]] \
   || (( cache_memory > 2147483648 )) || [[ "$cache_swap" != 0 ]]; then
    echo 'Use a systemd scope with MemoryMax at most 2 GiB and MemorySwapMax=0.' >&2
    exit 2
fi
mkdir -p target/verification/runtime-cache-lock
cache_evidence=$(mktemp -d "$PWD/target/verification/runtime-cache-lock/formal.XXXXXX")
mkdir -p "$cache_evidence/tmp"
export TMPDIR="$cache_evidence/tmp"
printf 'Evidence: %s\nMemoryMax: %s\n' "$cache_evidence" "$cache_memory"
sha256sum scripts/check-runtime-cache-lock-order.sh \
    formal/tlaplus/finalized_floor/RuntimeCacheLockOrder* \
    casper/src/rust/util/rholang/runtime_manager.rs \
    casper/src/rust/util/rholang/runtime_cache_lock_tests.rs \
    > "$cache_evidence/inputs.sha256"
cache_finish() {
    local cache_result=$?
    trap - EXIT
    if ! sha256sum --check "$cache_evidence/inputs.sha256" > "$cache_evidence/inputs-check.log"; then
        cat "$cache_evidence/inputs-check.log"
        cache_result=1
    fi
    printf 'Gate exit: %s\n' "$cache_result"
    exit "$cache_result"
}
trap cache_finish EXIT
for cache_variant in '' BlockIndex HoldReadUnsafe ShadowCloneUnsafe; do
    cache_name="RuntimeCacheLockOrder$cache_variant"
    mkdir -p "$cache_evidence/$cache_name"
    set +e
    java -Xmx512m -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
        -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
        -metadir "$cache_evidence/$cache_name" \
        -config "formal/tlaplus/finalized_floor/$cache_name.cfg" \
        formal/tlaplus/finalized_floor/RuntimeCacheLockOrder.tla \
        > "$cache_evidence/$cache_name.log" 2>&1
    cache_result=$?
    set -e
    tail -n 12 "$cache_evidence/$cache_name.log"
    case "$cache_variant" in
        ''|BlockIndex)
            [[ "$cache_result" == 0 ]]
            rg -q 'Model checking completed. No error has been found.' "$cache_evidence/$cache_name.log"
            ;;
        *)
            [[ "$cache_result" == 12 ]]
            rg -q 'Invariant NoWaitForCycle is violated' "$cache_evidence/$cache_name.log"
            ;;
    esac
done

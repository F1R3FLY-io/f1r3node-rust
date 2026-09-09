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
import_variant=${1:-StateImportOwnership}
import_module=MCStateImportOwnership
case "$import_variant" in
    StateImportOwnership|StateImportOwnershipReplacement|StateImportOwnershipCombined)
        import_expected=0
        import_pattern='Model checking completed. No error has been found.'
        ;;
    StateImportOwnershipLateAuthority)
        import_expected=12
        import_pattern='Invariant TagAuthorizationCurrent is violated'
        ;;
    StateImportOwnershipRetireClosedBeforeTag)
        import_expected=12
        import_pattern='Invariant RetiredRecoveryHasPublishedEvidence is violated'
        ;;
    StateImportOwnershipRetireFailedAfterTag)
        import_expected=12
        import_pattern='Invariant RetirementHasWitness is violated'
        ;;
    StateImportOwnershipFollowCurrentRoot)
        import_expected=12
        import_pattern='Invariant ReaderUsesCapturedRoot is violated'
        ;;
    StateImportOwnershipUnmatchedWrite)
        import_expected=12
        import_pattern='Invariant WriteCompletionMatched is violated'
        ;;
    StateImportOwnershipInvalidDomain)
        import_module=MCStateImportOwnershipInvalidDomain
        import_expected=10
        import_pattern='Assumption'
        ;;
    *) echo 'Unknown state-import ownership configuration.' >&2; exit 2 ;;
esac
mkdir -p target/verification/state-import
import_evidence=$(mktemp -d "$PWD/target/verification/state-import/ownership.XXXXXX")
mkdir -p "$import_evidence/tmp" "$import_evidence/states"
export TMPDIR="$import_evidence/tmp"
printf 'Evidence: %s\nConfiguration: %s\nMemoryMax: %s\n' \
    "$import_evidence" "$import_variant" "$import_memory"
sha256sum scripts/check-state-import-ownership.sh \
    formal/tlaplus/state_import/StateImportPublication.tla \
    formal/tlaplus/state_import/StateImportOwnership.tla \
    "formal/tlaplus/state_import/$import_module.tla" \
    "formal/tlaplus/state_import/$import_variant.cfg" > "$import_evidence/inputs.sha256"
import_finish() {
    local import_result=$?
    trap - EXIT
    if ! sha256sum -c "$import_evidence/inputs.sha256" > "$import_evidence/inputs-check.log"; then
        cat "$import_evidence/inputs-check.log"
        import_result=1
    fi
    printf '%s\n' "$import_result" > "$import_evidence/gate.exit"
    printf 'Gate exit: %s\n' "$import_result"
    exit "$import_result"
}
trap import_finish EXIT
set +e
java -Xmx768m -XX:+UseSerialGC -Djava.io.tmpdir="$TMPDIR" \
    -cp /usr/share/java/tla2tools.jar tlc2.TLC -workers 1 \
    -metadir "$import_evidence/states" \
    -config "formal/tlaplus/state_import/$import_variant.cfg" \
    "formal/tlaplus/state_import/$import_module.tla" > "$import_evidence/tlc.log" 2>&1
import_result=$?
set -e
tail -n 24 "$import_evidence/tlc.log"
printf 'Native exit: %s\n' "$import_result"
printf '%s\n' "$import_result" > "$import_evidence/native.exit"
[[ "$import_result" == "$import_expected" ]]
rg -q -F "$import_pattern" "$import_evidence/tlc.log"
if [[ "$import_variant" == StateImportOwnershipInvalidDomain ]]; then
    rg -q -F 'MCStateImportOwnershipInvalidDomain is false.' "$import_evidence/tlc.log"
    ! rg -q -F 'Computing initial states...' "$import_evidence/tlc.log"
fi

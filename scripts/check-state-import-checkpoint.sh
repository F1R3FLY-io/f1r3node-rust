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
import_variant=${1:-StateImportCheckpointPublication}
import_expected=12
case "$import_variant" in
    StateImportCheckpointPublication|StateImportCheckpointShared|StateImportCheckpointReplacement|StateImportCheckpointFresh)
        import_expected=0
        import_pattern='Model checking completed. No error has been found.' ;;
    StateImportCheckpointFollowCurrentRoot) import_pattern='Invariant CpCapturedBaseIsLocal is violated' ;;
    StateImportCheckpointOmitConstructedChild) import_pattern='Invariant CpConstructionCut is violated' ;;
    StateImportCheckpointSealBeforeConstruction) import_pattern='Invariant CpCertificateCoversRoot is violated' ;;
    StateImportCheckpointPublishOtherRoot) import_pattern='Invariant CpPublishedRootAuthorized is violated' ;;
    StateImportCheckpointPointerAfterTagError) import_pattern='Invariant CpPointerRequiresTagSuccess is violated' ;;
    StateImportCheckpointDropRunningCheckpoint) import_pattern='Invariant CpOutstandingWorkOwned is violated' ;;
    StateImportCheckpointReportUnknownPointerSuccess) import_pattern='Invariant CpReportedRequiresSuccess is violated' ;;
    StateImportCheckpointReportAfterCancellation) import_pattern='Invariant CpCancelledResultSuppressed is violated' ;;
    StateImportCheckpointTerminalLatePointer) import_pattern='Invariant CpTerminalCannotExecute is violated' ;;
    *) echo 'Unknown checkpoint publication configuration.' >&2; exit 2 ;;
esac
mkdir -p target/verification/state-import
import_evidence=$(mktemp -d "$PWD/target/verification/state-import/checkpoint-publication.XXXXXX")
mkdir -p "$import_evidence/tmp" "$import_evidence/states"
export TMPDIR="$import_evidence/tmp"
printf 'Evidence: %s\nConfiguration: %s\nMemoryMax: %s\n' "$import_evidence" "$import_variant" "$import_memory"
sha256sum scripts/check-state-import-checkpoint.sh \
    formal/tlaplus/state_import/StateImportPublication.tla \
    formal/tlaplus/state_import/StateImportOperations.tla \
    formal/tlaplus/state_import/StateImportCheckpointPublication.tla \
    formal/tlaplus/state_import/MCStateImportCheckpointPublication.tla \
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
    formal/tlaplus/state_import/MCStateImportCheckpointPublication.tla > "$import_evidence/tlc.log" 2>&1
import_result=$?
set -e
tail -n 24 "$import_evidence/tlc.log"
printf '%s\n' "$import_result" > "$import_evidence/native.exit"
[[ "$import_result" == "$import_expected" ]]
rg -q -F "$import_pattern" "$import_evidence/tlc.log"

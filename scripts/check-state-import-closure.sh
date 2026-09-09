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
mkdir -p target/verification/state-import
import_evidence=$(mktemp -d "$PWD/target/verification/state-import/closure-gate.XXXXXX")
mkdir -p "$import_evidence/tmp"
export TMPDIR="$import_evidence/tmp"
printf 'Evidence: %s\nMemoryMax: %s\n' "$import_evidence" "$import_memory"
sha256sum scripts/check-state-import-closure.sh \
    formal/rocq/cost_accounted_rho/theories/StateImportClosure.v \
    formal/rocq/cost_accounted_rho/theories/StateImportStorage.v \
    formal/rocq/cost_accounted_rho/theories/StateImportPage.v \
    formal/rocq/cost_accounted_rho/theories/StateImportCodec.v \
    formal/rocq/cost_accounted_rho/theories/StateImportCursor.v \
    formal/rocq/cost_accounted_rho/theories/StateImportTraversal.v \
    formal/rocq/cost_accounted_rho/theories/StateImportStack.v \
    formal/rocq/cost_accounted_rho/theories/StateImportExport.v \
    formal/rocq/cost_accounted_rho/theories/StateImportExecution.v \
    formal/rocq/cost_accounted_rho/theories/StateImportWire.v \
    formal/rocq/cost_accounted_rho/theories/StateImportCold.v \
    formal/rocq/cost_accounted_rho/theories/StateImportEncodedCold.v \
    formal/rocq/cost_accounted_rho/theories/StateImportOccurrence.v \
    formal/rocq/cost_accounted_rho/theories/StateImportOverlay.v \
    formal/rocq/cost_accounted_rho/theories/StateImportReceivedCold.v \
    formal/rocq/cost_accounted_rho/theories/StateImportObservations.v \
    formal/rocq/cost_accounted_rho/theories/StateImportHistoryObservations.v \
    formal/rocq/cost_accounted_rho/theories/StateImportReadTape.v \
    formal/rocq/cost_accounted_rho/theories/StateImportOwnedPage.v \
    formal/rocq/cost_accounted_rho/theories/StateImportScan.v \
    formal/rocq/cost_accounted_rho/theories/StateImportCheckpoint.v \
    formal/rocq/cost_accounted_rho/theories/StateImportRetry.v \
    > "$import_evidence/inputs.sha256"
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
if rg -n '(^|[[:space:]])(Admitted[.]|Axiom[[:space:]]|Parameter[[:space:]])' \
    formal/rocq/cost_accounted_rho/theories/StateImportClosure.v \
    formal/rocq/cost_accounted_rho/theories/StateImportStorage.v \
    formal/rocq/cost_accounted_rho/theories/StateImportPage.v \
    formal/rocq/cost_accounted_rho/theories/StateImportCodec.v \
    formal/rocq/cost_accounted_rho/theories/StateImportCursor.v \
    formal/rocq/cost_accounted_rho/theories/StateImportTraversal.v \
    formal/rocq/cost_accounted_rho/theories/StateImportStack.v \
    formal/rocq/cost_accounted_rho/theories/StateImportExport.v \
    formal/rocq/cost_accounted_rho/theories/StateImportExecution.v \
    formal/rocq/cost_accounted_rho/theories/StateImportWire.v \
    formal/rocq/cost_accounted_rho/theories/StateImportCold.v \
    formal/rocq/cost_accounted_rho/theories/StateImportEncodedCold.v \
    formal/rocq/cost_accounted_rho/theories/StateImportOccurrence.v \
    formal/rocq/cost_accounted_rho/theories/StateImportOverlay.v \
    formal/rocq/cost_accounted_rho/theories/StateImportReceivedCold.v \
    formal/rocq/cost_accounted_rho/theories/StateImportObservations.v \
    formal/rocq/cost_accounted_rho/theories/StateImportHistoryObservations.v \
    formal/rocq/cost_accounted_rho/theories/StateImportReadTape.v \
    formal/rocq/cost_accounted_rho/theories/StateImportOwnedPage.v \
    formal/rocq/cost_accounted_rho/theories/StateImportScan.v \
    formal/rocq/cost_accounted_rho/theories/StateImportCheckpoint.v \
    formal/rocq/cost_accounted_rho/theories/StateImportRetry.v; then
    exit 1
fi
if ! coqc -Q formal/rocq/cost_accounted_rho/theories CostAccountedRho \
    -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportClosure.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportClosure.v \
    > "$import_evidence/rocq.log" 2>&1; then
    cat "$import_evidence/rocq.log"
    exit 1
fi
if ! coqc -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportStorage.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportStorage.v \
    > "$import_evidence/storage-rocq.log" 2>&1; then
    cat "$import_evidence/storage-rocq.log"
    exit 1
fi
if ! coqc -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportPage.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportPage.v \
    > "$import_evidence/page-rocq.log" 2>&1; then
    cat "$import_evidence/page-rocq.log"
    exit 1
fi
if ! coqc -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportCodec.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportCodec.v \
    > "$import_evidence/codec-rocq.log" 2>&1; then
    cat "$import_evidence/codec-rocq.log"
    exit 1
fi
if ! coqc -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportCursor.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportCursor.v \
    > "$import_evidence/cursor-rocq.log" 2>&1; then
    cat "$import_evidence/cursor-rocq.log"
    exit 1
fi
if ! coqc -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportTraversal.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportTraversal.v \
    > "$import_evidence/traversal-rocq.log" 2>&1; then
    cat "$import_evidence/traversal-rocq.log"
    exit 1
fi
if ! coqc -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportStack.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportStack.v \
    > "$import_evidence/stack-rocq.log" 2>&1; then
    cat "$import_evidence/stack-rocq.log"
    exit 1
fi
if ! coqc -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportExport.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportExport.v \
    > "$import_evidence/export-rocq.log" 2>&1; then
    cat "$import_evidence/export-rocq.log"
    exit 1
fi
if ! coqc -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportExecution.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportExecution.v \
    > "$import_evidence/execution-rocq.log" 2>&1; then
    cat "$import_evidence/execution-rocq.log"
    exit 1
fi
if ! coqc -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportWire.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportWire.v \
    > "$import_evidence/wire-rocq.log" 2>&1; then
    cat "$import_evidence/wire-rocq.log"
    exit 1
fi
if ! coqc -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportCold.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportCold.v \
    > "$import_evidence/cold-rocq.log" 2>&1; then
    cat "$import_evidence/cold-rocq.log"
    exit 1
fi
if ! coqc -time -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportEncodedCold.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportEncodedCold.v \
    > "$import_evidence/encoded-cold-rocq.log" 2>&1; then
    tail -n 60 "$import_evidence/encoded-cold-rocq.log"
    exit 1
fi
if ! coqc -time -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportOccurrence.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportOccurrence.v \
    > "$import_evidence/occurrence-rocq.log" 2>&1; then
    tail -n 60 "$import_evidence/occurrence-rocq.log"
    exit 1
fi
if ! coqc -time -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportOverlay.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportOverlay.v \
    > "$import_evidence/overlay-rocq.log" 2>&1; then
    tail -n 60 "$import_evidence/overlay-rocq.log"
    exit 1
fi
if ! coqc -time -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportReceivedCold.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportReceivedCold.v \
    > "$import_evidence/received-cold-rocq.log" 2>&1; then
    tail -n 60 "$import_evidence/received-cold-rocq.log"
    exit 1
fi
if ! coqc -time -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportObservations.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportObservations.v \
    > "$import_evidence/observations-rocq.log" 2>&1; then
    tail -n 60 "$import_evidence/observations-rocq.log"
    exit 1
fi
if ! coqc -time -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportHistoryObservations.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportHistoryObservations.v \
    > "$import_evidence/history-observations-rocq.log" 2>&1; then
    tail -n 60 "$import_evidence/history-observations-rocq.log"
    exit 1
fi
if ! coqc -time -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportReadTape.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportReadTape.v \
    > "$import_evidence/read-tape-rocq.log" 2>&1; then
    tail -n 60 "$import_evidence/read-tape-rocq.log"
    exit 1
fi
if ! coqc -time -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportOwnedPage.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportOwnedPage.v \
    > "$import_evidence/owned-page-rocq.log" 2>&1; then
    tail -n 60 "$import_evidence/owned-page-rocq.log"
    exit 1
fi
if ! coqc -time -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportScan.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportScan.v \
    > "$import_evidence/scan-rocq.log" 2>&1; then
    tail -n 60 "$import_evidence/scan-rocq.log"
    exit 1
fi
if ! coqc -time -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportCheckpoint.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportCheckpoint.v \
    > "$import_evidence/checkpoint-rocq.log" 2>&1; then
    tail -n 60 "$import_evidence/checkpoint-rocq.log"
    exit 1
fi
if ! coqc -time -Q "$import_evidence" CostAccountedRho \
    -o "$import_evidence/StateImportRetry.vo" \
    formal/rocq/cost_accounted_rho/theories/StateImportRetry.v \
    > "$import_evidence/retry-rocq.log" 2>&1; then
    tail -n 60 "$import_evidence/retry-rocq.log"
    exit 1
fi
if ! coqchk -silent -Q "$import_evidence" CostAccountedRho \
    CostAccountedRho.StateImportClosure CostAccountedRho.StateImportStorage \
    CostAccountedRho.StateImportPage \
    CostAccountedRho.StateImportCodec CostAccountedRho.StateImportCursor \
    CostAccountedRho.StateImportTraversal \
    CostAccountedRho.StateImportStack \
    CostAccountedRho.StateImportExport \
    CostAccountedRho.StateImportExecution \
    CostAccountedRho.StateImportWire \
    CostAccountedRho.StateImportCold \
    CostAccountedRho.StateImportEncodedCold \
    CostAccountedRho.StateImportOccurrence \
    CostAccountedRho.StateImportOverlay \
    CostAccountedRho.StateImportReceivedCold \
    CostAccountedRho.StateImportObservations \
    CostAccountedRho.StateImportHistoryObservations \
    CostAccountedRho.StateImportReadTape \
    CostAccountedRho.StateImportOwnedPage \
    CostAccountedRho.StateImportScan \
    CostAccountedRho.StateImportCheckpoint \
    CostAccountedRho.StateImportRetry \
    > "$import_evidence/kernel.log" 2>&1; then
    cat "$import_evidence/kernel.log"
    exit 1
fi
printf 'State-import lemmas compiled and passed kernel checking.\n'

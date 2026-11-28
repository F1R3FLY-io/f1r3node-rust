#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
OUT="${COLD_COMPRESSION_EVIDENCE:-target/cold-store-validation/formal}"
[[ "$OUT" == target/* && "$OUT" != *..* ]] || exit 2
mkdir -p "$OUT"
OUT="$(realpath "$OUT")"
rm -f "$OUT/report.json"
WORK="$(mktemp -d "$OUT/check.XXXXXX")"
REL="${WORK#"$ROOT/"}"
JAR="${TLA_TOOLS_JAR:?Set TLA_TOOLS_JAR to the pinned TLC 1.7.4 jar}"
printf '%s  %s\n' 936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88 "$JAR" | sha256sum --check --status
JAR="$(realpath "$JAR")"
cp formal/tlaplus/cold_store_compression/*.{tla,cfg} "$WORK/"
cp formal/rocq/cold_store_compression/theories/MainTheorem.v "$WORK/"
names=(canonical_roundtrip corrupt_frame_refused unknown_format_refused successful_compressed_read_verified physical_choice_preserves_state write_preserves_other_keys canonical_export_independent canonical_observer_independent changed_row_preserved different_value_preserved deletion_requires_matching_value stable_matching_row_deleted acquire_preserves_bounds release_preserves_bounds acquire_release_exact arbitrary_trace_preserves_bounds)
for cfg in "$WORK"/*.cfg; do
    model="$(basename "$cfg" .cfg)"
    set +e
    (cd "$WORK" && timeout 120 java -Xmx512m -XX:+UseParallelGC -jar "$JAR" -workers 1 -metadir "$WORK/states-$model" -config "$model.cfg" "$model.tla") >"$OUT/$model.log" 2>&1
    result=$?
    set -e
    if [[ "$model" == MC_ColdStoreCompression ]]; then
        [[ "$result" -eq 0 ]]
        grep -q 'Model checking completed. No error has been found.' "$OUT/$model.log"
    else
        expected="$(awk '/^INVARIANTS / { print $2 }' "$cfg")"
        [[ "$result" -eq 12 ]]
        grep -Fxq "Error: Invariant $expected is violated." "$OUT/$model.log"
        [[ "$(grep -c '^Error: Invariant ' "$OUT/$model.log")" -eq 1 ]]
        ! grep '^Error:' "$OUT/$model.log" | grep -Fvx -e "Error: Invariant $expected is violated." -e 'Error: The behavior up to this point is:'
        grep -q 'State 1:' "$OUT/$model.log"
    fi
    printf '%s exit=%s\n' "$model" "$result"
done
rocq() {
    if [[ -n "${COLD_COMPRESSION_ROCQ_CONTAINER:-}" ]]; then
        docker exec --user "$(id -u):$(id -g)" -w /work "$COLD_COMPRESSION_ROCQ_CONTAINER" "$@"
    else
        "$@"
    fi
}
! grep -nE '\b(Axiom|Axioms|Admitted|Parameter|Parameters|Conjecture|Conjectures)\b' "$WORK/MainTheorem.v"
rocq coqc -Q "$REL" ColdStoreCompression "$REL/MainTheorem.v" >"$OUT/coqc.log" 2>&1
rocq coqchk -Q "$REL" ColdStoreCompression ColdStoreCompression.MainTheorem >"$OUT/coqchk.log" 2>&1
grep -q 'Modules were successfully checked' "$OUT/coqchk.log"
{
    printf 'From ColdStoreCompression Require Import MainTheorem.\n'
    for theorem in "${names[@]}"; do printf 'Print Assumptions %s.\n' "$theorem"; done
} >"$WORK/Assumptions.v"
rocq coqc -Q "$REL" ColdStoreCompression "$REL/Assumptions.v" >"$OUT/assumptions.log" 2>&1
[[ "$(grep -c 'Closed under the global context' "$OUT/assumptions.log")" -eq "${#names[@]}" ]]
rocq coqc --version >"$OUT/coq-version.txt"
sha256sum formal/tlaplus/cold_store_compression/*.{tla,cfg} formal/rocq/cold_store_compression/theories/MainTheorem.v rspace++/src/rspace/history/cold_store_codec.rs rspace++/src/rspace/history/cold_store_codec/tests.rs rspace++/src/rspace/history/history_repository.rs rspace++/src/rspace/rspace.rs rspace++/tests/cold_store_compression.rs Cargo.lock scripts/ci/check-cold-store-compression.sh >"$OUT/sources.sha256"
jq -n --arg source_sha256 "$(sha256sum "$OUT/sources.sha256" | cut -d ' ' -f 1)" --arg timestamp "$(date -u +%FT%TZ)" --argjson theorems "${#names[@]}" '{schema:"cold-store-formal-evidence/v1", timestamp:$timestamp, source_manifest_sha256:$source_sha256, tlc:{positive:1,negative_controls:5},construction:{theorems:$theorems,axioms:0,kernel_checked:true},rust_binding:"run separately",maintainer_acceptance:"pending"}' >"$OUT/report.json"
printf 'Kernel-checked %s closed theorems. Evidence: %s\n' "${#names[@]}" "$OUT"

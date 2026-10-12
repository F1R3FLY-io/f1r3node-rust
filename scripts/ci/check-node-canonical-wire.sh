#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
mode=${1:?Usage: check-node-canonical-wire.sh export|verify OUTPUT}
output=${2:?An output directory is required}
mkdir -p "$output"
output=$(cd "$output" && pwd)
project=formal/rocq/node_observation/b11
manifest() {
    {
        if [[ -n ${B11_SOURCE_LIST:-} ]]; then
            cat "$B11_SOURCE_LIST"
        else
            git ls-files '*.rs' '*Cargo.toml' Cargo.lock rust-toolchain.toml .cargo/config.toml
        fi
        printf '%s\n' block-storage/src/rust/dag/soak_snapshot/canonical_wire_tests.rs
    } | LC_ALL=C sort -u | while IFS= read -r path; do shasum -a 256 "$path"; done
}
case "$mode" in
export)
    test ! -e "$output/cases"
    manifest > "$output/rust-inputs.sha256"
    shasum -a 256 scripts/ci/check-node-canonical-wire.sh > "$output/export-tool.sha256"
    rustc -Vv > "$output/rustc.txt"
    cargo -V > "$output/cargo.txt"
    mkdir "$output/cases"
    B11_ROCQ_CASES_DIR="$output/cases" cargo test --locked --offline -p block-storage --lib canonical_wire_tests -- --test-threads=1 > "$output/rust.log" 2>&1
    grep -q 'test result: ok. 2 passed; 0 failed' "$output/rust.log"
    shasum -a 256 -c "$output/rust-inputs.sha256" > "$output/rust-input-check.log"
    (cd "$output" && shasum -a 256 cases/*.v cases/cases.tsv > cases.sha256)
    ;;
verify)
    manifest > "$output/current-rust-inputs.sha256"
    cmp "$output/rust-inputs.sha256" "$output/current-rust-inputs.sha256"
    shasum -a 256 -c "$output/export-tool.sha256" > "$output/export-tool-check.log"
    shasum -a 256 -c "$output/rust-inputs.sha256" > "$output/verify-input-check.log"
    (cd "$output" && shasum -a 256 -c cases.sha256 > cases-check.log)
    grep -q 'test result: ok. 2 passed; 0 failed' "$output/rust.log"
    test "$(wc -l < "$output/cases/cases.tsv" | tr -d ' ')" = 75
    test "$(cut -f1 "$output/cases/cases.tsv" | sort -u | wc -l | tr -d ' ')" = 75
    test "$(cut -f2 "$output/cases/cases.tsv" | sort -u | wc -l | tr -d ' ')" = 75
    test "$(find "$output/cases" -name 'Case*.v' | wc -l | tr -d ' ')" = 75
    test "$(grep -c '^Example reject_' "$output/cases/Case000.v")" = 6
    mkdir -p "$output/proof"
    cp "$project"/theories/*.v "$output/proof/"
    shasum -a 256 "$project"/_CoqProject "$project"/theories/*.v scripts/ci/check-node-canonical-wire.sh > "$output/proof-inputs.sha256"
    coqc --version > "$output/rocq.txt"
    : > "$output/proof.log"
    for module in Wire Schema MainTheorem; do
        coqc -Q "$output/proof" NodeObservationB11 "$output/proof/$module.v" >> "$output/proof.log" 2>&1
    done
    coqchk -Q "$output/proof" NodeObservationB11 NodeObservationB11.MainTheorem > "$output/kernel.log" 2>&1
    printf 'From NodeObservationB11 Require Import MainTheorem.\n' > "$output/proof/Assumptions.v"
    for theorem in canonical_integer_roundtrip canonical_metadata_roundtrip canonical_snapshot_roundtrip canonical_metadata_injective canonical_snapshot_injective canonical_ordered_parents_preserved canonical_availability_distinct canonical_collection_permutation; do
        printf 'Check %s.\nPrint Assumptions %s.\n' "$theorem" "$theorem" >> "$output/proof/Assumptions.v"
    done
    coqc -Q "$output/proof" NodeObservationB11 "$output/proof/Assumptions.v" > "$output/assumptions.log" 2>&1
    test "$(grep -c '^Closed under the global context' "$output/assumptions.log")" = 8
    : > "$output/correspondence.log"
    while IFS=$'\t' read -r file name digest; do
        [[ "$file" =~ ^Case[0-9][0-9][0-9]\.v$ ]]
        [[ "$digest" =~ ^[0-9a-f]{64}$ ]]
        printf '%s %s\n' "$file" "$name" >> "$output/correspondence.log"
        coqc -Q "$output/proof" NodeObservationB11 "$output/cases/$file" >> "$output/correspondence.log" 2>&1
    done < "$output/cases/cases.tsv"
    shasum -a 256 -c "$output/proof-inputs.sha256" > "$output/proof-input-check.log"
    shasum -a 256 -c "$output/rust-inputs.sha256" > "$output/final-input-check.log"
    (cd "$output" && shasum -a 256 -c cases.sha256 > final-cases-check.log)
    printf 'B11 PASS: 8 closed theorems, 75 production cases, 6 rejection controls.\n' | tee "$output/result.txt"
    ;;
*) printf 'Unknown mode: %s\n' "$mode" >&2; exit 2 ;;
esac

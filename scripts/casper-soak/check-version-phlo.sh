#!/usr/bin/env bash
set -euo pipefail
[[ $# == 1 ]] || { printf 'Usage: %s OUTPUT\n' "$0" >&2; exit 2; }
: "${TLA_TOOLS_JAR:?Set TLA_TOOLS_JAR to the pinned TLC 1.7.4 JAR.}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
OUT="$1"
[[ ! -e "$OUT" && ! -L "$OUT" ]] || { printf 'The output must be new.\n' >&2; exit 2; }
mkdir -p "$(dirname "$OUT")"
mkdir "$OUT"
OUT="$(cd "$OUT" && pwd)"
COMPLETED=0
finish() {
    status=$?
    trap - EXIT HUP INT TERM
    outcome=failed
    if [[ "$COMPLETED" == 1 && "$status" == 0 ]]; then
        outcome=passed
    elif [[ "$status" == 0 ]]; then
        status=1
    fi
    printf '{"scope":"controlled-version-phlo-profile","checks":"%s","exit":%s,"claim_discharge":"pending","node_execution":false,"required_rust_tests":15,"required_cases":102,"required_invocations":103,"required_model_controls":4}\n' "$outcome" "$status" >"$OUT/summary.json.tmp"
    mv "$OUT/summary.json.tmp" "$OUT/summary.json"
    printf '%s\n' "$status" >"$OUT/exit.txt"
    exit "$status"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
JAVA="${SOAK_VERSION_PHLO_JAVA:-java}"
export SOAK_VERSION_PHLO_EVIDENCE="$OUT/fixtures"
FILES=(
    .github/workflows/casper-version-phlo.yml
    scripts/casper-soak/check-version-phlo.sh
    scripts/casper-soak/src/bin/casper-version-phlo.rs
    scripts/casper-soak/src/profiles/version_phlo.rs
    scripts/casper-soak/tests/version_phlo.rs
    scripts/casper-soak/src/lib.rs
    scripts/casper-soak/src/manifest.rs
    scripts/casper-soak/src/models.rs
    scripts/casper-soak/Cargo.toml Cargo.toml Cargo.lock rust-toolchain.toml .cargo/config.toml
    docs/claims/casper-soak-version-phlo.md
    formal/tlaplus/casper_soak/profiles/version_phlo/README.md
    formal/tlaplus/casper_soak/profiles/version_phlo/*.tla
    formal/tlaplus/casper_soak/profiles/version_phlo/*.cfg
    formal/tlaplus/casper_soak/profiles/version_phlo/*.jsonc
)
shasum -a 256 "${FILES[@]}" >"$OUT/source-before.sha256"
[[ "$(shasum -a 256 "$TLA_TOOLS_JAR" | awk '{print $1}')" == 936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88 ]] || { printf 'The TLC JAR digest differs.\n' >&2; exit 2; }
{ rustc --version; cargo --version; "$JAVA" -version; } >"$OUT/tools.txt" 2>&1
cargo test --locked --release -p casper-soak --bin casper-version-phlo --test version_phlo >"$OUT/fixtures.txt" 2>&1
BIN="${CARGO_TARGET_DIR:-target}/release/casper-version-phlo"
"$BIN" identity >"$OUT/identity.json"
grep -F 'test result: ok. 15 passed; 0 failed; 0 ignored;' "$OUT/fixtures.txt" >/dev/null
CASES=(version_phlo_complete version_phlo_generation phlo_refund_mismatch phlo_observed_version_conflation)
for cap in signed-envelope-capture version-rejection minimum-price-configuration phlo-settlement-observations; do
    for status in unknown unsupported; do CASES+=("version_phlo_capability_missing_${cap}_$status"); done
done
for field in casper_protocol_version accounting_authority_version; do CASES+=("phlo_version_labels_$field"); done
for field in phloLimit phloPrice; do
    CASES+=("phlo_signed_field_missing_request_$field" "phlo_mutation_$field")
    for omitted in false true; do CASES+=("phlo_signed_field_missing_capture_${field}_$omitted"); done
done
for field in execution_outcome prepayment charge refund exhausted; do CASES+=("phlo_settlement_mismatch_$field"); done
for malformed in false true; do CASES+=("phlo_independent_failure_$malformed"); done
for case in minimum_equal minimum_above minimum_below exhausted; do CASES+=("phlo_$case"); done
for version in 6 8 9; do CASES+=("phlo_version_rejected_$version"); done
for defect in missing status field value trigger digest order schedule capability; do CASES+=("phlo_mutation_$defect"); done
for field in prepayment charge refund; do CASES+=("phlo_missing_$field"); done
for field in phloLimit phloPrice prepayment charge refund; do
    for value in '0' '"-1"' '"01"' '"1.5"' '"18446744073709551616"'; do
        digest="$(printf '%s\n' "$value" | shasum -a 256 | awk '{print $1}')"
        CASES+=("phlo_malformed_${field}_$digest")
    done
done
for defect in duplicate conflict duplicate_record wrong_run wrong_member wrong_clock late empty extra_stage sequence clock_regression; do CASES+=("phlo_transport_$defect"); done
for defect in live post_merge policy funding; do CASES+=("phlo_blocked_$defect"); done
for field in fixture_digest deploy_signature; do CASES+=("phlo_receipt_binding_$field"); done
for defect in missing_envelope missing_admission conflicting_settlement unknown_payload; do CASES+=("phlo_retained_failure_$defect"); done
for defect in manifest_bytes source binary envelope_bytes envelope_fields expectation raw_digest raw_association; do CASES+=("phlo_binding_$defect"); done
[[ ${#CASES[@]} == 102 ]]
{
    for name in "${CASES[@]}"; do printf '%s/invocation-1.json\n' "$name"; done
    printf 'version_phlo_generation/invocation-2.json\n'
} | LC_ALL=C sort >"$OUT/expected-invocations.txt"
find "$OUT/fixtures" -mindepth 2 -maxdepth 2 -type f -name 'invocation-*.json' | while IFS= read -r path; do printf '%s\n' "${path#"$OUT/fixtures/"}"; done | LC_ALL=C sort >"$OUT/actual-invocations.txt"
cmp "$OUT/expected-invocations.txt" "$OUT/actual-invocations.txt"
while IFS= read -r path; do
    jq -e '.actual_exit == .expected_exit and (.stdout | fromjson | .scenario_verdict) == .expected_verdict and (.stdout | fromjson | .node_launch_count == 0 and .soak_verdict == "non_passing")' "$OUT/fixtures/$path" >/dev/null
done <"$OUT/expected-invocations.txt"
for item in version_phlo_complete:0:passed version_phlo_capability_missing_signed-envelope-capture_unknown:3:blocked version_phlo_generation:0:passed phlo_version_labels_casper_protocol_version:2:invalid_input phlo_signed_field_missing_capture_phloPrice_false:1:incomplete phlo_refund_mismatch:1:product_failure; do
    name="${item%%:*}"; rest="${item#*:}"; code="${rest%%:*}"; verdict="${rest#*:}"
    jq -e --argjson code "$code" --arg verdict "$verdict" '.actual_exit == $code and (.stdout | fromjson | .scenario_verdict) == $verdict' "$OUT/fixtures/$name/invocation-1.json" >/dev/null
done
jq -e --slurpfile identity "$OUT/identity.json" '.profile_binary_sha256 == $identity[0].executable_sha256 and .profile_identity.source_digests == $identity[0].source_digests' "$OUT/fixtures/version_phlo_complete/output-1/report.json" >/dev/null
"$BIN" models --root "$ROOT" --output "$OUT/models" --java "$JAVA" --jar "$TLA_TOOLS_JAR" >"$OUT/models.txt" 2>&1
shasum -a 256 "${FILES[@]}" >"$OUT/source-after.sha256"
cmp "$OUT/source-before.sha256" "$OUT/source-after.sha256"
COMPLETED=1

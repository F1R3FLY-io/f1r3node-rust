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
    printf '{"scope":"controlled-slashing-profile","checks":"%s","exit":%s,"claim_discharge":"pending","node_execution":false,"required_rust_tests":7,"required_cases":66,"required_invocations":72,"required_model_controls":4}\n' "$outcome" "$status" >"$OUT/summary.json.tmp"
    mv "$OUT/summary.json.tmp" "$OUT/summary.json"
    printf '%s\n' "$status" >"$OUT/exit.txt"
    exit "$status"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
JAVA="${SOAK_SLASHING_JAVA:-java}"
export SOAK_SLASHING_EVIDENCE="$OUT/fixtures"
FILES=(
    .github/workflows/casper-slashing.yml
    scripts/casper-soak/check-slashing.sh
    scripts/casper-soak/src/bin/casper-slashing.rs
    scripts/casper-soak/src/profiles/slashing.rs
    scripts/casper-soak/tests/slashing.rs
    scripts/casper-soak/src/lib.rs
    scripts/casper-soak/src/manifest.rs
    scripts/casper-soak/src/models.rs
    scripts/casper-soak/Cargo.toml Cargo.toml Cargo.lock rust-toolchain.toml .cargo/config.toml
    docs/claims/casper-soak-slashing.md
    formal/tlaplus/casper_soak/profiles/slashing/README.md
    formal/tlaplus/casper_soak/profiles/slashing/*.tla
    formal/tlaplus/casper_soak/profiles/slashing/*.cfg
    formal/tlaplus/casper_soak/profiles/slashing/*.jsonc
)
shasum -a 256 "${FILES[@]}" >"$OUT/source-before.sha256"
[[ "$(shasum -a 256 "$TLA_TOOLS_JAR" | awk '{print $1}')" == 936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88 ]] || { printf 'The TLC JAR digest differs.\n' >&2; exit 2; }
{ rustc --version; cargo --version; "$JAVA" -version; } >"$OUT/tools.txt" 2>&1
CARGO_PROFILE_TEST_OPT_LEVEL="${CARGO_PROFILE_TEST_OPT_LEVEL:-1}" cargo test --locked -p casper-soak --bin casper-slashing --test slashing >"$OUT/fixtures.txt" 2>&1
BIN="${CARGO_TARGET_DIR:-target}/debug/casper-slashing"
"$BIN" identity >"$OUT/identity.json"
grep -F 'test result: ok. 7 passed; 0 failed; 0 ignored;' "$OUT/fixtures.txt" >/dev/null
CASES=(
    slashing_complete slashing_generation slashing_delivery_order slashing_epoch_mismatch slashing_authorization_mismatch
    merge_lost_slash missing_evidence forged_deploy stale_epoch same_key_rebond duplicate_evidence restart_during_delivery zero_stake_rejection
    malformed-sibling-keeps-authorization missing-inventory empty-inventory conflicting-evidence-counts-unknown
    duplicate-transport known-failure-plus-foreign boolean-receipt-refusal cross-producer-order-unknown
    restart-wrong-target restart-boolean-exit snapshot-before-delivery missing-restart-capability false-scenario-label
    request-deadline-refusal future-rebond-epoch-refusal invalid-expected-authorization conflicting-snapshots-keep-failure
    slashing_capability_missing live-blocked post-merge-blocked boolean-neglect-refusal pinned-expectation-refusal corrupt-artifact duplicate-json-key symlink-artifact
)
for name in authorization recovery_outcome offender_deduplicated invalid_hash_seed; do CASES+=("missing-$name" "mismatch-$name"); done
for reverse in false true; do for field in epoch time; do CASES+=("conflicting-$field-$reverse"); done; done
for field in node_revision parent_prestate_digest epoch rebond_epoch run_id; do CASES+=("foreign-$field"); done
for field in observed status evidence_digest; do CASES+=("unapplied-$field"); done
for field in protocol_version authorization_rule economic_neglect_slashing evidence_schema; do CASES+=("blocked-$field"); done
for value in true '"01"' '"18446744073709551616"' '-1'; do CASES+=("bad-epoch-$(printf '%s' "$value" | shasum -a 256 | awk '{print $1}')"); done
[[ ${#CASES[@]} == 66 ]]
{
    for name in "${CASES[@]}"; do printf '%s/invocation-1.json\n' "$name"; done
    for name in slashing_generation missing_evidence forged_deploy stale_epoch same_key_rebond restart_during_delivery; do printf '%s/invocation-2.json\n' "$name"; done
} | LC_ALL=C sort >"$OUT/expected-invocations.txt"
find "$OUT/fixtures" -mindepth 2 -maxdepth 2 -type f -name 'invocation-*.json' | while IFS= read -r path; do printf '%s\n' "${path#"$OUT/fixtures/"}"; done | LC_ALL=C sort >"$OUT/actual-invocations.txt"
cmp "$OUT/expected-invocations.txt" "$OUT/actual-invocations.txt"
while IFS= read -r path; do
    jq -e '.actual_exit == .expected_exit and (.stdout | fromjson | .scenario_verdict) == .expected_verdict and (.stdout | fromjson | .node_launch_count == 0 and .soak_verdict == "non_passing")' "$OUT/fixtures/$path" >/dev/null
done <"$OUT/expected-invocations.txt"
for item in slashing_complete:0:passed slashing_capability_missing:3:blocked slashing_delivery_order:1:incomplete slashing_epoch_mismatch:1:incomplete slashing_authorization_mismatch:1:product_failure request-deadline-refusal:2:invalid_input; do
    name="${item%%:*}"; rest="${item#*:}"; code="${rest%%:*}"; verdict="${rest#*:}"
    jq -e --argjson code "$code" --arg verdict "$verdict" '.actual_exit == $code and (.stdout | fromjson | .scenario_verdict) == $verdict' "$OUT/fixtures/$name/invocation-1.json" >/dev/null
done
jq -e --slurpfile identity "$OUT/identity.json" '.profile_binary_sha256 == $identity[0].executable_sha256 and .profile_identity.source_digests == $identity[0].source_digests' "$OUT/fixtures/slashing_complete/output-1/report.json" >/dev/null
"$BIN" models --root "$ROOT" --output "$OUT/models" --java "$JAVA" --jar "$TLA_TOOLS_JAR" >"$OUT/models.txt" 2>&1
shasum -a 256 "${FILES[@]}" >"$OUT/source-after.sha256"
cmp "$OUT/source-before.sha256" "$OUT/source-after.sha256"
COMPLETED=1

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
    printf '{"scope":"controlled-carrier-profile","checks":"%s","exit":%s,"claim_discharge":"pending","node_execution":false,"required_rust_tests":23,"required_cases":88,"required_invocations":91,"required_model_controls":4}\n' "$outcome" "$status" >"$OUT/summary.json.tmp"
    mv "$OUT/summary.json.tmp" "$OUT/summary.json"
    printf '%s\n' "$status" >"$OUT/exit.txt"
    exit "$status"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
JAVA="${SOAK_CARRIER_JAVA:-java}"
export SOAK_CARRIER_EVIDENCE="$OUT/fixtures"
FILES=(
    .github/workflows/casper-carrier-index.yml
    scripts/casper-soak/check-carrier-index.sh
    scripts/casper-soak/src/bin/casper-carrier-index.rs
    scripts/casper-soak/src/profiles/carrier_index.rs
    scripts/casper-soak/tests/carrier_index.rs
    scripts/casper-soak/src/lib.rs
    scripts/casper-soak/src/manifest.rs
    scripts/casper-soak/src/models.rs
    scripts/casper-soak/Cargo.toml Cargo.toml Cargo.lock rust-toolchain.toml .cargo/config.toml
    docs/claims/casper-soak-carrier-index.md
    formal/tlaplus/casper_soak/profiles/carrier_index/README.md
    formal/tlaplus/casper_soak/profiles/carrier_index/*.tla
    formal/tlaplus/casper_soak/profiles/carrier_index/*.cfg
    formal/tlaplus/casper_soak/profiles/carrier_index/*.jsonc
)
shasum -a 256 "${FILES[@]}" >"$OUT/source-before.sha256"
[[ "$(shasum -a 256 "$TLA_TOOLS_JAR" | awk '{print $1}')" == 936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88 ]] || { printf 'The TLC JAR digest differs.\n' >&2; exit 2; }
{ rustc --version; cargo --version; "$JAVA" -version; } >"$OUT/tools.txt" 2>&1
cargo test --locked --release -p casper-soak --bin casper-carrier-index --test carrier_index >"$OUT/fixtures.txt" 2>&1
BIN="${CARGO_TARGET_DIR:-target}/release/casper-carrier-index"
"$BIN" identity >"$OUT/identity.json"
grep -F 'test result: ok. 23 passed; 0 failed; 0 ignored;' "$OUT/fixtures.txt" >/dev/null
CASES=(
    carrier_index_complete carrier_index_generation carrier_path_unobserved carrier_window_mismatch
    carrier_fallback carrier_failure_then_missing carrier_set_order carrier_duplicate
    carrier_conflicting_late_copy carrier_missing_with_zero carrier_no_observations
    carrier_late_snapshot carrier_immutable carrier_conflicting_predecessor_copy
    carrier_comparison_counter_missing
)
for v in unknown unsupported; do CASES+=("carrier_index_capability_missing_$v"); done
for v in probe_count ancestor_body_read_count; do CASES+=("carrier_counter_missing_$v"); done
for v in valid invalid approved fork missing_history watermark_boundary retention_boundary; do CASES+=("carrier_case_$v"); done
for v in candidate_id node_revision node_binary_digest dag_digest deploy_signature availability_digest watermark retention_boundary identity_domain; do CASES+=("carrier_pair_$v"); done
for v in live post_merge typed policy; do CASES+=("carrier_blocked_$v"); done
for v in '0' '"-1"' '"01"' '"18446744073709551616"' 'null'; do
    digest="$(printf '%s' "$v" | shasum -a 256 | awk '{print $1}')"
    CASES+=("carrier_bad_counter_$digest")
done
for v in read_failure availability watermark prune restart; do CASES+=("carrier_fault_$v"); done
for v in missing not_applied trigger readiness predecessor late; do CASES+=("carrier_restart_$v"); done
for v in raw fixture qualification binary source; do CASES+=("carrier_pin_$v"); done
for v in run_id incarnation member_id pair_id; do CASES+=("carrier_foreign_$v"); done
for v in engaged_path probe_count ancestor_body_read_count fallback_reason; do CASES+=("carrier_failure_and_malformed_$v"); done
for v in restart read_failure; do
    for member in both reference; do CASES+=("carrier_required_fault_${v}_$member"); done
done
for field in watermark retention_boundary; do
    for v in 0 9 10 11 29 30 31 18446744073709551615; do CASES+=("carrier_boundary_${field}_$v"); done
done
[[ ${#CASES[@]} == 88 ]]
{
    for name in "${CASES[@]}"; do printf '%s/invocation-1.json\n' "$name"; done
    for name in carrier_index_generation carrier_set_order carrier_immutable; do printf '%s/invocation-2.json\n' "$name"; done
} | LC_ALL=C sort >"$OUT/expected-invocations.txt"
find "$OUT/fixtures" -mindepth 2 -maxdepth 2 -type f -name 'invocation-*.json' | while IFS= read -r path; do printf '%s\n' "${path#"$OUT/fixtures/"}"; done | LC_ALL=C sort >"$OUT/actual-invocations.txt"
cmp "$OUT/expected-invocations.txt" "$OUT/actual-invocations.txt"
while IFS= read -r path; do
    jq -e '.actual_exit == .expected_exit and (.stdout | fromjson | .scenario_verdict) == .expected_verdict and (.stdout | fromjson | .node_launch_count == 0 and .soak_verdict == "non_passing")' "$OUT/fixtures/$path" >/dev/null
done <"$OUT/expected-invocations.txt"
for item in carrier_index_complete:0:passed carrier_index_capability_missing_unknown:3:blocked carrier_index_generation:0:passed carrier_path_unobserved:1:incomplete carrier_window_mismatch:2:invalid_input carrier_counter_missing_ancestor_body_read_count:1:incomplete; do
    name="${item%%:*}"; rest="${item#*:}"; code="${rest%%:*}"; verdict="${rest#*:}"
    jq -e --argjson code "$code" --arg verdict "$verdict" '.actual_exit == $code and (.stdout | fromjson | .scenario_verdict) == $verdict' "$OUT/fixtures/$name/invocation-1.json" >/dev/null
done
jq -e --slurpfile identity "$OUT/identity.json" '.profile_binary_sha256 == $identity[0].executable_sha256 and .profile_identity.source_digests == $identity[0].source_digests' "$OUT/fixtures/carrier_index_complete/output-1/report.json" >/dev/null
"$BIN" models --root "$ROOT" --output "$OUT/models" --java "$JAVA" --jar "$TLA_TOOLS_JAR" >"$OUT/models.txt" 2>&1
shasum -a 256 "${FILES[@]}" >"$OUT/source-after.sha256"
cmp "$OUT/source-before.sha256" "$OUT/source-after.sha256"
COMPLETED=1

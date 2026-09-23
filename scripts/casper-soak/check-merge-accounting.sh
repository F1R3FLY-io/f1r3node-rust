#!/usr/bin/env bash
set -euo pipefail
[[ $# == 1 ]] || { printf 'Usage: %s OUTPUT\n' "$0" >&2; exit 2; }
: "${TLA_TOOLS_JAR:?Set TLA_TOOLS_JAR to a pinned JAR.}"
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
    if [[ "$COMPLETED" != 1 && "$status" == 0 ]]; then status=1; fi
    printf '%s\n' "$status" >"$OUT/exit.txt"
    exit "$status"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
JAVA="${SOAK_ACCOUNTING_JAVA:-java}"
export CARGO_PROFILE_TEST_OPT_LEVEL="${CARGO_PROFILE_TEST_OPT_LEVEL:-1}"
export SOAK_ACCOUNTING_EVIDENCE="$OUT/fixtures"
FILES=(.github/workflows/casper-merge-accounting.yml formal/tlaplus/casper_soak/profiles/merge_accounting/README.md scripts/casper-soak/src/bin/casper-merge-accounting.rs scripts/casper-soak/src/profiles/merge_accounting.rs scripts/casper-soak/tests/merge_accounting.rs scripts/casper-soak/check-merge-accounting.sh scripts/casper-soak/src/lib.rs scripts/casper-soak/src/manifest.rs scripts/casper-soak/src/models.rs scripts/casper-soak/Cargo.toml Cargo.toml Cargo.lock rust-toolchain.toml .cargo/config.toml docs/claims/casper-soak-merge-accounting.md formal/tlaplus/casper_soak/profiles/merge_accounting/*.tla formal/tlaplus/casper_soak/profiles/merge_accounting/*.cfg formal/tlaplus/casper_soak/profiles/merge_accounting/*.jsonc)
shasum -a 256 "${FILES[@]}" >"$OUT/source-before.sha256"
[[ "$(shasum -a 256 "$TLA_TOOLS_JAR" | awk '{print $1}')" == 936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88 ]] || { printf 'The TLC JAR digest differs.\n' >&2; exit 2; }
{ rustc --version; cargo --version; "$JAVA" -version; printf 'CARGO_PROFILE_TEST_OPT_LEVEL=%s\n' "$CARGO_PROFILE_TEST_OPT_LEVEL"; } >"$OUT/tools.txt" 2>&1
cargo test --locked -p casper-soak --bin casper-merge-accounting --test merge_accounting >"$OUT/fixtures.txt" 2>&1
BIN="${CARGO_TARGET_DIR:-target}/debug/casper-merge-accounting"
"$BIN" identity >"$OUT/identity.json"
jq -e --slurpfile identity "$OUT/identity.json" '.profile_binary_sha256 == $identity[0].executable_sha256 and .profile_identity.source_digests == $identity[0].source_digests' "$OUT/fixtures/accounting_complete/output-1/report.json" >/dev/null
for item in accounting_complete:0:passed accounting_capability_missing:3:blocked accounting_generation:0:passed accounting_multiplicity:0:passed accounting_settlement_missing:1:incomplete accounting_epoch_mismatch:2:invalid_input; do
    name="${item%%:*}"
    rest="${item#*:}"
    code="${rest%%:*}"
    verdict="${rest#*:}"
    jq -e --argjson code "$code" --arg verdict "$verdict" '.actual_exit == $code and (.stdout | fromjson | .scenario_verdict == $verdict and .node_launch_count == 0 and .soak_verdict == "non_passing")' "$OUT/fixtures/$name/invocation-1.json" >/dev/null
done
grep -F 'test result: ok. 7 passed; 0 failed; 0 ignored;' "$OUT/fixtures.txt" >/dev/null
"$BIN" models --root "$ROOT" --output "$OUT/models" --java "$JAVA" --jar "$TLA_TOOLS_JAR" >"$OUT/models.txt" 2>&1
shasum -a 256 "${FILES[@]}" >"$OUT/source-after.sha256"
cmp "$OUT/source-before.sha256" "$OUT/source-after.sha256"
printf '{"scope":"profile-fixture-and-model-checks","claim_discharge":"pending","node_execution":false,"rust_tests":7,"model_controls":4}\n' >"$OUT/summary.json"
COMPLETED=1

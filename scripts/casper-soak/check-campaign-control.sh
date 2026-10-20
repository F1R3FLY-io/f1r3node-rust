#!/usr/bin/env bash
set -euo pipefail
umask 077
[[ $# == 1 ]] || exit 2
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
out="$1"
[[ ! -e "$out" && ! -L "$out" ]] || exit 2
mkdir -p "$(dirname "$out")"
mkdir -m 700 "$out"
out="$(cd "$out" && pwd)"
completed=false
finish() {
  local code=$?
  trap - EXIT
  if [[ "$completed" != true && "$code" == 0 ]]; then code=1; fi
  jq -n --argjson code "$code" --argjson completed "$completed" \
    '{schema_version:1,scope:"campaign-control-verification",exit_code:$code,
      status:(if $code==0 and $completed then "passed" else "failed" end),
      claim_discharge:"pending",binding:"pending",soak:"pending",node_launches:0,cloud_launches:0}' > "$out/report.json"
  exit "$code"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
: "${TLA_TOOLS_JAR:?Set TLA_TOOLS_JAR to the pinned TLC 1.7.4 JAR.}"
java="${SOAK_CAMPAIGN_JAVA:-java}"
timeout_command=timeout
command -v timeout >/dev/null || timeout_command=gtimeout
command -v "$timeout_command" >/dev/null
files=(
  scripts/casper-soak/Cargo.toml Cargo.toml Cargo.lock rust-toolchain.toml .cargo/config.toml
  scripts/casper-soak/src/campaign_control/*.rs
  scripts/casper-soak/src/bin/casper-campaign-*.rs
  scripts/casper-soak/src/*.rs
  scripts/casper-soak/tests/campaign_*.rs
  scripts/casper-soak/campaign*.sh scripts/casper-soak/*campaign*.sh
  scripts/casper-soak/supervisor/*
  formal/tlaplus/casper_soak/campaign/*
  docs/claims/casper-campaign-*.md docs/claims/casper-soak-campaign.md
  .github/workflows/casper-campaign-control.yml .github/workflows/merge-recovery-soak.yml
)
shasum -a 256 "${files[@]}" > "$out/source-before.sha256"
{ rustc --version; cargo --version; "$java" -version; "$timeout_command" --version; } > "$out/tools.txt" 2>&1
cargo build --locked -p casper-soak --bin casper-campaign-models > "$out/build.txt" 2>&1
failed=0
bash scripts/casper-soak/test-campaign-control.sh "$out/fixtures" > "$out/fixtures.txt" 2>&1 || failed=1
cargo test --locked -p casper-soak --test campaign_models > "$out/regressions.txt" 2>&1 || failed=1
bash scripts/casper-soak/check-campaign-reservation.sh "$out/reservations" > "$out/reservations.txt" 2>&1 || failed=1
"${CARGO_TARGET_DIR:-target}/debug/casper-campaign-models" --root "$root" --output "$out/models" \
  --java "$java" --jar "$TLA_TOOLS_JAR" --timeout "$timeout_command" > "$out/models.txt" 2>&1 || failed=1
bash scripts/casper-soak/test-campaign.sh "$out/planner" > "$out/planner.txt" 2>&1 || failed=1
shasum -a 256 "${files[@]}" > "$out/source-after.sha256"
cmp "$out/source-before.sha256" "$out/source-after.sha256" || failed=1
[[ "$failed" == 0 ]]
jq -e '.status=="passed" and (.results|length)==6 and all(.results[];.outcome=="passed")' "$out/models/report.json" >/dev/null
completed=true

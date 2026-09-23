#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
GATE="$ROOT/scripts/ci/check-formal-gate.sh"
mkdir -p "$WORK/bin" "$WORK/repo/.github/workflows" "$WORK/repo/scripts/ci"
cp "$ROOT/.github/workflows/slashing-tests.yml" "$WORK/repo/.github/workflows/"
cp "$GATE" "$ROOT/scripts/ci/test-check-formal-gate.sh" "$WORK/repo/scripts/ci/"
printf 'fixture\n' > "$WORK/repo/Cargo.toml"
cp "$WORK/repo/.github/workflows/slashing-tests.yml" "$WORK/workflow.yml"
cat > "$WORK/bin/git" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
    rev-parse) printf '%s\n' "$TEST_HEAD" ;;
    diff) exit "${TEST_DIRTY:-0}" ;;
    show) [[ "${TEST_GIT_FAILURE:-}" != show ]] || exit 2; cat "$TEST_WORKFLOW" ;;
    ls-files)
        case "${2:-}" in
            --others) [[ "${TEST_GIT_FAILURE:-}" != untracked ]] || exit 2; printf '%s' "${TEST_UNTRACKED:-}" ;;
            --error-unmatch) test -f scripts/ci/check-formal-gate.sh && test -f scripts/ci/test-check-formal-gate.sh ;;
            *)
                if [[ "${TEST_GIT_FAILURE:-}" == inventory ]]; then printf '%s\n' Cargo.toml; exit 2; fi
                printf '%s\n' .github/workflows/slashing-tests.yml scripts/ci/check-formal-gate.sh scripts/ci/test-check-formal-gate.sh Cargo.toml ;;
        esac ;;
    *) exit 2 ;;
esac
SH
chmod +x "$WORK/bin/git"
export PATH="$WORK/bin:$PATH"
export GITHUB_SHA=1111111111111111111111111111111111111111
export GITHUB_WORKFLOW_SHA=$GITHUB_SHA TEST_HEAD=$GITHUB_SHA
export GITHUB_REPOSITORY=F1R3FLY-io/f1r3node-rust
export GITHUB_WORKFLOW_REF=F1R3FLY-io/f1r3node-rust/.github/workflows/slashing-tests.yml@refs/pull/1/merge
export GITHUB_RUN_ID=123 GITHUB_RUN_ATTEMPT=1 GITHUB_EVENT_NAME=pull_request
export FORMAL_TLA_RESULT=success FORMAL_ROCQ_RESULT=success TEST_WORKFLOW="$WORK/workflow.yml"
cd "$WORK/repo"
count=0
reject() {
    local code
    set +e
    "$@" > "$WORK/rejected.txt" 2>&1
    code=$?
    set -e
    [[ "$code" == 1 ]] || { printf 'Expected exit 1, observed %s\n' "$code" >&2; cat "$WORK/rejected.txt" >&2; exit 1; }
    count=$((count+1))
}
bash "$GATE" status
for state in failure cancelled skipped neutral timed_out action_required stale ''; do
    reject env FORMAL_TLA_RESULT="$state" bash "$GATE" status
    reject env FORMAL_ROCQ_RESULT="$state" bash "$GATE" status
done
for job in tla-model-check rocq-build; do
    bash "$GATE" record "$job" "receipts/$job"
    bash "$GATE" complete "$job" "receipts/$job"
done
bash "$GATE" verify receipts
reject env GITHUB_RUN_ATTEMPT=2 bash "$GATE" verify receipts
reject env GITHUB_RUN_ID=124 bash "$GATE" verify receipts
reject env GITHUB_EVENT_NAME=push bash "$GATE" verify receipts
reject env GITHUB_WORKFLOW_SHA=2222222222222222222222222222222222222222 bash "$GATE" verify receipts
reject env GITHUB_SHA=2222222222222222222222222222222222222222 bash "$GATE" verify receipts
reject env GITHUB_SHA=bad bash "$GATE" verify receipts
reject env GITHUB_RUN_ID=0 bash "$GATE" verify receipts
reject env GITHUB_REPOSITORY=other/repository bash "$GATE" verify receipts
reject env GITHUB_WORKFLOW_REF=other/workflow bash "$GATE" verify receipts
reject env TEST_DIRTY=1 bash "$GATE" verify receipts
reject env TEST_UNTRACKED=extra-input bash "$GATE" verify receipts
for failure in show untracked inventory; do
    reject env TEST_GIT_FAILURE="$failure" bash "$GATE" verify receipts
done
reject env FORMAL_TLA_RESULT=skipped bash "$GATE" verify receipts
reject bash "$GATE" record unknown-job new-record
cp receipts/tla-model-check/receipt.json "$WORK/valid.json"
for mutation in '.result = "pending"' '.job = "rocq-build"' '.sources = []' '.sources[0].sha256 = "wrong"' '.sources += [.sources[0]]' '.repository = "other/repository"'; do
    jq -S "$mutation" "$WORK/valid.json" > receipts/tla-model-check/receipt.json
    reject bash "$GATE" verify receipts
done
printf '{"result":"failure",' > receipts/tla-model-check/receipt.json
reject bash "$GATE" verify receipts
awk 'NR == 1 {print; print "  \"result\": \"failure\","; next} {print}' "$WORK/valid.json" > receipts/tla-model-check/receipt.json
reject bash "$GATE" verify receipts
dd if=/dev/zero of=receipts/tla-model-check/receipt.json bs=1048577 count=1 2>/dev/null
reject bash "$GATE" verify receipts
cp "$WORK/valid.json" receipts/tla-model-check/receipt.json
printf '\000' >> receipts/tla-model-check/receipt.json
reject bash "$GATE" verify receipts
rm receipts/tla-model-check/receipt.json
reject bash "$GATE" verify receipts
ln -s "$WORK/valid.json" receipts/tla-model-check/receipt.json
reject bash "$GATE" verify receipts
rm receipts/tla-model-check/receipt.json
cp "$WORK/valid.json" receipts/tla-model-check/receipt.json
bash "$GATE" record tla-model-check drift
printf 'changed\n' >> Cargo.toml
reject bash "$GATE" complete tla-model-check drift
reject bash "$GATE" verify receipts
printf 'fixture\n' > Cargo.toml
printf 'wrong workflow\n' > "$WORK/workflow.yml"
reject bash "$GATE" verify receipts
cp .github/workflows/slashing-tests.yml "$WORK/workflow.yml"
bash "$GATE" verify receipts
awk '/^  formal-verification-gate:/{active=1;next} active && /^  [a-zA-Z0-9_-]+:/{exit} active{print}' .github/workflows/slashing-tests.yml > "$WORK/job.yml"
grep -Fxq '    if: always()' "$WORK/job.yml"
grep -Fxq '    needs: [tla-model-check, rocq-build]' "$WORK/job.yml"
grep -Fq 'needs.tla-model-check.result' "$WORK/job.yml"
grep -Fq 'needs.rocq-build.result' "$WORK/job.yml"
grep -Fq 'check-formal-gate.sh status' "$WORK/job.yml"
grep -Fq 'check-formal-gate.sh verify target/formal-receipts' "$WORK/job.yml"
! grep -Eq 'continue-on-error|pull_request_target' .github/workflows/slashing-tests.yml
for kind in tla rocq; do
    [[ $(grep -Fc "name: formal-provenance-$kind-\${{ github.run_id }}-\${{ github.run_attempt }}" .github/workflows/slashing-tests.yml) == 2 ]]
done
printf 'Formal gate: positive round trips, workflow wiring, and %s exact-exit refusal controls passed.\n' "$count"

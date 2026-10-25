#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
base="$TMP/base"
git clone -q --shared --no-checkout "$ROOT" "$base"
manifest=docs/casper/soak-cleanup.json
claim=CLAIM-SOAK-CLEANUP-001
specification=docs/claims/soak-segment-cleanup.md
acceptance=scripts/cbc/lib/cbc-acceptance.py
attributes=scripts/cbc/lib/cbc-attributes.py
while IFS= read -r path; do
    mkdir -p "$base/$(dirname "$path")"
    cp "$ROOT/$path" "$base/$path"
done < <(jq -r '[.artifacts|keys[]]+[.claims[]]+.build_inputs|unique[]' "$ROOT/$manifest")
ssh-keygen -q -t ed25519 -N '' -f "$TMP/fixture-key"
printf 'fixture %s\n' "$(cat "$TMP/fixture-key.pub")" >"$TMP/allowed-signers"
git -C "$base" config --local cbc.acceptanceAllowedSigners "$TMP/allowed-signers"
cd "$base"
: >"$TMP/digests"
while IFS= read -r path; do
    jq -nc --arg path "$path" --arg hash "$(shasum -a 256 "$path" | awk '{print $1}')" '{key:$path,value:$hash}' >>"$TMP/digests"
done < <(jq -r '[.artifacts|keys[]]+[.claims[]]+.build_inputs|unique[]' "$manifest")
jq -s '{schema:"soak-cleanup-verification/v1",source_digests:from_entries}' "$TMP/digests" >report.json
while IFS= read -r path; do
    ledger="$(python3 "$attributes" evidence-path "$path")"
    ids=()
    while IFS= read -r id; do ids+=("$id"); done < <(jq -r --arg path "$path" '.artifacts[$path][]' "$manifest")
    python3 "$acceptance" claim-set "$path" "$ledger" "${ids[@]}" >/dev/null
    jq -n --arg path "$path" '{artifact:{path:$path,commit:"isolated-test-fixture"},status:"discharged",
        adapter:"isolated-test-fixture",claim:"isolated-test-fixture",evidence:{ref:"report.json"},waiver:null}' |
        python3 "$acceptance" record "$path" "$ledger" "$specification" '' "$claim" >/dev/null
    python3 "$acceptance" payload "$path" "$ledger" "$claim" |
        jq '.accepted_by="fixture"|.approved_at="2026-10-25T00:00:00Z"|.approval_ref="isolated-test-fixture"' >"$TMP/approval.json"
    rm -f "$TMP/approval.json.sig"
    ssh-keygen -Y sign -f "$TMP/fixture-key" -n cbc-acceptance "$TMP/approval.json" >/dev/null 2>&1
    python3 "$acceptance" accept "$path" "$ledger" "$TMP/approval.json" "$TMP/approval.json.sig" "$claim" >/dev/null
done < <(jq -r '.artifacts|keys[]' "$manifest")
cd "$ROOT"
checks=0
for name in current stale-control changed-claim changed-build changed-report missing-signature invalid-signature missing-policy missing-record wrong-control; do
    directory="$TMP/$name"
    cp -R "$base" "$directory"
    expected=4
    args=()
    case "$name" in
        current) expected=0 ;;
        stale-control) printf 'old control source\n' >"$directory/scripts/run-merge-recovery-soak.sh" ;;
        changed-claim) printf '\nChanged claim.\n' >>"$directory/$specification" ;;
        changed-build) printf 'changed dependencies\n' >"$directory/Cargo.lock" ;;
        changed-report) printf '{}\n' >"$directory/report.json" ;;
        missing-signature)
            signature="$(find "$directory/docs/casper/cbc-evidence" -name '*.acceptance.json' -print -quit)"
            rm -f "$signature" ;;
        invalid-signature)
            signature="$(find "$directory/docs/casper/cbc-evidence" -name '*.acceptance.json' -print -quit)"
            jq '.approval |= gsub("isolated-test-fixture";"changed-review")' "$signature" >"$TMP/changed.json"
            mv "$TMP/changed.json" "$signature" ;;
        missing-policy) printf '/run-merge-recovery-soak.sh cbc-acceptance=unspecified\n' >>"$directory/scripts/.gitattributes" ;;
        missing-record) rm -f "$directory/docs/casper/cbc-evidence/scripts-run-merge-recovery-soak-sh.md" ;;
        wrong-control) args=(--control-revision bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb) ;;
    esac
    status=0
    bash "$ROOT/scripts/ci/check-soak-cleanup.sh" --root "$directory" \
        --candidate-revision aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa "${args[@]}" >"$TMP/$name.json" 2>"$TMP/$name.err" || status=$?
    [ "$status" -eq "$expected" ] || { cat "$TMP/$name.json" "$TMP/$name.err"; printf 'Unexpected admission verdict: %s\n' "$name" >&2; exit 1; }
    if [ "$name" = current ]; then
        jq -e '.candidate_revision != .control_revision and any(.items[];.artifact_status!="discharged")' "$TMP/$name.json" >/dev/null
    fi
    checks=$((checks + 1))
    [ "$name" != current ] || checks=$((checks + 1))
done
step_block() {
    awk -v title="$2" '$0=="      - name: "title {found=1;print;next} found && /^      - name:/ {exit} found {print} END {if (!found) exit 1}' "$1"
}
segment=.github/actions/soak-segment/action.yml
gate_line="$(awk '/^[[:space:]]*- name: Require accepted segment cleanup$/{print NR}' "$segment")"
driver_line="$(awk '/^[[:space:]]*- name: Soak \(/{print NR}' "$segment")"
[ "$gate_line" -lt "$driver_line" ]
grep -Fq -- '--root ci-control' "$segment"
grep -Fq 'SOAK_HARNESS_BIN: ${{ github.workspace }}/ci-control/target/debug/casper-soak' "$segment"
checks=$((checks + 1))
workflow=.github/workflows/merge-recovery-soak.yml
grep -q '^      - name: Require accepted cleanup before runner launch$' "$workflow"
grep -q '^      - name: Require accepted cleanup on this runner$' "$workflow"
step_block "$workflow" 'Build the harness host controls' | grep -q '^        working-directory: ci-control$'
step_block "$workflow" 'Build the harness host controls' | grep -Fq -- '--target-dir target -p casper-soak --bin casper-soak'
runner_gate="$(awk '/^      - name: Require accepted cleanup on this runner$/{print NR}' "$workflow")"
node_build="$(awk '/^      - name: Build node image$/{print NR}' "$workflow")"
[ "$runner_gate" -lt "$node_build" ]
checks=$((checks + 1))
ci=.github/workflows/ci.yml
grep -q '^      - name: Require current cleanup discharge$' "$ci"
if step_block "$ci" 'Require current cleanup discharge' | grep -Eq '^[[:space:]]*(continue-on-error|if):'; then
    printf 'Cleanup CI admission must be unconditional.\n' >&2
    exit 1
fi
checks=$((checks + 1))
while IFS=$'\t' read -r path hash; do
    [ "$(shasum -a 256 "scripts/cbc/$path" | awk '{print $1}')" = "$hash" ] || exit 1
done < <(jq -r '.sha256|to_entries[]|[.key,.value]|@tsv' scripts/cbc/upstream.json)
checks=$((checks + 1))
printf 'Cleanup admission and workflow checks passed: %s checks.\n' "$checks"

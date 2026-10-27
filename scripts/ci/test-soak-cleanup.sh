#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
bash "$ROOT/scripts/ci/test-cbc-runtime.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
base="$TMP/base"
git clone -q --shared --no-checkout "$ROOT" "$base"
manifest=docs/casper/soak-cleanup.json
claim=CLAIM-SOAK-CLEANUP-001
specification=docs/claims/soak-segment-cleanup.md
cbc=scripts/cbc.sh
runtime="$(bash "$ROOT/scripts/ci/install-cbc-runtime.sh" --check)"
while IFS= read -r path; do
    mkdir -p "$base/$(dirname "$path")"
    cp "$ROOT/$path" "$base/$path"
done < <(jq -r '[.artifacts|keys[]]+[.claims[]]+.build_inputs|unique[]' "$ROOT/$manifest")
for path in .github/.gitattributes docs/.gitattributes formal/.gitattributes scripts/.gitattributes Cargo.lock; do
    mkdir -p "$base/$(dirname "$path")"
    cp "$ROOT/$path" "$base/$path"
done
ssh-keygen -q -t ed25519 -N '' -f "$TMP/fixture-key"
printf 'fixture %s\n' "$(cat "$TMP/fixture-key.pub")" >"$TMP/allowed-signers"
git -C "$base" config --local cbc.acceptanceAllowedSigners "$TMP/allowed-signers"
cd "$base"
: >"$TMP/digests"
while IFS= read -r path; do
    jq -nc --arg path "$path" --arg hash "$(shasum -a 256 "$path" | awk '{print $1}')" '{key:$path,value:$hash}' >>"$TMP/digests"
done < <(jq -r '[.artifacts|keys[]]+[.claims[]]+.build_inputs|unique[]' "$manifest")
closure="$(bash scripts/ci/soak-cleanup-lock-closure.sh --digest)"
jq -s --slurpfile runtime scripts/cbc-runtime.lock.json \
    --arg closure "$closure" \
    '{schema:"soak-cleanup-verification/v1",source_digests:from_entries,
      runtime:($runtime[0]|{repository,revision,source_directory,sha256}),
      lock_closure_sha256:$closure}' "$TMP/digests" >report.json
while IFS= read -r path; do
    ledger="$(bash "$cbc" attributes evidence-path "$path")"
    ids=()
    while IFS= read -r id; do ids+=(--claim-id "$id"); done < <(jq -r --arg path "$path" '.artifacts[$path][]' "$manifest")
    bash "$cbc" claim-set "$path" "${ids[@]}" >/dev/null
    jq -n --arg path "$path" '{artifact:{path:$path,commit:"isolated-test-fixture"},status:"discharged",
        adapter:"isolated-test-fixture",claim:"isolated-test-fixture",evidence:{ref:"report.json"},waiver:null}' |
        bash "$cbc" record "$path" "$ledger" "$specification" '' "$claim" >/dev/null
    bash "$cbc" acceptance-payload "$path" --claim-id "$claim" |
        jq '.accepted_by="fixture"|.approved_at="2026-10-25T00:00:00Z"|.approval_ref="isolated-test-fixture"' >"$TMP/approval.json"
    rm -f "$TMP/approval.json.sig"
    ssh-keygen -Y sign -f "$TMP/fixture-key" -n cbc-acceptance "$TMP/approval.json" >/dev/null 2>&1
    bash "$cbc" accept "$path" --claim-id "$claim" --approval "$TMP/approval.json" --signature "$TMP/approval.json.sig" >/dev/null
done < <(jq -r '.artifacts|keys[]' "$manifest")
cd "$ROOT"
# bump_lock_version LOCK NAME: changes the version of every NAME entry in LOCK.
bump_lock_version() {
    awk -v entry="name = \"$2\"" 'previous == entry && /^version = "/ { sub(/"/, "\"999.") } { print; previous = $0 }' \
        "$1" >"$1.new"
    mv "$1.new" "$1"
}
checks=0
for name in current unrelated-lock changed-closure stale-control changed-claim changed-build changed-report missing-signature invalid-signature missing-policy missing-record wrong-control missing-runtime corrupt-runtime changed-runtime-pin; do
    directory="$TMP/$name"
    cp -R "$base" "$directory"
    expected=4
    args=()
    runtime_env=()
    case "$name" in
        current) expected=0 ;;
        stale-control) printf 'old control source\n' >"$directory/scripts/run-merge-recovery-soak.sh" ;;
        changed-claim) printf '\nChanged claim.\n' >>"$directory/$specification" ;;
        unrelated-lock)
            expected=0
            outside="$(comm -23 <(awk -F'"' '/^name = /{print $2}' "$directory/Cargo.lock" | sort -u) \
                <(bash "$directory/scripts/ci/soak-cleanup-lock-closure.sh" --root "$directory" | awk '{print $1}' | sort -u) | head -1)"
            [ -n "$outside" ]
            bump_lock_version "$directory/Cargo.lock" "$outside" ;;
        changed-closure) bump_lock_version "$directory/Cargo.lock" regex ;;
        changed-build) printf '\nchanged toolchain\n' >>"$directory/rust-toolchain.toml" ;;
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
        missing-runtime) runtime_env=(CBC_RUNTIME_CACHE="$TMP/not-installed") ;;
        corrupt-runtime)
            package="$(dirname "$(dirname "$runtime")")"
            mkdir "$TMP/corrupt-cache"
            cp -R "$package" "$TMP/corrupt-cache/$(basename "$package")"
            bad="$TMP/corrupt-cache/$(basename "$package")/AItools/scripts/cbc.sh"
            chmod u+w "$bad"
            printf '\nchanged runtime\n' >>"$bad"
            runtime_env=(CBC_RUNTIME_CACHE="$TMP/corrupt-cache") ;;
        changed-runtime-pin)
            jq '.sha256["cbc.sh"]="0000000000000000000000000000000000000000000000000000000000000000"' \
                "$directory/scripts/cbc-runtime.lock.json" >"$TMP/pin.json"
            mv "$TMP/pin.json" "$directory/scripts/cbc-runtime.lock.json" ;;
    esac
    status=0
    env "${runtime_env[@]}" bash "$ROOT/scripts/ci/check-soak-cleanup.sh" --root "$directory" \
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
runner_install="$(awk '/^      - name: Install the pinned CbC runtime on this runner$/{print NR}' "$workflow")"
launch_install="$(awk '/^      - name: Install the pinned CbC runtime before runner launch$/{print NR}' "$workflow")"
launch_gate="$(awk '/^      - name: Require accepted cleanup before runner launch$/{print NR}' "$workflow")"
node_build="$(awk '/^      - name: Build node image$/{print NR}' "$workflow")"
reaper_exempt="$(awk '/^      - name: Exempt this runner from the CI reaper$/{print NR}' "$workflow")"
[ -n "$reaper_exempt" ] && [ "$reaper_exempt" -lt "$runner_install" ]
[ "$launch_install" -lt "$launch_gate" ]
[ "$runner_install" -lt "$runner_gate" ]
[ "$runner_gate" -lt "$node_build" ]
checks=$((checks + 1))
ci=.github/workflows/ci.yml
grep -q '^      - name: Require current cleanup discharge$' "$ci"
ci_install="$(awk '/^      - name: Install the pinned CbC runtime$/{print NR}' "$ci")"
ci_verification="$(awk '/^      - name: Verify soak driver and cleanup controls$/{print NR}' "$ci")"
[ "$ci_install" -lt "$ci_verification" ]
if step_block "$ci" 'Require current cleanup discharge' | grep -Eq '^[[:space:]]*(continue-on-error|if):'; then
    printf 'Cleanup CI admission must be unconditional.\n' >&2
    exit 1
fi
checks=$((checks + 1))
[ ! -e "$ROOT/scripts/cbc" ]
checks=$((checks + 1))
printf 'Cleanup admission and workflow checks passed: %s checks.\n' "$checks"

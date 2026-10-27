#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
cd "$ROOT"
output=""
record=0
image=""
harness="${SOAK_HARNESS_BIN:-$ROOT/target/debug/casper-soak}"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output) output="$2"; shift 2 ;;
        --record) record=1; shift ;;
        --linux-image) image="$2"; shift 2 ;;
        --harness-binary) harness="$2"; shift 2 ;;
        *) exit 2 ;;
    esac
done
[ -n "$output" ] || exit 2
mkdir -p "$output"
output="$(cd "$output" && pwd -P)"
[ ! -e "$output/report.json" ] || { printf 'Use a new verification output directory.\n' >&2; exit 4; }
if [ "$record" -eq 1 ]; then [[ "$output" == "$ROOT/"* ]] || exit 4; fi
manifest=docs/casper/soak-cleanup.json
claim=CLAIM-SOAK-CLEANUP-001
specification=docs/claims/soak-segment-cleanup.md
cbc=scripts/cbc.sh
bash scripts/ci/install-cbc-runtime.sh --check >/dev/null
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
hash() { shasum -a 256 "$1" | awk '{print $1}'; }
snapshot() {
    while IFS= read -r path; do
        jq -nc --arg path "$path" --arg hash "$(hash "$path")" '{key:$path,value:$hash}'
    done < <(jq -r '[.artifacts|keys[]]+[.claims[]]+.build_inputs|unique[]' "$manifest")
}
snapshot | jq -s from_entries >"$TMP/before.json"
closure="$(bash scripts/ci/soak-cleanup-lock-closure.sh --digest)"
: >"$TMP/checks"
run_check() {
    local index="$1" linux="$2" logical="$3" status=0
    shift 3
    if [ "$linux" -eq 1 ] && [ -n "$image" ]; then
        docker run --rm --network none --cap-drop ALL --security-opt no-new-privileges \
            --user "$(id -u):$(id -g)" --tmpfs /tmp:rw,exec,size=12g,mode=1777 \
            -v "$ROOT:$ROOT:ro" -v "$output:$output" -v "$harness:/harness/casper-soak:ro" \
            -e SOAK_HARNESS_BIN=/harness/casper-soak -w "$ROOT" "$image" \
            timeout --signal=TERM --kill-after=5 550 "$@" >"$TMP/check-$index.log" 2>&1 || status=$?
    else
        SOAK_HARNESS_BIN="$harness" "$@" >"$TMP/check-$index.log" 2>&1 || status=$?
    fi
    sed "s|$ROOT|[WORKTREE]|g" "$TMP/check-$index.log" >"$output/check-$index.log"
    [ "$status" -eq 0 ] || { cat "$output/check-$index.log"; printf 'Cleanup verification failed.\n' >&2; exit 4; }
    jq -nc --argjson command "$logical" --arg hash "$(hash "$output/check-$index.log")" \
        '{command:$command,exit:0,log_sha256:$hash}' >>"$TMP/checks"
}
run_check 0 0 '["bash","scripts/ci/test-soak-cleanup.sh"]' bash scripts/ci/test-soak-cleanup.sh
run_check 1 0 '["bash","scripts/ci/test-soak-cleanup-model.sh","model.json"]' \
    bash scripts/ci/test-soak-cleanup-model.sh "$output/model.json"
run_check 2 1 '["bash","scripts/ci/test-soak-cleanup-regressions.sh","regressions.json"]' \
    bash scripts/ci/test-soak-cleanup-regressions.sh "$output/regressions.json"
run_check 3 1 '["bash","scripts/bench/test-run-merge-recovery-soak.sh"]' \
    bash scripts/bench/test-run-merge-recovery-soak.sh
snapshot | jq -s from_entries >"$TMP/after.json"
cmp -s "$TMP/before.json" "$TMP/after.json" || { printf 'A verification input changed.\n' >&2; exit 4; }
[ "$(bash scripts/ci/soak-cleanup-lock-closure.sh --digest)" = "$closure" ] ||
    { printf 'The harness dependency closure changed.\n' >&2; exit 4; }
jq -n --slurpfile sources "$TMP/before.json" --slurpfile checks "$TMP/checks" \
    --slurpfile model "$output/model.json" --slurpfile regressions "$output/regressions.json" \
    --slurpfile runtime scripts/cbc-runtime.lock.json \
    --arg at "$(date -u +%FT%TZ)" --arg revision "$(git rev-parse HEAD)" \
    --arg jar "$(hash "${TLA2TOOLS_JAR:?TLA2TOOLS_JAR is required}")" --arg harness "$(hash "$harness")" \
    --arg closure "$closure" \
    '{schema:"soak-cleanup-verification/v1",verified_at:$at,control_revision:$revision,
      source_digests:$sources[0],runtime:($runtime[0]|{repository,revision,source_directory,sha256}),
      checks:$checks,model:$model[0],regressions:$regressions[0],
      tlc_sha256:$jar,harness_binary_sha256:$harness,lock_closure_sha256:$closure,scope:"bounded-segment-cleanup",
      node_execution:false,maintainer_acceptance:"pending",live_soak:"pending"}' >"$output/report.json"
if [ "$record" -eq 1 ]; then
    reference="${output#"$ROOT/"}/report.json"
    mkdir "$output/unsigned-acceptance"
    while IFS= read -r path; do
        ledger="$(bash "$cbc" attributes evidence-path "$path")"
        if [ -f "$ledger" ] && [ ! -e "${ledger%.md}.claims.json" ]; then
            mkdir -p "$(dirname "$ledger")/history"
            cp -n "$ledger" "$(dirname "$ledger")/history/$(hash "$ledger")-$(basename "$ledger")"
            old="$(awk '/^```json$/{block=1;next} /^```/{block=0} block' "$ledger" | jq -sc 'if length==1 then .[0] else error("ambiguous legacy ledger") end')"
            jq -e --arg path "$path" --argjson old "$old" \
                '($old.claim_ids- .artifacts[$path])|length==0' "$manifest" >/dev/null
            printf '# CbC Evidence: %s\n' "$path" >"$ledger"
            while IFS= read -r prior_id; do
                {
                printf '\n```json\n'
                jq --arg id "$prior_id" --arg hash "$(jq -r --arg path "$path" '.[$path]' "$TMP/before.json")" \
                    '.schema="cbc-claim/v1"|.claim_id=$id|if .artifact.sha256!=$hash then .status="pending" else . end' <<<"$old"
                printf '```\n'
                } >>"$ledger"
            done < <(jq -r '.claim_ids[]' <<<"$old")
        fi
        ids=()
        while IFS= read -r id; do ids+=(--claim-id "$id"); done < <(jq -r --arg path "$path" '.artifacts[$path][]' "$manifest")
        bash "$cbc" claim-set "$path" "${ids[@]}" >/dev/null
        inputs="$(jq -nc --arg path "$path" --arg claim "$specification" --slurpfile hashes "$TMP/before.json" \
            '{artifact:$path,source_sha256:$hashes[0][$path],claim:{path:$claim,sha256:$hashes[0][$claim]}}')"
        jq -n --arg path "$path" --arg revision "$(git rev-parse HEAD)" --arg ref "$reference" \
            --arg at "$(date -u +%FT%TZ)" --rawfile claim "$specification" \
            '{artifact:{path:$path,commit:$revision,commit_is_base:true,working_tree:true},
              claim:$claim,adapter:"bounded-model-and-linux-fixture",status:"discharged",scope:"bounded-segment-cleanup",
              evidence:{kind:"bounded-model-and-linux-fixture",ref:$ref},waiver:null,verified_at:$at}' |
            bash "$cbc" record "$path" "$ledger" "$specification" "$inputs" "$claim" >/dev/null
        bash "$cbc" acceptance-payload "$path" --claim-id "$claim" >"$output/unsigned-acceptance/$(basename "${ledger%.md}").json"
    done < <(jq -r '.artifacts|keys[]' "$manifest")
    status=0
    bash scripts/ci/check-soak-cleanup.sh --root "$ROOT" --output "$output/status.json" >/dev/null || status=$?
    [ "$status" -eq 0 ] || [ "$status" -eq 4 ] || exit "$status"
fi
printf 'Cleanup verification passed. Maintainer acceptance remains separate.\n'

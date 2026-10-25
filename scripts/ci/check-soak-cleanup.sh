#!/usr/bin/env bash
set -euo pipefail
root=.
control=""
candidate=""
output=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --root) root="$2"; shift 2 ;;
        --control-revision) control="$2"; shift 2 ;;
        --candidate-revision) candidate="$2"; shift 2 ;;
        --output) output="$2"; shift 2 ;;
        *) printf 'Unknown cleanup gate option: %s\n' "$1" >&2; exit 4 ;;
    esac
done
emit() {
    if [ -n "$output" ]; then
        mkdir -p "$(dirname "$output")"
        printf '%s\n' "$1" >"$output"
    fi
    printf '%s\n' "$1"
}
reject() {
    emit "$(jq -n --arg error "$1" '{schema:"soak-cleanup-admission/v1",status:"rejected",error:$error}')"
    exit 4
}
trap 'reject "The cleanup gate could not validate an input."' ERR
cd "$root"
root="$(pwd -P)"
[ "$(git rev-parse --show-toplevel)" = "$root" ] || reject 'The control root must be the Git worktree root.'
revision="$(git rev-parse HEAD)"
[ -z "$control" ] || [ "$control" = "$revision" ] || reject 'The control checkout has a different revision.'
manifest=docs/casper/soak-cleanup.json
claim=CLAIM-SOAK-CLEANUP-001
cbc=scripts/cbc.sh
hash() {
    shasum -a 256 "$1" | awk '{print $1}'
}
local_file() {
    [[ "$1" =~ ^[A-Za-z0-9_./-]+$ ]] || reject 'A cleanup path is invalid.'
    case "/$1/" in
        */../*|*/.git/*|//*) reject 'A cleanup path leaves the worktree.' ;;
    esac
    local resolved
    resolved="$(realpath "$1")"
    [[ "$resolved" == "$root/"* ]] || reject 'A cleanup path leaves the worktree.'
    [ -f "$1" ] || reject "A cleanup input is missing: $1"
}
local_file "$manifest"
jq -e '.schema=="soak-cleanup/v1" and (.artifacts|type=="object") and (.claims|type=="object") and (.build_inputs|type=="array")' "$manifest" >/dev/null
for required in "$manifest" scripts/ci/check-soak-cleanup.sh scripts/run-merge-recovery-soak.sh \
    scripts/cbc.sh scripts/cbc-runtime.lock.json scripts/ci/install-cbc-runtime.sh \
    scripts/casper-soak/src/host_control.rs .github/actions/soak-segment/action.yml \
    .github/workflows/merge-recovery-soak.yml .github/workflows/ci.yml docs/claims/soak-segment-cleanup.md; do
    jq -e --arg path "$required" '.artifacts|has($path)' "$manifest" >/dev/null || reject 'The cleanup manifest omits a required artifact.'
done
inputs="$(jq -c '[.artifacts|keys[]]+[.claims[]]+.build_inputs|unique' "$manifest")"
rows="$(mktemp)"
trap 'rm -f "$rows"' EXIT
checked="|"
while IFS= read -r path; do
    local_file "$path"
    policy="$(git check-attr cbc cbc-acceptance -- "$path")"
    printf '%s\n' "$policy" | grep -q ': cbc: mandatory$' || reject "Cleanup verification is not mandatory: $path"
    printf '%s\n' "$policy" | grep -q ': cbc-acceptance: required$' || reject "Cleanup acceptance is not required: $path"
    ledger="$(bash "$cbc" attributes evidence-path "$path")"
    local_file "$ledger"
    status="$(bash "$cbc" status "$path" --json)"
    expected="$(jq -c --arg path "$path" '.artifacts[$path]|sort' "$manifest")"
    actual="$(jq -c '.claim_ids|sort' "${ledger%.md}.claims.json")"
    [ "$expected" = "$actual" ] || reject "The declared cleanup claims differ: $path"
    record="$(awk '/^```json$/{block=1;next} /^```/{block=0} block' "$ledger" |
        jq -sc --arg claim "$claim" 'map(select(.claim_id==$claim))|if length==1 then .[0] else error("missing cleanup claim") end')"
    specification="$(jq -r --arg claim "$claim" '.claims[$claim]' "$manifest")"
    [ "$(jq -r '.inputs.claim.path' <<<"$record")" = "$specification" ] || reject "The cleanup specification differs: $path"
    reference="$(jq -r '.evidence.ref' <<<"$record")"
    if [[ "$checked" != *"|$reference|"* ]]; then
        local_file "$reference"
        [ "$(jq -r '.schema' "$reference")" = soak-cleanup-verification/v1 ] || reject 'The cleanup report schema differs.'
        [ "$(jq -cS '.runtime' "$reference")" = "$(jq -cS '{repository,revision,source_directory,sha256}' scripts/cbc-runtime.lock.json)" ] || reject 'The cleanup runtime binding differs.'
        [ "$(jq -c '.source_digests|keys|sort' "$reference")" = "$inputs" ] || reject 'The cleanup report does not bind the complete source set.'
        while IFS= read -r source; do
            local_file "$source"
            expected_hash="$(jq -r --arg source "$source" '.source_digests[$source]' "$reference")"
            [ "$(hash "$source")" = "$expected_hash" ] || reject "A cleanup source or build input changed: $source"
        done < <(jq -r '.source_digests|keys[]' "$reference")
        checked="$checked$reference|"
    fi
    jq -c --arg claim "$claim" '. as $full | (.claims[]|select(.claim_id==$claim)) as $selected |
        {path:$full.path,status:$selected.gate,artifact_status:$full.gate,claims:$full.claims}' <<<"$status" >>"$rows"
done < <(jq -r '.artifacts|keys[]' "$manifest")
result="$(jq -s --arg control "$revision" --arg candidate "$candidate" --arg claim "$claim" '
    map(select(.status!="discharged"))|length' "$rows")"
report="$(jq -s --arg control "$revision" --arg candidate "$candidate" --arg claim "$claim" --argjson gaps "$result" '
    {schema:"soak-cleanup-admission/v1",control_revision:$control,candidate_revision:$candidate,
     claim_id:$claim,status:(if $gaps==0 then "accepted" else "rejected" end),gaps:$gaps,items:.}' "$rows")"
emit "$report"
[ "$result" -eq 0 ] || exit 4

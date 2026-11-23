#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
TMP="$(cd "$TMP" && pwd -P)"
manifest=docs/casper/soak-cleanup.json
claim=CLAIM-SOAK-CLEANUP-001
specification=docs/claims/soak-segment-cleanup.md
cbc=scripts/cbc.sh
kit_script="$ROOT/scripts/ci/soak-cleanup-kit.sh"
import_script="$ROOT/scripts/ci/soak-cleanup-import.sh"
bash "$ROOT/scripts/ci/install-cbc-runtime.sh" --check >/dev/null
base="$TMP/base"
git clone -q --shared --no-checkout "$ROOT" "$base"
while IFS= read -r path; do
    mkdir -p "$base/$(dirname "$path")"
    cp "$ROOT/$path" "$base/$path"
done < <(jq -r '[.artifacts|keys[]]+[.claims[]]+.build_inputs|unique[]' "$ROOT/$manifest")
for path in .github/.gitattributes docs/.gitattributes formal/.gitattributes scripts/.gitattributes Cargo.lock Cargo.toml; do
    mkdir -p "$base/$(dirname "$path")"
    cp "$ROOT/$path" "$base/$path"
done
ssh-keygen -q -t ed25519 -N '' -f "$TMP/fixture-key"
ssh-keygen -q -t ed25519 -N '' -f "$TMP/other-key"
printf 'fixture %s\nreviewer %s\n' "$(cat "$TMP/fixture-key.pub")" "$(cat "$TMP/other-key.pub")" >"$TMP/allowed-signers"
git -C "$base" config --local cbc.acceptanceAllowedSigners "$TMP/allowed-signers"
cd "$base"
: >"$TMP/digests"
while IFS= read -r path; do
    jq -nc --arg path "$path" --arg hash "$(shasum -a 256 "$path" | awk '{print $1}')" '{key:$path,value:$hash}' >>"$TMP/digests"
done < <(jq -r '[.artifacts|keys[]]+[.claims[]]+.build_inputs|unique[]' "$manifest")
jq -s --slurpfile runtime scripts/cbc-runtime.lock.json \
    --arg closure "$(bash scripts/ci/soak-cleanup-lock-closure.sh --digest)" \
    '{schema:"soak-cleanup-verification/v1",source_digests:from_entries,
      runtime:($runtime[0]|{repository,revision,source_directory,sha256}),
      lock_closure_sha256:$closure}' "$TMP/digests" >report.json
mkdir "$TMP/payloads"
while IFS= read -r path; do
    ledger="$(bash "$cbc" attributes evidence-path "$path")"
    ids=()
    while IFS= read -r id; do ids+=(--claim-id "$id"); done < <(jq -r --arg path "$path" '.artifacts[$path][]' "$manifest")
    bash "$cbc" claim-set "$path" "${ids[@]}" >/dev/null
    jq -n --arg path "$path" '{artifact:{path:$path,commit:"isolated-test-fixture"},status:"discharged",
        adapter:"isolated-test-fixture",claim:"isolated-test-fixture",evidence:{ref:"report.json"},waiver:null}' |
        bash "$cbc" record "$path" "$ledger" "$specification" '' "$claim" >/dev/null
    bash "$cbc" acceptance-payload "$path" --claim-id "$claim" >"$TMP/payloads/$(basename "${ledger%.md}").json"
done < <(jq -r '.artifacts|keys[]' "$manifest")
cd "$ROOT"
sign() {
    rm -f "$2.sig"
    ssh-keygen -Y sign -f "$1" -n cbc-acceptance "$2" >/dev/null 2>&1
}
kit() {
    bash "$kit_script" --root "$base" --payloads "$1" --pull-request 1 --signer fixture --kit "$2"
}
expect() {
    local wanted="$1" label="$2" message="$3" status=0
    shift 3
    "$@" >"$TMP/$label.out" 2>&1 || status=$?
    [ "$status" -eq "$wanted" ] && grep -Fq -- "$message" "$TMP/$label.out" ||
        { cat "$TMP/$label.out"; printf 'Unexpected result: %s (%s)\n' "$label" "$status" >&2; exit 1; }
    checks=$((checks + 1))
}
checks=0
first="$(cd "$TMP/payloads" && ls ./*.json | head -1)"
first="${first#./}"
expect 2 payloads-inside-worktree 'outside the worktree' kit "$base/docs" "$TMP/kit-a"
expect 2 kit-inside-worktree 'outside the worktree' kit "$TMP/payloads" "$base/kit"
cp -R "$TMP/payloads" "$TMP/filled"
jq '.accepted_by="someone"' "$TMP/payloads/$first" >"$TMP/filled/$first"
expect 2 filled-payload 'Not an unsigned approval' kit "$TMP/filled" "$TMP/kit-b"
cp -R "$TMP/payloads" "$TMP/short"
rm "$TMP/short/$first"
expect 2 missing-payload 'the manifest covers' kit "$TMP/short" "$TMP/kit-c"
expect 0 kit 'approvals for fixture' kit "$TMP/payloads" "$TMP/kit"
[ "$(find "$TMP/kit" -name '*.json' | wc -l | tr -d ' ')" -eq "$(jq '.artifacts|length' "$ROOT/$manifest")" ]
jq -e '.accepted_by=="fixture" and (.approval_ref|endswith("/pull/1"))' "$TMP/kit/$first" >/dev/null
for approval in "$TMP/kit"/*.json; do sign "$TMP/fixture-key" "$approval"; done
import() { bash "$import_script" --root "$base" --kit "$1" --payloads "$TMP/payloads"; }
variant() {
    rm -rf "$TMP/variant"
    cp -R "$TMP/kit" "$TMP/variant"
}
variant; rm "$TMP/variant/$first.sig"
expect 4 missing-signature 'Missing signature' import "$TMP/variant"
variant; rm "$TMP/variant/$first" "$TMP/variant/$first.sig"
expect 4 missing-approval 'Missing approval' import "$TMP/variant"
variant; cp "$TMP/variant/$first" "$TMP/variant/extra.json"
expect 4 extra-approval 'without an unsigned payload' import "$TMP/variant"
variant; jq '.claim_set_sha256="0000"' "$TMP/kit/$first" >"$TMP/variant/$first"; sign "$TMP/fixture-key" "$TMP/variant/$first"
expect 4 changed-approval 'differs from its unsigned payload' import "$TMP/variant"
variant; jq '.accepted_by="reviewer"' "$TMP/kit/$first" >"$TMP/variant/$first"; sign "$TMP/other-key" "$TMP/variant/$first"
expect 4 mixed-signers 'do not share one signer' import "$TMP/variant"
variant; sign "$TMP/other-key" "$TMP/variant/$first"
expect 4 wrong-key 'does not verify' import "$TMP/variant"
[ -z "$(find "$base/docs/casper/cbc-evidence" -name '*.acceptance.json' -print -quit)" ]
expect 0 import '"status":"accepted"' import "$TMP/kit"
printf 'Cleanup signing: %s kit and import controls passed.\n' "$checks"

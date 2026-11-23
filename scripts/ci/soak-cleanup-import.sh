#!/usr/bin/env bash
set -euo pipefail
usage() {
    printf 'Usage: soak-cleanup-import.sh --kit DIR --payloads DIR [--root DIR]\n' >&2
    exit 2
}
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
kit=""
payloads=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --kit) kit="$2"; shift 2 ;;
        --payloads) payloads="$2"; shift 2 ;;
        --root) ROOT="$(cd "$2" && pwd -P)"; shift 2 ;;
        *) usage ;;
    esac
done
[ -n "$kit" ] && [ -n "$payloads" ] || usage
kit="$(cd "$kit" && pwd -P)"
payloads="$(cd "$payloads" && pwd -P)"
cd "$ROOT"
reject() { printf '%s\n' "$1" >&2; exit 4; }
signers="$(git config --local --get cbc.acceptanceAllowedSigners)" || reject 'Set cbc.acceptanceAllowedSigners first.'
[ -f "$signers" ] || reject 'The allowed-signers file does not exist.'
manifest=docs/casper/soak-cleanup.json
claim=CLAIM-SOAK-CLEANUP-001
expected="$(jq '.artifacts|length' "$manifest")"
for approval in "$kit"/*.json; do
    [ -f "$approval" ] || continue
    [ -f "$payloads/$(basename "$approval")" ] || reject "The kit has an approval without an unsigned payload: $(basename "$approval")"
done
identity=""
count=0
for unsigned in "$payloads"/*.json; do
    [ -f "$unsigned" ] || continue
    name="$(basename "$unsigned")"
    approval="$kit/$name"
    [ -f "$approval" ] || reject "Missing approval: $name"
    [ -s "$approval.sig" ] || reject "Missing signature: $name.sig"
    jq -n -e --slurpfile signed "$approval" --slurpfile original "$unsigned" \
        '($signed[0]|del(.accepted_by,.approved_at,.approval_ref)) == ($original[0]|del(.accepted_by,.approved_at,.approval_ref))' \
        >/dev/null || reject "The approval differs from its unsigned payload: $name"
    jq -e --arg claim "$claim" '.claim_id==$claim and (.accepted_by|length>0) and (.approved_at|length>0) and (.approval_ref|length>0)' \
        "$approval" >/dev/null || reject "The approval is incomplete: $name"
    current="$(jq -c '{accepted_by,approved_at,approval_ref}' "$approval")"
    [ -z "$identity" ] || [ "$identity" = "$current" ] || reject "The approvals do not share one signer, time, and review: $name"
    identity="$current"
    ssh-keygen -Y verify -f "$signers" -I "$(jq -r .accepted_by "$approval")" -n cbc-acceptance \
        -s "$approval.sig" <"$approval" >/dev/null 2>&1 || reject "The signature does not verify for its principal: $name"
    count=$((count + 1))
done
[ "$count" -eq "$expected" ] || reject "Found $count approvals; the manifest covers $expected artifacts."
for unsigned in "$payloads"/*.json; do
    approval="$kit/$(basename "$unsigned")"
    bash scripts/cbc.sh accept "$(jq -r .artifact "$approval")" --claim-id "$claim" \
        --approval "$approval" --signature "$approval.sig" >/dev/null
done
printf '%s approvals imported for %s.\n' "$count" "$(jq -r .accepted_by <<<"$identity")"
bash scripts/ci/check-soak-cleanup.sh --root "$ROOT" | jq -c '{status,gaps}'

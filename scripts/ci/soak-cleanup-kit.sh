#!/usr/bin/env bash
set -euo pipefail
usage() {
    printf 'Usage: soak-cleanup-kit.sh --payloads DIR --pull-request N --signer PRINCIPAL --kit DIR [--root DIR]\n' >&2
    exit 2
}
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
payloads=""
pr=""
signer=""
kit=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --payloads) payloads="$2"; shift 2 ;;
        --pull-request) pr="$2"; shift 2 ;;
        --signer) signer="$2"; shift 2 ;;
        --kit) kit="$2"; shift 2 ;;
        --root) ROOT="$(cd "$2" && pwd -P)"; shift 2 ;;
        *) usage ;;
    esac
done
[ -n "$payloads" ] && [ -n "$pr" ] && [ -n "$signer" ] && [ -n "$kit" ] || usage
[[ "$pr" =~ ^[1-9][0-9]*$ ]] || { printf 'The pull request must be a number.\n' >&2; exit 2; }
[[ "$signer" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || { printf 'The signer must be a principal name.\n' >&2; exit 2; }
payloads="$(cd "$payloads" && pwd -P)"
[ ! -e "$kit" ] || { printf 'The kit directory already exists: %s\n' "$kit" >&2; exit 2; }
mkdir -p "$kit"
kit="$(cd "$kit" && pwd -P)"
for directory in "$payloads" "$kit"; do
    [[ "$directory/" != "$ROOT/"* ]] || { printf 'Keep the signing files outside the worktree: %s\n' "$directory" >&2; exit 2; }
done
manifest="$ROOT/docs/casper/soak-cleanup.json"
expected="$(jq '.artifacts|length' "$manifest")"
at="$(date -u +%FT%TZ)"
ref="https://github.com/F1R3FLY-io/f1r3node-rust/pull/$pr"
count=0
for unsigned in "$payloads"/*.json; do
    [ -f "$unsigned" ] || continue
    jq -e '.accepted_by=="" and .approved_at=="" and .approval_ref==""' "$unsigned" >/dev/null ||
        { printf 'Not an unsigned approval: %s\n' "$unsigned" >&2; exit 2; }
    jq -e --slurpfile manifest "$manifest" '. as $approval | .claim_id=="CLAIM-SOAK-CLEANUP-001" and ($manifest[0].artifacts|has($approval.artifact))' \
        "$unsigned" >/dev/null || { printf 'Not a cleanup approval: %s\n' "$unsigned" >&2; exit 2; }
    jq --arg by "$signer" --arg at "$at" --arg ref "$ref" \
        '.accepted_by=$by|.approved_at=$at|.approval_ref=$ref' "$unsigned" >"$kit/$(basename "$unsigned")"
    count=$((count + 1))
done
[ "$count" -eq "$expected" ] || { printf 'Found %s approvals; the manifest covers %s artifacts.\n' "$count" "$expected" >&2; exit 2; }
jq -s -e --slurpfile manifest "$manifest" \
    '[.[].artifact] as $covered | ($covered|length)==($covered|unique|length) and ($covered|unique)==($manifest[0].artifacts|keys|sort)' \
    "$kit"/*.json >/dev/null || { printf 'The approvals must cover each manifest artifact exactly once.\n' >&2; exit 2; }
printf '%s approvals for %s, review %s\n' "$count" "$signer" "$ref"
printf 'Sign them in %s:\n' "$kit"
printf '  for f in *.json; do ssh-keygen -Y sign -f <private key> -n cbc-acceptance "$f"; done\n'

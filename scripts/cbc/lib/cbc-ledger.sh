#!/usr/bin/env bash
#
# cbc-ledger.sh - read/write the CbC evidence ledger.
#
# One auditable record per artifact under docs/cbc-evidence/ (override with
# CBC_EVIDENCE_DIR): Markdown summary + an embedded machine-readable JSON block.
# Part of the /cbc skill (EPIC-025). See docs/designs/cbc-skill.md.

[[ -n "${_CBC_LEDGER_SH_LOADED:-}" ]] && return 0
_CBC_LEDGER_SH_LOADED=1
_CBC_LEDGER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

cbc_acceptance_tool() {
    python3 "$_CBC_LEDGER_DIR/cbc-acceptance.py" "$@"
}

cbc_ledger_dir() {
    python3 "$_CBC_LEDGER_DIR/cbc-attributes.py" evidence-dir "${1:-}"
}

# Derive the stable slug from the normalized repository-relative artifact path.
cbc_ledger_slug() {
    python3 "$_CBC_LEDGER_DIR/cbc-attributes.py" evidence-slug "$1"
}

cbc_ledger_path() {
    python3 "$_CBC_LEDGER_DIR/cbc-attributes.py" evidence-path "$1"
}

# Write a record.
# Args: artifact commit claim adapter status evidence_json [waiver_json]
# Echoes the record path.
cbc_ledger_write() {
    local artifact="$1" commit="$2" claim="$3" adapter="$4" status="$5"
    local evidence="$6" waiver="${7:-null}"
    local dir id path ts record
    path="$(cbc_ledger_path "$artifact")" || return 3
    dir="$(dirname "$path")"; mkdir -p "$dir" || return 3
    id="$(cbc_ledger_slug "$artifact")" || return 3
    ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

    record=$(jq -n \
        --arg path "$artifact" --arg commit "$commit" --arg id "$id" \
        --arg claim "$claim" --arg adapter "$adapter" --arg status "$status" \
        --arg ts "$ts" --argjson evidence "$evidence" --argjson waiver "$waiver" \
        '{artifact: {path: $path, commit: $commit, id: $id}, claim: $claim,
          adapter: $adapter, status: $status, evidence: $evidence,
          waiver: $waiver, verified_at: $ts}')

    [[ -n "$record" ]] || return 3
    printf '%s\n' "$record" | cbc_acceptance_tool record "$artifact" "$path" "${8:-}" "${9:-}" "${10:-}"
}

# Record an explicit waiver against an artifact (used by `cbc waive`, TODO-025-004).
# Args: artifact reason by
cbc_ledger_waive() {
    local artifact="$1" reason="$2" by="${3:-unknown}" claim_id="${4:-}" claim_file="${5:-}"
    local commit waiver
    commit="$(git rev-parse --short HEAD 2>/dev/null || echo uncommitted)"
    waiver=$(jq -n --arg reason "$reason" --arg by "$by" \
        --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" '{reason: $reason, by: $by, at: $at}')
    cbc_ledger_write "$artifact" "$commit" "" "waiver" "waived" \
        '{"kind":"waiver","ref":null,"counterexample":null,"detail":"explicit waiver"}' "$waiver" "$claim_file" "" "$claim_id"
}

# Echo an artifact's recorded status, or "none".
# Shared effective status for discharge and completion. Fail closed on policy errors.
cbc_gate_report() {
    local artifact="$1" policy mandatory attrs ledger result
    attrs=$(git check-attr cbc cbc-acceptance -- "$artifact" 2>/dev/null) || { jq -n --arg path "$artifact" '{path:$path,status:"policy_error"}'; return 0; }
    mandatory=$(printf '%s\n' "$attrs" | sed -n 's/.*: cbc: //p')
    policy=$(printf '%s\n' "$attrs" | sed -n 's/.*: cbc-acceptance: //p')
    case "$policy" in
        unspecified) ;;
        required) [[ "$mandatory" == mandatory ]] || { jq -n --arg path "$artifact" '{path:$path,status:"policy_error"}'; return 0; } ;;
        *) jq -n --arg path "$artifact" '{path:$path,status:"policy_error"}'; return 0 ;;
    esac
    [[ "$mandatory" == mandatory ]] || { jq -n --arg path "$artifact" '{path:$path,status:"not_required"}'; return 0; }
    ledger=$(cbc_ledger_path "$artifact") || { jq -n --arg path "$artifact" '{path:$path,status:"policy_error"}'; return 0; }
    result=$(cbc_acceptance_tool report "$artifact" "$ledger" "$policy") || { jq -n --arg path "$artifact" '{path:$path,status:"invalid_record"}'; return 0; }
    printf '%s\n' "$result"
}

cbc_gate_status() {
    local result
    result=$(cbc_gate_report "$1") || { echo policy_error; return 0; }
    printf '%s' "$result" | jq -r '.status'
}

cbc_ledger_status() {
    local path; path="$(cbc_ledger_path "$1")" || return 3
    cbc_acceptance_tool status "$1" "$path"
}

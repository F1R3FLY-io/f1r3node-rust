#!/usr/bin/env bash
#
# cbc-gate.sh - the CbC discharge gate and waiver recording.
#
#   cbc discharge : every in-scope CbC-mandatory artifact must carry a
#                   discharged or waived evidence record; --strict exits 4 on gaps.
#   cbc waive     : record an explicit waiver (rationale) against an artifact.
#
# Realizes FLOW-009 step 7. Part of /cbc (EPIC-025). See docs/designs/cbc-skill.md.

[[ -n "${_CBC_GATE_SH_LOADED:-}" ]] && return 0
_CBC_GATE_SH_LOADED=1

_CBC_GATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cbc-identify.sh
source "$_CBC_GATE_DIR/cbc-identify.sh"
# shellcheck source=cbc-ledger.sh
source "$_CBC_GATE_DIR/cbc-ledger.sh"

# cbc discharge [--scope diff|epic <EID>] [--files "..."] [--base <ref>] [--strict] [--json]
cbc_discharge() {
    local strict=0 json=0 rc=0
    local -a id_args=()
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --strict) strict=1; shift ;;
            --json)   json=1; shift ;;
            --scope)
                id_args+=("--scope" "${2:-}")
                if [[ "${2:-}" == "epic" || "${2:-}" == "flow" ]]; then
                    id_args+=("${3:-}"); shift 3 || true
                else
                    shift 2 || true
                fi ;;
            --files)  id_args+=("--files" "${2:-}"); shift 2 || true ;;
            --base)   id_args+=("--base" "${2:-}"); shift 2 || true ;;
            *) echo "cbc discharge: unknown option '$1'" >&2; return 3 ;;
        esac
    done

    # Reuse `cbc identify` (scope resolution + tag filtering) for the candidate set.
    local mand
    if [[ ${#id_args[@]} -gt 0 ]]; then
        mand=$(cbc_identify "${id_args[@]}" --json) || rc=$?
    else
        mand=$(cbc_identify --json) || rc=$?
    fi
    [[ $rc -ne 0 ]] && return $rc

    local rows="" path st result gaps=0 total=0
    while IFS= read -r path; do
        [[ -z "$path" ]] && continue
        total=$((total + 1))
        result=$(cbc_gate_report "$path") || return 3
        st=$(printf '%s' "$result" | jq -r '.status') || return 3
        case "$st" in
            discharged|waived) : ;;
            *) gaps=$((gaps + 1)) ;;
        esac
        rows+="$result"$'\n'
    done < <(echo "$mand" | jq -r '.[].path')
    rows=$(printf '%s' "$rows" | sed '/^$/d')

    if [[ "$json" -eq 1 ]]; then
        if [[ -z "$rows" ]]; then
            echo '{"total":0,"gaps":0,"items":[]}'
        else
            printf '%s\n' "$rows" | jq -s --argjson g "$gaps" --argjson t "$total" \
                '{total:$t, gaps:$g, items:.}'
        fi
    else
        if [[ "$total" -eq 0 ]]; then
            echo "cbc discharge: no CbC-mandatory files in scope."
        else
            echo "cbc discharge: $((total - gaps))/$total discharged or waived ($gaps undischarged)"
            printf '%s\n' "$rows" | jq -c '.' | while IFS= read -r result; do
                st=$(printf '%s' "$result" | jq -r '.status')
                path=$(printf '%s' "$result" | jq -r '.path')
                printf '  %-12s %s\n' "$st" "$path"
                printf '%s' "$result" | jq -r '.claims[]? | "    \(.claim_id): \(.status)"'
            done
        fi
    fi

    [[ "$gaps" -gt 0 && "$strict" -eq 1 ]] && return 4
    return 0
}

# cbc waive <artifact> --reason TEXT
cbc_waive() {
    local artifact="" reason="" claim_id="" claim_file=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --reason) [[ $# -ge 2 && -n "$2" ]] || return 3; reason="$2"; shift 2 ;;
            --claim-id) [[ $# -ge 2 && -n "$2" && -z "$claim_id" ]] || return 3; claim_id="$2"; shift 2 ;;
            --claim) [[ $# -ge 2 && -n "$2" ]] || return 3; claim_file="$2"; shift 2 ;;
            -*) echo "cbc waive: unknown option '$1'" >&2; return 3 ;;
            *) [[ -z "$artifact" ]] && artifact="$1"; shift ;;
        esac
    done
    [[ -n "$artifact" ]] || { echo "cbc waive: artifact required" >&2; return 3; }
    [[ -n "$reason" ]]   || { echo "cbc waive: --reason TEXT is required" >&2; return 3; }
    [[ -f "$artifact" ]] || { echo "cbc waive: artifact not found: $artifact" >&2; return 1; }

    local by path ledger
    ledger=$(cbc_ledger_path "$artifact") || return 3
    cbc_acceptance_tool select "$artifact" "$ledger" "$claim_id" || return 3
    [[ -z "$claim_file" || -f "$claim_file" ]] || return 1
    by="$(git config user.email 2>/dev/null || echo unknown)"
    path="$(cbc_ledger_waive "$artifact" "$reason" "$by" "$claim_id" "$claim_file")" || return 3
    echo "cbc waive: $artifact -> waived; evidence: $path"
    echo "  gate: $(cbc_gate_status "$artifact")"
}

# Import only independently signed approval. Never sign or infer approval here.
cbc_accept() {
    local artifact="${1:-}" approval="" signature="" claim_id=""
    [[ -n "$artifact" ]] || return 3
    shift
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --claim-id) [[ $# -ge 2 && -n "$2" && -z "$claim_id" ]] || return 3; claim_id="$2"; shift 2 ;;
            --approval|--signature)
                [[ $# -ge 2 && -n "$2" ]] || return 3
                if [[ "$1" == --approval ]]; then approval="$2"; else signature="$2"; fi
                shift 2 ;;
            *) echo "cbc accept: unknown option '$1'" >&2; return 3 ;;
        esac
    done
    [[ -n "$approval" && -n "$signature" ]] || { echo "cbc accept: --approval and --signature are required" >&2; return 3; }
    local ledger
    ledger=$(cbc_ledger_path "$artifact") || return 3
    cbc_acceptance_tool select "$artifact" "$ledger" "$claim_id" || return 3
    cbc_acceptance_tool accept "$artifact" "$ledger" "$approval" "$signature" "$claim_id"
}

cbc_acceptance_payload() {
    local artifact="${1:-}" claim_id="" ledger
    [[ -n "$artifact" ]] || return 3
    shift
    if [[ $# -gt 0 ]]; then
        [[ $# -eq 2 && "$1" == --claim-id && -n "$2" ]] || return 3
        claim_id="$2"
    fi
    ledger=$(cbc_ledger_path "$artifact") || return 3
    cbc_acceptance_tool select "$artifact" "$ledger" "$claim_id" || return 3
    cbc_acceptance_tool payload "$artifact" "$ledger" "$claim_id"
}

cbc_claim_set() {
    local artifact="${1:-}" ledger
    local ids=()
    [[ -n "$artifact" ]] || return 3
    shift
    while [[ $# -gt 0 ]]; do
        [[ $# -ge 2 && "$1" == --claim-id && -n "$2" ]] || return 3
        ids+=("$2")
        shift 2
    done
    [[ ${#ids[@]} -gt 0 && -f "$artifact" ]] || return 3
    ledger=$(cbc_ledger_path "$artifact") || return 3
    cbc_acceptance_tool claim-set "$artifact" "$ledger" "${ids[@]}" || return 3
}

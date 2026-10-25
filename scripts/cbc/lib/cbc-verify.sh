#!/usr/bin/env bash
#
# cbc-verify.sh - run a verifier adapter to discharge an artifact's claim, then
# record the result in the evidence ledger. Variant-agnostic: it selects a
# pluggable adapter (mock / agentic / embedded), calls the adapter contract, and
# assembles the full ledger record. Part of /cbc (EPIC-025).
#
# See docs/designs/cbc-skill.md.

[[ -n "${_CBC_VERIFY_SH_LOADED:-}" ]] && return 0
_CBC_VERIFY_SH_LOADED=1

_CBC_VERIFY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cbc-ledger.sh
source "$_CBC_VERIFY_DIR/cbc-ledger.sh"

# cbc verify <artifact> [--adapter NAME] [--claim PATH] [--prover NAME]
cbc_verify() {
    local artifact="" adapter="${CBC_ADAPTER:-agentic}" claim_file="" claim_id="" ledger
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --adapter) [[ $# -ge 2 && -n "$2" ]] || return 3; adapter="$2"; shift 2 ;;
            --claim) [[ $# -ge 2 && -n "$2" ]] || return 3; claim_file="$2"; shift 2 ;;
            --claim-id) [[ $# -ge 2 && -n "$2" && -z "$claim_id" ]] || return 3; claim_id="$2"; shift 2 ;;
            --prover) [[ $# -ge 2 && -n "$2" ]] || return 3; export CBC_PROVER="$2"; shift 2 ;;
            -*)        echo "cbc verify: unknown option '$1'" >&2; return 3 ;;
            *)         [[ -z "$artifact" ]] && artifact="$1"; shift ;;
        esac
    done

    [[ -n "$artifact" ]] || { echo "cbc verify: artifact required" >&2; return 3; }
    [[ -f "$artifact" ]] || { echo "cbc verify: artifact not found: $artifact" >&2; return 1; }

    ledger=$(cbc_ledger_path "$artifact") || return 3
    cbc_acceptance_tool select "$artifact" "$ledger" "$claim_id" || return 3
    local adapter_file="$_CBC_VERIFY_DIR/cbc-adapters/${adapter}.sh"
    [[ -f "$adapter_file" ]] || { echo "cbc verify: unknown adapter '$adapter'" >&2; return 3; }
    # shellcheck source=/dev/null
    source "$adapter_file"

    if ! CBC_ARTIFACT_HINT="$artifact" cbc_adapter_available; then
        echo "cbc verify: adapter '$adapter' unavailable (missing toolchain/keys); nothing recorded" >&2
        return 3
    fi

    local claim="" commit evidence_tmp rc=0 inputs
    if [[ -n "$claim_file" && ! -f "$claim_file" ]]; then
        echo "cbc verify: claim not found: $claim_file" >&2
        return 1
    fi
    [[ -n "$claim_file" ]] && claim="$(cat "$claim_file")"
    inputs=$(cbc_acceptance_tool snapshot "$artifact" "$claim_file") || return 3
    commit="$(git rev-parse --short HEAD 2>/dev/null || echo uncommitted)"
    evidence_tmp="$(mktemp)"

    cbc_adapter_verify "$artifact" "$claim_file" "$evidence_tmp" || rc=$?
    local evidence; evidence="$(cat "$evidence_tmp" 2>/dev/null)"; rm -f "$evidence_tmp"
    [[ -z "$evidence" ]] && evidence='{"kind":"error","ref":null,"counterexample":null,"detail":"adapter produced no evidence"}'

    local status
    case "$rc" in
        0) status="discharged" ;;
        2) status="refuted" ;;
        *) status="error" ;;
    esac

    local path
    path="$(cbc_ledger_write "$artifact" "$commit" "$claim" "$(cbc_adapter_name)" "$status" "$evidence" null "$claim_file" "$inputs" "$claim_id")" || return 3

    echo "cbc verify: $artifact -> $status (adapter: $adapter)"
    echo "  evidence: $path"
    [[ "$status" == "refuted" ]] && echo "  refuted - loop back to synthesis; merge stays gated."
    return "$rc"
}

_cbc_status_details() {
    local artifact="$1" ledger="$2" metadata="${3:-}" gate
    [[ -n "$metadata" ]] || metadata=$(cbc_acceptance_tool metadata "$artifact" "$ledger") || return 3
    gate=$(cbc_gate_report "$artifact") || return 3
    jq -cn --arg path "$artifact" --argjson raw "$metadata" --argjson gate "$gate" '
        {path:$path,status:$raw.status,gate:$gate.status} +
        (if $raw.claims then {claims: [$raw.claims[] | . as $claim |
          . + {gate: ([$gate.claims[]? | select(.claim_id == $claim.claim_id) | .status][0] // $gate.status)}]}
         else {} end)'
}

# cbc status [<artifact>] [--json]
cbc_status() {
    local artifact="" json=0
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --json) json=1 ;;
            -*) echo "cbc status: unknown option '$1'" >&2; return 3 ;;
            *) [[ -z "$artifact" ]] || return 3; artifact="$1" ;;
        esac
        shift
    done
    if [[ -n "$artifact" ]]; then
        local ledger result status gate
        ledger=$(cbc_ledger_path "$artifact") || return 3
        result=$(_cbc_status_details "$artifact" "$ledger") || return 3
        if [[ "$json" -eq 1 ]]; then
            printf '%s\n' "$result"
        else
            status=$(printf '%s' "$result" | jq -r '.status')
            gate=$(printf '%s' "$result" | jq -r '.gate')
            echo "$artifact: $status (gate: $gate)"
            printf '%s' "$result" | jq -r '.claims[]? | "    \(.claim_id): \(.status) (gate: \(.gate))"'
        fi
        return 0
    fi
    local directories dir f details active st p prior duplicate row identity aggregate_gate rows="" any=0
    local seen=()
    directories=$(python3 "$_CBC_VERIFY_DIR/cbc-attributes.py" evidence-dirs) || return 3
    while IFS= read -r dir; do
        for f in "$dir"/*.md "$dir"/*.claims.json; do
            [[ -e "$f" ]] || continue
            identity="$f"
            [[ "$f" == *.claims.json ]] && f="${f%.claims.json}.md"
            f="${f#./}"
            [[ -e "$f" ]] && identity="$f"
            details=$(cbc_acceptance_tool metadata "" "$f") || return 3
            st=$(printf '%s' "$details" | jq -r '.status')
            p=$(printf '%s' "$details" | jq -r '.artifact // empty')
            if [[ -n "$p" ]]; then
                active=$(cbc_ledger_path "$p") || return 3
                if [[ -e "$f" ]]; then
                    [[ "$active" -ef "$f" ]] || continue
                else
                    [[ "${active%.md}.claims.json" -ef "$identity" ]] || continue
                fi
            fi
            duplicate=0
            for prior in "${seen[@]}"; do
                if [[ "$prior" -ef "$identity" ]]; then
                    duplicate=1
                    break
                fi
            done
            [[ "$duplicate" -eq 0 ]] || continue
            seen+=("$identity")
            any=1
            row=$(printf '%s' "$details" | jq -c --arg path "${p:-$f}" 'del(.artifact) + {path:$path}')
            if [[ -n "$p" ]] && printf '%s' "$details" | jq -e 'has("claims")' >/dev/null; then
                row=$(_cbc_status_details "$p" "$f" "$details") || return 3
            fi
            if [[ "$json" -eq 1 ]]; then
                rows+="$row"$'\n'
            else
                if printf '%s' "$row" | jq -e 'has("gate")' >/dev/null; then
                    aggregate_gate=$(printf '%s' "$row" | jq -r '.gate')
                    printf '  %-12s %s (gate: %s)\n' "$st" "${p:-$f}" "$aggregate_gate"
                else
                    printf '  %-12s %s\n' "$st" "${p:-$f}"
                fi
                printf '%s' "$row" | jq -r '.claims[]? | "    \(.claim_id): \(.status) (gate: \(.gate // .status))"'
            fi
        done
    done < <(printf '%s' "$directories" | jq -r '.[]')
    if [[ "$json" -eq 1 ]]; then
        printf '%s' "$rows" | jq -s '.'
    elif [[ "$any" -eq 0 ]]; then
        echo "No evidence ledger yet in the selected directories."
    fi
    return 0
}

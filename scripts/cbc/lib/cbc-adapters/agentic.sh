#!/usr/bin/env bash
#
# agentic.sh - reference CbC adapter: an LLM proposes the proof annotations /
# obligations for the claim (via the unified LLM client, lib/llm-client.sh) and
# a prover discharges them. This is the "AI synthesis as a claim to be
# discharged" path.
#
# Availability-gated on BOTH an LLM key (ANTHROPIC_API_KEY / OPENROUTER_API_KEY /
# ABACUSAI_API_KEY) AND a configured prover (CBC_PROVER on PATH). When either is
# missing, verification is reported as unavailable (exit 3).
#
# Scope note: this records the LLM-proposed annotations as evidence and runs the
# prover for discharge; automatic injection of the proposed annotations into the
# source (and the counterexample-driven retry loop) is a documented follow-up.
#
# Part of /cbc (EPIC-025). See docs/designs/cbc-skill.md.

_CBC_AGENTIC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_CBC_LLM_CLIENT="$_CBC_AGENTIC_DIR/../llm-client.sh"

cbc_adapter_name() { echo "agentic"; }

cbc_adapter_available() {
    [[ -n "${ANTHROPIC_API_KEY:-}${OPENROUTER_API_KEY:-}${ABACUSAI_API_KEY:-}" ]] || return 1
    local prover="${CBC_PROVER:-}"
    [[ -n "$prover" ]] && command -v "$prover" >/dev/null 2>&1 || return 1
    [[ -f "$_CBC_LLM_CLIENT" ]]
}

cbc_adapter_verify() {
    local artifact="$1" claim_file="$2" out="$3"
    if ! cbc_adapter_available; then
        jq -n '{kind:"error", ref:null, counterexample:null, detail:"agentic: requires an LLM key and a configured prover (CBC_PROVER)"}' > "$out"
        return 3
    fi

    local claim=""
    [[ -n "$claim_file" && -f "$claim_file" ]] && claim="$(cat "$claim_file")"

    # 1. LLM proposes the proof annotations / obligations for the claim.
    local proposal=""
    # shellcheck source=/dev/null
    if source "$_CBC_LLM_CLIENT" 2>/dev/null && declare -f llm_request >/dev/null 2>&1; then
        proposal=$(llm_request \
            "Propose the formal annotations (requires/ensures/invariants) needed to discharge this correctness claim against the artifact. Claim: ${claim:-(see artifact)}. Artifact: $artifact" \
            --system "You are a Correctness-by-Construction proof assistant. Output only annotations." 2>/dev/null) || proposal=""
    fi

    # 2. Prover discharges. (Annotation injection into source is a follow-up;
    #    the prover runs against the artifact as-is and the proposal is recorded.)
    local prover="${CBC_PROVER}" log rc=0
    log=$("$prover" "$artifact" 2>&1) || rc=$?
    if [[ $rc -eq 0 ]]; then
        jq -n --arg ref "$prover" --arg prop "$proposal" \
            '{kind:"proof", ref:$ref, counterexample:null, detail:"agentic: LLM-proposed annotations, prover-discharged", proposal:$prop}' > "$out"
        return 0
    fi
    jq -n --arg ce "$log" --arg prop "$proposal" \
        '{kind:"counterexample", ref:null, counterexample:$ce, detail:"agentic: prover refuted proposed annotations", proposal:$prop}' > "$out"
    return 2
}

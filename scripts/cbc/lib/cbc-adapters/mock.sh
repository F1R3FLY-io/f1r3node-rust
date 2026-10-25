#!/usr/bin/env bash
#
# mock.sh - deterministic CbC verifier adapter for tests and local dev.
#
# Outcome controlled by CBC_MOCK_RESULT=discharged|refuted|error (default
# discharged). Exercises the verify -> evidence -> ledger pipeline without any
# real verifier toolchain. Part of /cbc (EPIC-025). See docs/designs/cbc-skill.md.
#
# Adapter contract:
#   cbc_adapter_name
#   cbc_adapter_available           -> exit 0 if usable
#   cbc_adapter_verify <artifact> <claim-file> <evidence-out>
#                                   -> 0 discharged / 2 refuted / 3 error
#                                      (writes normalized evidence JSON to out)

cbc_adapter_name() { echo "mock"; }

cbc_adapter_available() { return 0; }

cbc_adapter_verify() {
    local out="$3"
    case "${CBC_MOCK_RESULT:-discharged}" in
        discharged)
            jq -n '{kind:"proof", ref:"mock", counterexample:null, detail:"mock: discharged"}' > "$out"
            return 0 ;;
        refuted)
            jq -n '{kind:"counterexample", ref:null, counterexample:"mock counterexample", detail:"mock: refuted"}' > "$out"
            return 2 ;;
        *)
            jq -n '{kind:"error", ref:null, counterexample:null, detail:"mock: error"}' > "$out"
            return 3 ;;
    esac
}

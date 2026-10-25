#!/usr/bin/env bash
#
# cbc.sh - /cbc skill driver: identify mission-critical code, discharge its
# correctness claims through a pluggable verifier, and gate completion.
#
# This is the EPIC-025 driver. identify, verify, status, discharge, and waive
# are implemented; focused CbC skills route to this script where applicable.
#
# See docs/designs/cbc-skill.md and AItools/commands/cbc.md.

# Note: intentionally NOT using `set -e`. The git-attribute parsing relies on
# commands whose non-zero exit is meaningful (e.g. grep/check-attr), and errors
# are propagated via explicit return codes. pipefail is kept so scope-resolution
# failures surface through the identify pipeline.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/cbc-identify.sh
source "$SCRIPT_DIR/lib/cbc-identify.sh"
# shellcheck source=lib/cbc-verify.sh
source "$SCRIPT_DIR/lib/cbc-verify.sh"
# shellcheck source=lib/cbc-gate.sh
source "$SCRIPT_DIR/lib/cbc-gate.sh"

usage() {
    cat <<'EOF'
/cbc <subcommand> [arguments]

  attributes snapshot|lint [--cached]
  attributes compare <snapshot> [--cached]
            Inspect effective hierarchical policy without changing attributes.
  identify  [--scope diff|epic <EID>] [--files "a b c"] [--base <ref>] [--json]
            List CbC-mandatory files in scope (default: working diff vs HEAD).
  claim-set <artifact> --claim-id ID [--claim-id ID ...]
            Declare expected claims without dropping an existing claim.
  verify    <artifact> [--claim-id ID] [--adapter mock|agentic|embedded] [--claim PATH] [--prover NAME]
            Discharge the artifact's claim via a verifier adapter; record evidence.
  status    [<artifact>] [--json]
            Show ledger state for an artifact, or a summary across the ledger.
  discharge [--scope diff|epic <EID>] [--files "a b c"] [--strict] [--json]
            Gate: every in-scope CbC-mandatory artifact must be discharged or
            waived. --strict exits non-zero when any remain undischarged.
  acceptance-payload <artifact> [--claim-id ID]
            Print the unsigned binding for independent maintainer review.
  accept    <artifact> [--claim-id ID] --approval PATH --signature PATH
            Import a signed approval from an authorized maintainer.
  waive     <artifact> [--claim-id ID] [--claim PATH] --reason TEXT
            Record an explicit waiver (rationale) against an artifact.

CbC-mandatory code is tagged via git attributes (cbc=mandatory). Tags are read
here and proposed by the review tooling (TODO-025-006). See
docs/designs/cbc-skill.md.
EOF
}

main() {
    local sub="${1:-}"
    if [[ $# -gt 0 ]]; then shift; fi
    case "$sub" in
        attributes)
            python3 "$SCRIPT_DIR/lib/cbc-attributes.py" "$@"
            ;;
        identify)
            cbc_identify "$@"
            ;;
        claim-set)
            cbc_claim_set "$@"
            ;;
        verify)
            cbc_verify "$@"
            ;;
        acceptance-payload)
            cbc_acceptance_payload "$@"
            ;;
        accept)
            cbc_accept "$@"
            ;;
        status)
            cbc_status "$@"
            ;;
        discharge)
            cbc_discharge "$@"
            ;;
        waive)
            cbc_waive "$@"
            ;;
        -h|--help|help|"")
            usage
            ;;
        *)
            echo "cbc: unknown subcommand '$sub'" >&2
            usage >&2
            return 3
            ;;
    esac
}

main "$@"

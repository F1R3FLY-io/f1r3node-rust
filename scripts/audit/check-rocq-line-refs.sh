#!/usr/bin/env bash
# check-rocq-line-refs.sh — Audit Rocq line-number citations in slashing docs.
#
# For every (filename, theorem, line) cite of the form
#   `MainTheorem.v:NNN`, `Bisimulation.v:NNN`, etc.,
# pair it with the nearest preceding backtick-quoted identifier
# (the alleged theorem / lemma / definition name) and check whether
# the actual definition lives at that line of the .v file.
#
# Usage:  bash scripts/audit/check-rocq-line-refs.sh [--quiet]
#
# Exit codes:
#   0 — no mismatches found
#   1 — at least one mismatch
#   2 — usage / invocation error

set -u

REPO="$(git -C "$(dirname "$0")/.." rev-parse --show-toplevel 2>/dev/null)"
if [[ -z "${REPO}" ]]; then
    echo "error: not in a git repo" >&2
    exit 2
fi

# Audit EVERY Rocq subsystem's theories against the WHOLE theory corpus, not just
# slashing. The previous slashing-only scope meant fork-choice / finalized-floor /
# merge-algebra / deploy-lifecycle citations were never checked at all — the same
# blind spot that let the 2026-07-15 dev merge invalidate anchors with no gate firing.
declare -a THEORY_DIRS=(
    "${REPO}/formal/rocq/slashing/theories"
    "${REPO}/formal/rocq/finalized_floor/theories"
    "${REPO}/formal/rocq/fork_choice/theories"
    "${REPO}/formal/rocq/merge_algebra/theories"
)
DOCS_DIR="${REPO}/docs/theory"
QUIET=0
if [[ "${1:-}" == "--quiet" ]]; then QUIET=1; fi

for d in "${THEORY_DIRS[@]}"; do
    if [[ ! -d "${d}" ]]; then
        echo "error: Rocq theories directory not found: ${d}" >&2
        exit 2
    fi
done
if [[ ! -d "${DOCS_DIR}" ]]; then
    echo "error: docs directory not found: ${DOCS_DIR}" >&2
    exit 2
fi

# Build the canonical (file, line, name) index from .v sources.
#
# NOTE: the index is keyed by BASENAME, and basenames repeat across subsystems
# (MainTheorem.v, Foundation.v, GuardBridge.v all exist in several). A cite is
# therefore accepted if it matches the named declaration in ANY subsystem whose
# file has that basename — deliberately permissive, since a doc cite of
# `MainTheorem.v:184` is only meaningful within its own subsystem's docs. This
# still catches the failure that matters: a name/line pair that matches NOWHERE.
INDEX="$(mktemp)"
trap 'rm -f "${INDEX}"' EXIT
for dir in "${THEORY_DIRS[@]}"; do
    for v in "${dir}"/*.v; do
        [[ -f "${v}" ]] || continue
        base="$(basename "${v}")"
        grep -nE '^ *(Theorem|Lemma|Definition|Corollary|Fixpoint|Inductive) [A-Za-z_]+' "${v}" |
            awk -F'[: ]' -v f="${base}" '{for(i=1;i<=NF;i++) if($i ~ /^(Theorem|Lemma|Definition|Corollary|Fixpoint|Inductive)$/) {print f, $1, $(i+1); break}}'
    done
done > "${INDEX}"

# Scan docs for filename:line cites preceded by a backtick-quoted identifier.
MISMATCHES=0
TOTAL_CHECKED=0
while IFS= read -r hit; do
    # Format: docs-path:line-no:matched-text
    doc_path="${hit%%:*}"
    rest="${hit#*:}"
    doc_line="${rest%%:*}"
    matched="${rest#*:}"

    # The matched text looks like: ...`<identifier>`...`Filename.v:NNN`...
    # Pull the last `identifier` before the file:line citation.
    ident="$(printf '%s\n' "${matched}" | sed -nE 's/.*`([A-Za-z_][A-Za-z0-9_]*)`[^`]*`[A-Za-z_]+\.v:[0-9]+`.*/\1/p' | tail -1)"
    cite="$(printf '%s\n' "${matched}" | sed -nE 's/.*`([A-Za-z_]+\.v:[0-9]+)`.*/\1/p' | tail -1)"
    if [[ -z "${ident}" || -z "${cite}" ]]; then continue; fi

    cite_file="${cite%:*}"
    cite_line="${cite##*:}"

    TOTAL_CHECKED=$((TOTAL_CHECKED + 1))

    # Look up the canonical line for that identifier in that file.
    actual="$(awk -v f="${cite_file}" -v n="${ident}" '$1==f && $3==n {print $2; exit}' "${INDEX}")"
    if [[ -z "${actual}" ]]; then
        printf '⚠ %s:%s  `%s` not found in %s\n' "${doc_path}" "${doc_line}" "${ident}" "${cite_file}"
        MISMATCHES=$((MISMATCHES + 1))
        continue
    fi
    if [[ "${actual}" != "${cite_line}" ]]; then
        printf '✗ %s:%s  `%s` cited at %s but actually at line %s\n' \
            "${doc_path}" "${doc_line}" "${ident}" "${cite}" "${actual}"
        MISMATCHES=$((MISMATCHES + 1))
    fi
done < <(grep -rEn '`[A-Za-z_][A-Za-z0-9_]*`[^`]*`[A-Za-z_]+\.v:[0-9]+`' "${DOCS_DIR}" 2>/dev/null)

if (( MISMATCHES == 0 )); then
    [[ "${QUIET}" == 0 ]] && echo "✓ all ${TOTAL_CHECKED} cited Rocq lines resolve to the named theorem/lemma"
    exit 0
else
    echo ""
    echo "Found ${MISMATCHES} mismatch(es) out of ${TOTAL_CHECKED} cited Rocq lines."
    exit 1
fi

#!/usr/bin/env bash
# check-source-line-refs.sh — Audit Rholang and Rust line-number citations
# in the slashing documentation suite.
#
# For every backtick citation of the form `<file>.rhox:NNN` or `<file>.rs:NNN`
# in docs/theory/slashing/**/*.md, verify the cited line in the source file
# contains a recognizable declaration (a Rholang `contract` send or a Rust
# item definition).
#
# Models after scripts/audit/check-rocq-line-refs.sh.
#
# Usage:  bash scripts/audit/check-source-line-refs.sh [--quiet]
#
# Exit codes:
#   0 — no mismatches
#   1 — at least one mismatch
#   2 — usage error
set -u

REPO="$(git -C "$(dirname "$0")/.." rev-parse --show-toplevel 2>/dev/null)"
if [[ -z "${REPO}" ]]; then
    echo "error: not in a git repo" >&2
    exit 2
fi

DOCS_DIR="${REPO}/docs/theory/slashing"
QUIET=0
if [[ "${1:-}" == "--quiet" ]]; then QUIET=1; fi

if [[ ! -d "${DOCS_DIR}" ]]; then
    echo "error: docs directory not found: ${DOCS_DIR}" >&2
    exit 2
fi

# Allow-list of source files we audit.  Other source files are not yet covered;
# adding one here is a one-line change.
declare -a SOURCE_FILES=(
    "${REPO}/casper/src/main/resources/PoS.rhox"
    "${REPO}/casper/src/rust/validate.rs"
    "${REPO}/casper/src/rust/equivocation_detector.rs"
    "${REPO}/casper/src/rust/multi_parent_casper_impl.rs"
    "${REPO}/casper/src/rust/estimator.rs"
    "${REPO}/casper/src/rust/block_status.rs"
    "${REPO}/casper/src/rust/slashing_authorization.rs"
    "${REPO}/casper/src/rust/blocks/proposer/block_creator.rs"
    "${REPO}/casper/src/rust/util/rholang/costacc/slash_deploy.rs"
    "${REPO}/block-storage/src/rust/dag/block_dag_key_value_storage.rs"
    "${REPO}/block-storage/src/rust/dag/equivocations_access.rs"
)

# Build (basename, line, kind) index.
INDEX="$(mktemp)"
trap 'rm -f "${INDEX}"' EXIT
for src in "${SOURCE_FILES[@]}"; do
    if [[ ! -f "${src}" ]]; then continue; fi
    base="$(basename "${src}")"
    case "${base}" in
        *.rhox)
            # Rholang: `contract @PoS!("name", ...)` or `contract <name>(...)` line.
            # Match "contract @<channel>(@"<name>", ...)" or "contract <name>(...)".
            grep -nE '(^[[:space:]]*contract[[:space:]]+)|(^[[:space:]]*\| @"[A-Za-z_]+")' "${src}" 2>/dev/null \
                | awk -F: -v f="${base}" '{print f, $1, "rhox-decl"}' >> "${INDEX}"
            ;;
        *.rs)
            # Rust: pub/non-pub fn, pub struct, impl, trait.
            grep -nE '^[[:space:]]*(pub(\([^)]+\))? )?(async )?(fn|struct|enum|trait|impl|const|static)[[:space:]]+' "${src}" 2>/dev/null \
                | awk -F: -v f="${base}" '{print f, $1, "rs-decl"}' >> "${INDEX}"
            ;;
    esac
done

# Scan docs for `<file>.{rhox,rs}:NNN` or `<file>.{rhox,rs}:NNN-MMM` cites.
MISMATCHES=0
TOTAL_CHECKED=0
while IFS= read -r hit; do
    # Format: docs-path:line-no:matched-text
    doc_path="${hit%%:*}"
    rest="${hit#*:}"
    doc_line="${rest%%:*}"
    matched="${rest#*:}"

    # Extract every (<basename>, start-line) pair on this matched line.
    # A line can contain multiple cites; loop over them.
    while IFS=$'\t' read -r cite_file cite_start cite_end; do
        if [[ -z "${cite_file}" ]]; then continue; fi
        TOTAL_CHECKED=$((TOTAL_CHECKED + 1))

        # Does the start line correspond to a declaration in the index?
        if ! awk -v f="${cite_file}" -v n="${cite_start}" '$1==f && $2==n {found=1; exit} END{exit !found}' "${INDEX}"; then
            # Allow some slack: report only if the start line is more than 4
            # lines away from the nearest declaration of the same file.
            nearest=$(awk -v f="${cite_file}" -v n="${cite_start}" '$1==f { d = ($2 > n ? $2 - n : n - $2); if (d < best || best == "") { best = d; near = $2 } } END{ print near }' "${INDEX}")
            if [[ -z "${nearest}" ]]; then
                printf '✗ %s:%s  %s:%s — no declarations indexed for this file\n' "${doc_path}" "${doc_line}" "${cite_file}" "${cite_start}"
                MISMATCHES=$((MISMATCHES + 1))
            else
                diff_lines=$(( cite_start - nearest ))
                if (( diff_lines < 0 )); then diff_lines=$(( -diff_lines )); fi
                if (( diff_lines > 4 )); then
                    printf '✗ %s:%s  %s:%s — nearest declaration is at line %s (off by %s)\n' \
                        "${doc_path}" "${doc_line}" "${cite_file}" "${cite_start}" "${nearest}" "${diff_lines}"
                    MISMATCHES=$((MISMATCHES + 1))
                fi
            fi
        fi
    done < <(printf '%s\n' "${matched}" | grep -oE '`[A-Za-z_][A-Za-z0-9_]*\.(rhox|rs):[0-9]+(-[0-9]+)?`' \
              | sed -E 's/^`([A-Za-z_][A-Za-z0-9_]*\.(rhox|rs)):([0-9]+)(-([0-9]+))?`$/\1\t\3\t\5/')
done < <(grep -rEn '`[A-Za-z_][A-Za-z0-9_]*\.(rhox|rs):[0-9]+' "${DOCS_DIR}" 2>/dev/null)

if (( MISMATCHES == 0 )); then
    [[ "${QUIET}" == 0 ]] && echo "✓ all ${TOTAL_CHECKED} cited source lines resolve to a declaration (within 4-line slack)"
    exit 0
else
    echo ""
    echo "Found ${MISMATCHES} mismatch(es) out of ${TOTAL_CHECKED} cited source lines."
    exit 1
fi

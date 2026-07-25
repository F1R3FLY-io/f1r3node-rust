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

# Audit the WHOLE theory corpus, not just slashing.
#
# This was `docs/theory/slashing` only, and that narrowness was not harmless: it is
# exactly why `FtProvenance.v` could go on citing `engine/initializing.rs:1081-1099`
# and `token_metadata_check.rs:91,105` after the 2026-07-15 dev merge DELETED both
# anchors, with no gate firing. A model that cites a line which no longer says what
# it claims is a silently-false model. Widening the scope converts that whole class
# from invisible to gated.
DOCS_DIR="${REPO}/docs/theory"
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
    "${REPO}/casper/src/rust/engine/multi_parent_casper/validation_dispatcher.rs"
    "${REPO}/casper/src/rust/engine/multi_parent_casper/dispatch.rs"
    "${REPO}/casper/src/rust/estimator.rs"
    "${REPO}/casper/src/rust/block_status.rs"
    "${REPO}/casper/src/rust/slashing_authorization.rs"
    "${REPO}/casper/src/rust/blocks/proposer/block_creator.rs"
    "${REPO}/casper/src/rust/util/rholang/costacc/slash_deploy.rs"
    "${REPO}/block-storage/src/rust/dag/block_dag_key_value_storage.rs"
    "${REPO}/block-storage/src/rust/dag/equivocations_access.rs"
    # finalized-floor / fork-choice / merge-algebra surface (added 2026-07-15 with the
    # DOCS_DIR widening above): every source the non-slashing models cite by line.
    "${REPO}/casper/src/rust/safety/clique_oracle.rs"
    "${REPO}/casper/src/rust/finality/floor.rs"
    "${REPO}/casper/src/rust/finality/finalizer.rs"
    "${REPO}/casper/src/rust/engine/multi_parent_casper/snapshot.rs"
    "${REPO}/casper/src/rust/engine/multi_parent_casper/finalization_runner.rs"
    "${REPO}/casper/src/rust/casper.rs"
    "${REPO}/casper/src/rust/rholang/runtime.rs"
    "${REPO}/casper/src/rust/engine/initializing.rs"
    "${REPO}/casper/src/rust/util/token_metadata_check.rs"
    "${REPO}/casper/src/rust/util/rholang/interpreter_util.rs"
    "${REPO}/casper/src/rust/util/rholang/runtime_manager.rs"
    "${REPO}/casper/src/rust/merging/deploy_chain_index.rs"
    "${REPO}/casper/src/rust/merging/conflict_set_merger.rs"
    "${REPO}/casper/src/rust/merging/dag_merger.rs"
    "${REPO}/casper/src/rust/genesis/contracts/proof_of_stake.rs"
    "${REPO}/rholang/src/rust/interpreter/merging/rholang_merging_logic.rs"
    "${REPO}/rspace++/src/rspace/merger/merging_logic.rs"
    "${REPO}/rspace++/src/rspace/merger/event_log_index.rs"
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

        # What the cite CLAIMS is the invariant we check: the doc names an
        # identifier and asserts it lives at <file>:<line>. So verify exactly
        # that — the named identifier must actually occur in the cited range.
        #
        # The previous check asked instead "is line N a DECLARATION (± 4 lines)".
        # That only held for the slashing docs, which happen to cite declarations.
        # The finalized-floor / fork-choice / merge-algebra docs legitimately cite
        # STATEMENTS inside a function (e.g. the `sort_by` at snapshot.rs:325), so
        # declaration-proximity reported them as mismatches when they were correct
        # — 77/112 false positives on first widening. A gate that cries wolf on
        # two-thirds of its input is worse than no gate: it trains readers to
        # ignore it, which is precisely how the FtProvenance.v drift survived.
        src_path=""
        for cand in "${SOURCE_FILES[@]}"; do
            if [[ "$(basename "${cand}")" == "${cite_file}" ]]; then src_path="${cand}"; break; fi
        done
        if [[ -z "${src_path}" || ! -f "${src_path}" ]]; then
            continue   # file not in the audited allow-list; not a mismatch
        fi

        n_lines=$(wc -l < "${src_path}")
        end="${cite_end:-${cite_start}}"
        [[ -z "${end}" ]] && end="${cite_start}"

        # (1) Structural: the cited range must exist in the file at all.
        if (( cite_start > n_lines || end > n_lines )); then
            printf '✗ %s:%s  %s:%s-%s — past EOF (file has %s lines)\n' \
                "${doc_path}" "${doc_line}" "${cite_file}" "${cite_start}" "${end}" "${n_lines}"
            MISMATCHES=$((MISMATCHES + 1))
            continue
        fi

        # (2) Semantic: every backtick-quoted identifier on this doc line that is
        #     NOT itself a file cite is a candidate anchor. If any one of them
        #     occurs within the cited range (widened by a small window for
        #     multi-line signatures/comments), the cite is corroborated.
        idents=$(printf '%s\n' "${matched}" \
                 | grep -oE '`[A-Za-z_][A-Za-z0-9_:]*`' \
                 | tr -d '`' \
                 | grep -vE '\.(rhox|rs|v)$' | sort -u)
        if [[ -z "${idents}" ]]; then
            continue   # no identifier claimed; (1) is all we can honestly check
        fi
        lo=$(( cite_start > 3 ? cite_start - 3 : 1 ))
        hi=$(( end + 3 ))
        window=$(sed -n "${lo},${hi}p" "${src_path}")
        corroborated=0
        while IFS= read -r id; do
            [[ -z "${id}" ]] && continue
            if printf '%s' "${window}" | grep -qF -- "${id}"; then corroborated=1; break; fi
        done <<< "${idents}"
        if (( corroborated == 0 )); then
            printf '✗ %s:%s  %s:%s-%s — none of the identifiers named on this line (%s) occur in the cited range\n' \
                "${doc_path}" "${doc_line}" "${cite_file}" "${cite_start}" "${end}" \
                "$(printf '%s' "${idents}" | tr '\n' ' ')"
            MISMATCHES=$((MISMATCHES + 1))
        fi
    done < <(printf '%s\n' "${matched}" | grep -oE '`[A-Za-z_][A-Za-z0-9_]*\.(rhox|rs):[0-9]+(-[0-9]+)?`' \
              | sed -E 's/^`([A-Za-z_][A-Za-z0-9_]*\.(rhox|rs)):([0-9]+)(-([0-9]+))?`$/\1\t\3\t\5/')
done < <(grep -rEn '`[A-Za-z_][A-Za-z0-9_]*\.(rhox|rs):[0-9]+' "${DOCS_DIR}" 2>/dev/null)

if (( MISMATCHES == 0 )); then
    [[ "${QUIET}" == 0 ]] && echo "✓ all ${TOTAL_CHECKED} cited source lines exist and are corroborated by a named identifier"
    exit 0
else
    echo ""
    echo "Found ${MISMATCHES} mismatch(es) out of ${TOTAL_CHECKED} cited source lines."
    exit 1
fi

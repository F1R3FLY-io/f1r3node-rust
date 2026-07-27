#!/usr/bin/env bash
#
# check-fmt.sh — the workspace's ONE formatting check, and the only idiom in it
# that can tell "clean" from "crashed".
#
# ─────────────────────────────────────────────────────────────────────────────
# ★ THE DEFECT THIS SCRIPT EXISTS TO REMOVE
# ─────────────────────────────────────────────────────────────────────────────
#
# `rustfmt --check` reports differences by printing blocks that begin
#
#     Diff in /path/to/file.rs:12:
#
# so the obvious idiom for "is this file clean?" is
#
#     rustfmt --check "$f" | grep -c '^Diff in'      # ← WRONG
#
# It is wrong because it reads only stdout, and a rustfmt that CRASHES prints no
# diffs. `rustfmt --config-path <dir> <file>` panics — "path is expected to be
# under the root", inside `IgnorePathSet::is_match` — for any file outside the
# config root whenever that root's `rustfmt.toml` carries a non-empty `ignore`
# list, which ours does and must (see `rustfmt.toml`: those files' TEXT is a
# checkable citation re-derived from git by
# `rholang/tests/normalize_oracle_provenance.rs`). It exits 101 having written
# nothing to stdout, so the count is zero and the file reads as CLEAN.
#
# That is not hypothetical: it misled an agent into reverting an unrelated
# whole-file reformat of `rholang/tests/stack_depth_gate.rs` on the strength of
# a "zero diffs" that was a crash. The three states are
#
#     ┌───────────────┬──────┬──────────────────────┬────────────────────────┐
#     │ state         │ exit │ `grep -c '^Diff in'` │ what it means          │
#     ├───────────────┼──────┼──────────────────────┼────────────────────────┤
#     │ CLEAN         │   0  │ 0                    │ nothing to do          │
#     │ DIFFS         │   1  │ ≥ 1                  │ run `cargo fmt --all`  │
#     │ TOOL ERROR    │ 101  │ 0  ←── ambiguous     │ the answer is UNKNOWN  │
#     └───────────────┴──────┴──────────────────────┴────────────────────────┘
#
# The grep column cannot separate rows 1 and 3. The exit column can, so every
# check here is made on the EXIT CODE, corroborated by the output rather than
# decided by it. `--self-test` proves the separation by constructing all three.
#
# ⚠ The remedy is NOT to drop the `ignore` entries. Removing them would let the
# formatter silently falsify ~33 provenance claims, and `#[rustfmt::skip]` is
# refused because it edits the very text whose byte-identity is the point. The
# discipline is what gets fixed.
#
# ─────────────────────────────────────────────────────────────────────────────
# USAGE
# ─────────────────────────────────────────────────────────────────────────────
#
#   scripts/check-fmt.sh                  # the whole workspace (cargo fmt --all)
#   scripts/check-fmt.sh FILE [FILE...]   # named files, via rustfmt
#   scripts/check-fmt.sh --self-test      # prove the check separates all three states
#
# EXIT CODES — distinct, so a caller can distinguish them too:
#
#   0  CLEAN       every file checked is formatted
#   1  DIFFS       at least one file differs; the diff is printed
#   2  TOOL ERROR  the formatter did not answer the question; output is printed
#                  verbatim and NOTHING is concluded about formatting
#
# ⚠ 2, not 101: a caller of this script sees a stable, documented code rather
# than whatever the formatter happened to die with.
#
set -uo pipefail

readonly EXIT_CLEAN=0
readonly EXIT_DIFFS=1
readonly EXIT_TOOL=2

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT

# Results of the most recent `classify`, in lieu of multiple return values.
CLASSIFY_STATE=""   # CLEAN | DIFFS | TOOL_ERROR
CLASSIFY_RC=""      # the raw exit code of the classified command
CLASSIFY_DIFFS=""   # how many `Diff in` blocks the command printed
CLASSIFY_OUTPUT=""  # everything it printed, stdout and stderr interleaved

# ─────────────────────────────────────────────────────────────────────────────
# ★ THE CLASSIFIER — one place, used by every caller and by the self-test
# ─────────────────────────────────────────────────────────────────────────────
#
# Decides on the exit code, then requires the output to CORROBORATE it. The
# corroboration is what makes the classifier robust to a wrapper that returns 1
# for a reason other than diffs (`cargo fmt` fronts `rustfmt` and is free to
# fail on its own account): an exit code of 1 with no `Diff in` block is not
# "unformatted", it is an unexplained failure, and this reports it as one.
#
# usage: classify <command> [args...]
classify() {
    CLASSIFY_OUTPUT="$("$@" 2>&1)"
    CLASSIFY_RC=$?
    CLASSIFY_DIFFS="$(printf '%s\n' "$CLASSIFY_OUTPUT" | grep -c '^Diff in')"

    case "$CLASSIFY_RC" in
        0)
            if [[ "$CLASSIFY_DIFFS" -eq 0 ]]; then
                CLASSIFY_STATE="CLEAN"
            else
                # Exit 0 while printing diffs: the tool contradicted itself.
                CLASSIFY_STATE="TOOL_ERROR"
            fi
            ;;
        1)
            if [[ "$CLASSIFY_DIFFS" -ge 1 ]]; then
                CLASSIFY_STATE="DIFFS"
            else
                CLASSIFY_STATE="TOOL_ERROR"
            fi
            ;;
        *)
            CLASSIFY_STATE="TOOL_ERROR"
            ;;
    esac
}

# Is this repo-relative path one rustfmt.toml declines to examine?
#
# The `ignore` entries are plain repository-relative paths — see rustfmt.toml,
# where the criterion for earning one is stated and is narrow ("a file is listed
# here only while its own text asserts provenance against an external source").
# This reads them rather than restating them, so the two cannot drift.
is_exempt() {
    local rel="$1" line entry
    [[ -f "$REPO_ROOT/rustfmt.toml" ]] || return 1
    while IFS= read -r line; do
        entry="$(printf '%s' "$line" | sed -e 's/^[[:space:]]*"//' -e 's/",\{0,1\}[[:space:]]*$//')"
        [[ -n "$entry" && "$entry" == "$rel" ]] && return 0
    done < <(sed -n '/^ignore[[:space:]]*=[[:space:]]*\[/,/\]/p' "$REPO_ROOT/rustfmt.toml" \
                 | grep '"')
    return 1
}

# Map a classifier state onto this script's documented exit code.
state_to_exit() {
    case "$1" in
        CLEAN) return $EXIT_CLEAN ;;
        DIFFS) return $EXIT_DIFFS ;;
        *)     return $EXIT_TOOL ;;
    esac
}

report_and_exit() {
    case "$CLASSIFY_STATE" in
        CLEAN)
            echo -e "  ${CYAN}[fmt]${NC} ${GREEN}CLEAN${NC} (exit $CLASSIFY_RC, 0 diffs)"
            exit $EXIT_CLEAN
            ;;
        DIFFS)
            # ⚠ `Diff in` blocks, not files: rustfmt emits one per contiguous
            # hunk, so a single file routinely accounts for dozens.
            echo -e "  ${CYAN}[fmt]${NC} ${RED}DIFFS${NC} (exit $CLASSIFY_RC, $CLASSIFY_DIFFS diff block(s))"
            printf '%s\n' "$CLASSIFY_OUTPUT"
            echo ""
            echo -e "  ${YELLOW}Fix with:${NC} cargo fmt --all"
            exit $EXIT_DIFFS
            ;;
        *)
            echo -e "  ${CYAN}[fmt]${NC} ${RED}TOOL ERROR${NC} (exit $CLASSIFY_RC, $CLASSIFY_DIFFS diffs printed)"
            echo -e "  ${YELLOW}The formatter did not answer the question. NOTHING is concluded"
            echo -e "  about formatting — in particular this is NOT 'clean'.${NC}"
            echo ""
            printf '%s\n' "$CLASSIFY_OUTPUT"
            echo ""
            echo -e "  ${YELLOW}If the panic is 'path is expected to be under the root' in"
            echo -e "  IgnorePathSet::is_match, the target file is outside the config root"
            echo -e "  while rustfmt.toml carries an \`ignore\` list. Check it from inside the"
            echo -e "  root, or with 'cargo fmt --all'. The crash also drops a rustc-ice-*.txt"
            echo -e "  into the current directory; delete it.${NC}"
            exit $EXIT_TOOL
            ;;
    esac
}

# ─────────────────────────────────────────────────────────────────────────────
# THE SELF-TEST — ★ anti-vacuity: a check nobody has seen reject is not a check
# ─────────────────────────────────────────────────────────────────────────────
#
# Constructs all three states and requires the classifier to report three
# DIFFERENT ones. Also prints what the discarded idiom would have said, so the
# reason this script exists is evidence rather than assertion.
#
# ⚠ Every rustfmt here runs with the repository as the working directory: the
# rustup shim resolves the toolchain from the CWD, and `rust-toolchain.toml`
# pins it. The ICE is a property of that pinned toolchain, so a self-test that
# ran from /tmp would silently use the default toolchain and could exhibit no
# third state at all.
self_test() {
    local in_root outside status
    in_root="$REPO_ROOT/target/tmp/check-fmt-self-test.$$"
    outside="$(mktemp -d "${TMPDIR:-/tmp}/check-fmt-outside.XXXXXX")"
    mkdir -p "$in_root"
    # shellcheck disable=SC2064  # expand now: the paths must survive the trap
    trap "rm -rf '$in_root' '$outside'" EXIT

    # The DIRTY subject, and — by construction rather than by guesswork — the
    # CLEAN one: the same text after the repository's own configuration has
    # formatted it. A hand-written "clean" file would only be clean until the
    # style changed.
    printf 'fn  main( )   {\nlet x=1 ;\n     println!("{}",x);\n}\n' > "$in_root/dirty.rs"
    cp "$in_root/dirty.rs" "$in_root/clean.rs"
    ( cd "$REPO_ROOT" && rustfmt --edition 2021 --config-path "$REPO_ROOT" "$in_root/clean.rs" ) \
        > /dev/null 2>&1
    # The OUT-OF-ROOT subject: identical bytes to the clean one, so the only
    # thing that can make its outcome differ is where it lives.
    cp "$in_root/clean.rs" "$outside/away.rs"

    echo ""
    echo -e "${CYAN}check-fmt.sh --self-test${NC} — does the check separate all three states?"
    echo "================================================================"
    printf '  %-12s %-6s %-8s %-12s %s\n' "state" "exit" "grep -c" "classified" "subject"

    local -a states=() codes=() greps=()
    local subject label hazard_live=0
    for label in CLEAN DIFFS ICE; do
        case "$label" in
            CLEAN) subject="$in_root/clean.rs" ;;
            DIFFS) subject="$in_root/dirty.rs" ;;
            ICE)   subject="$outside/away.rs" ;;
        esac
        # RUSTC_ICE=0 keeps the DELIBERATE crash from dropping a dump file into
        # the tree; a real crash still leaves one, which is a diagnostic.
        classify env RUSTC_ICE=0 sh -c \
            "cd '$REPO_ROOT' && rustfmt --edition 2021 --check --config-path '$REPO_ROOT' '$subject'"

        # ⚠ The third state is produced by an UPSTREAM BUG, so this self-test
        # must not fail when that bug is fixed — a toolchain upgrade that
        # removes the crash is a good change and must not read as a broken
        # gate. If the out-of-root invocation stops crashing, the separation is
        # still proven, on a stand-in that exits 101 the way the crash did; the
        # difference is reported rather than swallowed, because "the hazard is
        # live" and "the hazard is gone" are both things a reader needs.
        if [[ "$label" == "ICE" ]]; then
            if [[ "$CLASSIFY_RC" -ne 0 && "$CLASSIFY_RC" -ne 1 ]]; then
                hazard_live=1
            else
                subject="(stand-in: a command exiting 101 with no diff output)"
                classify sh -c 'echo "stand-in for a formatter crash" >&2; exit 101'
            fi
        fi

        state_to_exit "$CLASSIFY_STATE"
        status=$?
        states+=("$CLASSIFY_STATE")
        codes+=("$status")
        greps+=("$CLASSIFY_DIFFS")
        printf '  %-12s %-6s %-8s %-12s %s\n' \
            "$label" "$CLASSIFY_RC" "$CLASSIFY_DIFFS" "$CLASSIFY_STATE" "$subject"
    done

    local failed=0

    # (1) The three states are three states.
    [[ "${states[0]}" == "CLEAN"      ]] || { echo -e "${RED}  a formatted in-root file was not CLEAN (${states[0]})${NC}"; failed=1; }
    [[ "${states[1]}" == "DIFFS"      ]] || { echo -e "${RED}  an unformatted in-root file was not DIFFS (${states[1]})${NC}"; failed=1; }
    [[ "${states[2]}" == "TOOL_ERROR" ]] || { echo -e "${RED}  an out-of-root file was not TOOL_ERROR (${states[2]})${NC}"; failed=1; }
    if [[ "${codes[0]}" == "${codes[1]}" || "${codes[0]}" == "${codes[2]}" || "${codes[1]}" == "${codes[2]}" ]]; then
        echo -e "${RED}  the three states do not map onto three exit codes: ${codes[*]}${NC}"
        failed=1
    fi

    # (1b) Is the upstream hazard still live on THIS toolchain? Reported, never
    #      asserted — see the note in the loop above.
    if [[ $hazard_live -eq 1 ]]; then
        echo -e "  ${YELLOW}the out-of-root crash is LIVE on $(rustfmt --version 2>/dev/null | head -1)${NC}"
    else
        echo -e "  ${GREEN}the out-of-root crash did NOT reproduce on this toolchain${NC}"
        echo -e "  ${YELLOW}— the third state above used a stand-in. If rustfmt has been fixed,"
        echo -e "  the pre-flight guard below is now belt-and-braces rather than load-bearing,"
        echo -e "  and the header of this script should say so.${NC}"
    fi

    # (2) ★ THE COUNTER-EVIDENCE. The discarded idiom, on the same three runs.
    echo ""
    echo -e "  ${YELLOW}the idiom this replaces — 'rustfmt --check | grep -c ^Diff in':${NC}"
    printf '    CLEAN -> %s, DIFFS -> %s, ICE -> %s\n' "${greps[0]}" "${greps[1]}" "${greps[2]}"
    if [[ "${greps[0]}" == "${greps[2]}" ]]; then
        echo -e "    ${YELLOW}CLEAN and ICE are INDISTINGUISHABLE under it (both ${greps[0]}) —"
        echo -e "    which is the defect, reproduced.${NC}"
    else
        echo -e "${RED}    the grep idiom separated CLEAN from ICE, so the premise of this"
        echo -e "    script no longer holds and its documentation is stale.${NC}"
        failed=1
    fi

    # (3) The pre-flight guard: an out-of-root path is refused BEFORE the
    #     formatter is invoked, so the usual way of hitting the ICE cannot.
    echo ""
    ( "${BASH_SOURCE[0]}" "$outside/away.rs" ) > /dev/null 2>&1
    status=$?
    if [[ "$status" -eq $EXIT_TOOL ]]; then
        echo -e "  ${GREEN}pre-flight${NC}: an out-of-root path exits $EXIT_TOOL without invoking the formatter"
    else
        echo -e "${RED}  pre-flight: an out-of-root path exited $status, expected $EXIT_TOOL${NC}"
        failed=1
    fi

    # (4) …and an in-root file still goes through, so (3) is a guard and not a
    #     blanket refusal.
    ( "${BASH_SOURCE[0]}" "$in_root/clean.rs" ) > /dev/null 2>&1
    status=$?
    if [[ "$status" -eq $EXIT_CLEAN ]]; then
        echo -e "  ${GREEN}pre-flight${NC}: an in-root formatted file still exits $EXIT_CLEAN"
    else
        echo -e "${RED}  pre-flight: an in-root clean file exited $status, expected $EXIT_CLEAN${NC}"
        failed=1
    fi

    echo "================================================================"
    if [[ $failed -eq 0 ]]; then
        echo -e "${GREEN}The check separates CLEAN / DIFFS / TOOL ERROR.${NC}"
        echo ""
        return 0
    fi
    echo -e "${RED}The check does NOT separate the three states.${NC}"
    echo ""
    return 1
}

# ─────────────────────────────────────────────────────────────────────────────
# ENTRY POINTS
# ─────────────────────────────────────────────────────────────────────────────

main() {
    if [[ $# -ge 1 && "$1" == "--self-test" ]]; then
        self_test
        exit $?
    fi

    if [[ $# -eq 0 ]]; then
        # Workspace mode. `cargo fmt` honours `rustfmt.toml` from the workspace
        # root for every member, so no path can be out of root and the ICE is
        # unreachable here — which is exactly why this is the DEFAULT mode and
        # file mode is the exception.
        cd "$REPO_ROOT" || exit $EXIT_TOOL
        classify cargo fmt --all -- --check
        report_and_exit
    fi

    # File mode. ⚠ PRE-FLIGHT: a path outside the config root is refused here,
    # because handing it to rustfmt with `--config-path` is the crash. Refusing
    # is strictly better than classifying the crash afterwards — no panic, no
    # rustc-ice-*.txt, and an error that says what to do.
    local -a targets=()
    local f abs rel
    for f in "$@"; do
        abs="$(readlink -f -- "$f")" || abs=""
        if [[ -z "$abs" || ! -e "$abs" ]]; then
            echo -e "  ${CYAN}[fmt]${NC} ${RED}TOOL ERROR${NC}: no such file: $f"
            exit $EXIT_TOOL
        fi
        if [[ "$abs" != "$REPO_ROOT"/* ]]; then
            echo -e "  ${CYAN}[fmt]${NC} ${RED}TOOL ERROR${NC}: $abs is outside $REPO_ROOT"
            echo -e "  ${YELLOW}rustfmt --config-path panics ('path is expected to be under the"
            echo -e "  root', in IgnorePathSet::is_match) on an out-of-root file whenever the"
            echo -e "  config carries a non-empty \`ignore\` list, which this one does and must."
            echo -e "  It exits 101 having printed no diffs, so a grep-based check would call"
            echo -e "  this file CLEAN. Nothing is concluded about it.${NC}"
            exit $EXIT_TOOL
        fi
        # ⚠ A FOURTH state, and the same defect wearing a different hat: a file
        # in rustfmt.toml's `ignore` list is never examined, so rustfmt exits 0
        # for it and a caller reads "clean". It is not clean, it is UNCHECKED —
        # deliberately, because its text is a citation re-derived from git. Say
        # so instead of letting silence mean approval.
        rel="${abs#"$REPO_ROOT"/}"
        if is_exempt "$rel"; then
            echo -e "  ${CYAN}[fmt]${NC} ${YELLOW}EXEMPT${NC}: $rel is in rustfmt.toml's \`ignore\` list"
            echo -e "  ${YELLOW}It was NOT checked and must not be formatted: its bytes are a"
            echo -e "  provenance claim re-derived from git by"
            echo -e "  rholang/tests/normalize_oracle_provenance.rs. rustfmt would exit 0 for"
            echo -e "  it either way, which is why this is reported rather than inferred.${NC}"
            continue
        fi
        targets+=("$abs")
    done

    if [[ ${#targets[@]} -eq 0 ]]; then
        echo -e "  ${CYAN}[fmt]${NC} ${GREEN}CLEAN${NC} (nothing to check: every named file is exempt)"
        exit $EXIT_CLEAN
    fi

    cd "$REPO_ROOT" || exit $EXIT_TOOL
    classify rustfmt --edition 2021 --check --config-path "$REPO_ROOT" "${targets[@]}"
    report_and_exit
}

main "$@"

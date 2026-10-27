#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
output="${1:?An output file is required}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
original="$(cat "$ROOT/scripts/run-merge-recovery-soak.sh")"
: >"$TMP/results"
for name in clean missing-owner-stop missing-residual-stop ignored-stop-failure overwritten-reason; do
    case "$name" in
        clean) assertion="" ;;
        missing-owner-stop)
            old='stop_node_writers_within "$WRITER_STOP_BUDGET" -q kill >"$OUTPUT_DIR/.writer-stop.log" 2>&1 || WRITER_STOP_STATUS=$?'
            new=':'; assertion='FAIL: owned writer survived' ;;
        missing-residual-stop)
            old='if [ "$CASPER_RUNTIME" -eq 0 ] && [ -d "$SUBPROCESS_DATA_DIR" ]; then'
            new='if false; then'; assertion='FAIL: residual writer survived' ;;
        ignored-stop-failure)
            old=$'if [ "$FAILURES" -eq 0 ]; then\n\t\tFAILURES=1\n\tfi'
            new=$'if false; then\n\t\tFAILURES=1\n\tfi'; assertion='FAIL: unconfirmed stop passed' ;;
        overwritten-reason)
            old='${EARLY_EXIT_REASON:-${PRIOR_EARLY_EXIT_REASON:-none}}'
            new='${EARLY_EXIT_REASON:-none}'; assertion='FAIL: earlier reason overwritten' ;;
    esac
    driver="$ROOT/scripts/run-merge-recovery-soak.sh"
    if [ "$name" != clean ]; then
        [[ "$original" == *"$old"* ]] || { printf 'Mutation anchor changed: %s\n' "$name" >&2; exit 1; }
        mkdir -p "$TMP/$name/scripts"
        ln -s "$ROOT/scripts/bench" "$TMP/$name/scripts/bench"
        driver="$TMP/$name/scripts/run-merge-recovery-soak.sh"
        printf '%s\n' "${original/"$old"/"$new"}" >"$driver"
        chmod +x "$driver"
    fi
    status=0
    SOAK_CLEANUP_DRIVER="$driver" timeout --signal=TERM --kill-after=2 100 \
        bash "$ROOT/scripts/bench/test-soak-segment-cleanup.sh" >"$TMP/$name.log" 2>&1 || status=$?
    if [ "$status" -eq 124 ] || [ "$status" -eq 137 ]; then
        cat "$TMP/$name.log"
        printf 'A regression control exceeded its deadline: %s\n' "$name" >&2
        exit 1
    fi
    if { [ "$name" = clean ] && [ "$status" -ne 0 ]; } || \
        { [ "$name" != clean ] && { [ "$status" -eq 0 ] || ! grep -Fq "$assertion" "$TMP/$name.log"; }; }; then
        cat "$TMP/$name.log"
        printf 'Unexpected fixture verdict: %s\n' "$name" >&2
        exit 1
    fi
    jq -nc --arg name "$name" --arg assertion "$assertion" --argjson status "$status" \
        '{case:$name,exit:$status,assertion:$assertion}' >>"$TMP/results"
done
mkdir -p "$(dirname "$output")"
jq -s '{schema:"soak-cleanup-regressions/v1",results:.}' "$TMP/results" >"$output"
printf 'Two-segment fixture and four negative controls passed.\n'

#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
output="${1:?An output file is required}"
jar="${TLA2TOOLS_JAR:?TLA2TOOLS_JAR is required}"
[ "$(shasum -a 256 "$jar" | awk '{print $1}')" = 936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88 ] || exit 4
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
: >"$TMP/results"
for name in clean SkipOwnerStop SkipResidualStop IgnoreStopFailure EraseReason; do
    expected=12
    case "$name" in
        clean) expected=0; invariant="" ;;
        SkipOwnerStop|SkipResidualStop) invariant=NoSuccessfulLeak ;;
        IgnoreStopFailure) invariant=StopFailurePersists ;;
        EraseReason) invariant=ReasonPersists ;;
    esac
    status=0
    java -Xmx512m -XX:+UseParallelGC -cp "$jar" tlc2.TLC -workers 1 -metadir "$TMP/$name" \
        -config "$ROOT/formal/tlaplus/soak_cleanup/$name.cfg" \
        "$ROOT/formal/tlaplus/soak_cleanup/SoakCleanup.tla" >"$TMP/$name.log" 2>&1 || status=$?
    if [ "$status" -ne "$expected" ] || { [ -n "$invariant" ] && ! grep -q "Invariant $invariant is violated" "$TMP/$name.log"; }; then
        cat "$TMP/$name.log"
        printf 'Unexpected model verdict: %s\n' "$name" >&2
        exit 1
    fi
    jq -nc --arg name "$name" --arg invariant "$invariant" --argjson status "$status" \
        '{case:$name,exit:$status,invariant:$invariant}' >>"$TMP/results"
done
mkdir -p "$(dirname "$output")"
jq -s '{schema:"soak-cleanup-model/v1",results:.}' "$TMP/results" >"$output"
printf 'Cleanup model and four negative controls passed.\n'

#!/usr/bin/env bash
set -euo pipefail
HELPER="${1:?The shared completion helper is required.}"
OUT="${2:?An unused output directory is required.}"
[[ -f "$HELPER" && ! -L "$HELPER" && ! -e "$OUT" ]]
[[ "$(shasum -a 256 "$HELPER" | awk '{print $1}')" == 924a1cde6ae61d15a72dda276687b0d63d9df0dd75d7b7f1aba4e8300f502d2e ]]
mkdir -p "$OUT"
for id in 002 003 004; do
 target/debug/check-casper-claims --root . --output "$OUT/claim-$id.json" --strict --claim "CLAIM-CASPER-SOAK-$id" >"$OUT/claim-$id.txt" 2>&1
done
cp docs/ToDos.md "$OUT/ToDos.md"
cp docs/ToDos.md "$OUT/before.md"
DIRECTORY="$(cd "$(dirname "$HELPER")" && pwd)"
(cd "$DIRECTORY" && shasum -a 256 task-complete.sh lib/{integrity-walker,epic-parser,user-flow-parser,story-parser}.sh) >"$OUT/helper-inputs.sha256"
ln -s "$DIRECTORY/lib" "$OUT/lib"
awk 'BEGIN { n=0 } $0 == "main \"$@\"" { n++; next } { print } END { if (n != 1) exit 2 }' "$HELPER" >"$OUT/helper.sh"
export TODOS_FILE="$OUT/ToDos.md" USER_FLOWS_FILE=docs/User-Flows.md USER_STORIES_FILE=docs/UserStories.md BF_TODO_PARSER_STRICT=0
source "$OUT/helper.sh"
epics="$(parse_epics "$TODOS_FILE")"
printf '%s\n' "$epics" >"$OUT/epics-before.json"
for item in 5:authority_finality 6:publication 7:recovery; do
 task="TASK-017-${item%%:*}"
 profile="${item#*:}"
 jq -e --arg task "$task" '[.[]|.tasks[]?|select(.id==$task)]|length==1 and .[0].status=="in_progress"' "$OUT/epics-before.json" >/dev/null
 report="$(validate_task "$task")"
 printf '%s\n' "$report" >"$OUT/$task-before.json"
 mark_task_complete "$task" "scripts/casper-soak/tests/$profile.rs" true false false >"$OUT/$task-complete.txt" 2>&1
 report="$(validate_task "$task")"
 printf '%s\n' "$report" >"$OUT/$task-after.json"
 jq -e '.grade=="full" and (.gaps|length)==0' "$OUT/$task-after.json" >/dev/null
done
epics="$(parse_epics "$TODOS_FILE")"
printf '%s\n' "$epics" >"$OUT/epics-after.json"
for phase in before after; do
 jq '[.[]|.tasks[]?|select(.id!="TASK-017-5" and .id!="TASK-017-6" and .id!="TASK-017-7")]' "$OUT/epics-$phase.json" >"$OUT/unrelated-$phase.json"
done
cmp "$OUT/unrelated-before.json" "$OUT/unrelated-after.json"
status=0
diff -u --label a/docs/ToDos.md --label b/docs/ToDos.md "$OUT/before.md" "$OUT/ToDos.md" >"$OUT/completion.patch.txt" || status=$?
[[ "$status" == 1 ]]
printf 'Strict completion succeeded on the tracker copy. The real tracker and index were not changed.\n'

#!/usr/bin/env bash
set -euo pipefail
HELPER="${1:?The shared completion helper is required.}"
OUT="${2:?An unused output directory is required.}"
[[ -f "$HELPER" && ! -L "$HELPER" && ! -e "$OUT" ]]
[[ "$(shasum -a 256 "$HELPER" | awk '{print $1}')" == 924a1cde6ae61d15a72dda276687b0d63d9df0dd75d7b7f1aba4e8300f502d2e ]]
mkdir -p "$OUT"
RUN=docs/casper/cbc-evidence/runs/casper-preparation-completion-20260918-01
bash "$RUN/verify.sh" . full >"$OUT/verification.txt" 2>&1
bash "$RUN/test-verifier.sh" "$OUT/fixtures" >"$OUT/checker-tests.txt" 2>&1
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
for task in TASK-017-1 TASK-017-2 TASK-017-3; do
 jq -e --arg task "$task" '[.[]|.tasks[]?|select(.id==$task)]|length==1 and .[0].status=="in_progress"' "$OUT/epics-before.json" >/dev/null
 report="$(validate_task "$task")"
 printf '%s\n' "$report" >"$OUT/$task-before.json"
 jq -e '.grade=="full" and (.gaps|length)==0' "$OUT/$task-before.json" >/dev/null
 mark_task_complete "$task" "" true false false >"$OUT/$task-complete.txt" 2>&1
 report="$(validate_task "$task")"
 printf '%s\n' "$report" >"$OUT/$task-after.json"
 jq -e '.grade=="full" and (.gaps|length)==0' "$OUT/$task-after.json" >/dev/null
done
epics="$(parse_epics "$TODOS_FILE")"
printf '%s\n' "$epics" >"$OUT/epics-after.json"
for phase in before after; do
 jq '[.[]|.tasks[]?|select(.id!="TASK-017-1" and .id!="TASK-017-2" and .id!="TASK-017-3")]' "$OUT/epics-$phase.json" >"$OUT/unrelated-$phase.json"
done
cmp "$OUT/unrelated-before.json" "$OUT/unrelated-after.json"
status=0
diff -u --label a/docs/ToDos.md --label b/docs/ToDos.md "$OUT/before.md" "$OUT/ToDos.md" >"$OUT/completion.patch.txt" || status=$?
[[ "$status" == 1 ]]
printf 'Strict completion succeeded on the tracker copy. Review the patch before applying it. The real tracker and index were not changed.\n'

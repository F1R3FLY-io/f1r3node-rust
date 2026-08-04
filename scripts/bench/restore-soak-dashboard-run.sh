#!/usr/bin/env bash
set -euo pipefail

SITE_DIR="${SITE_DIR:?SITE_DIR is required}"
RESTORE_REPORT_DIR="${RESTORE_REPORT_DIR:?RESTORE_REPORT_DIR is required}"
REMOVE_RUN_ID="${REMOVE_RUN_ID:?REMOVE_RUN_ID is required}"
RESTORE_RUN_ID="${RESTORE_RUN_ID:?RESTORE_RUN_ID is required}"
RESTORE_RUN_ATTEMPT="${RESTORE_RUN_ATTEMPT:?RESTORE_RUN_ATTEMPT is required}"
RESTORE_SERIES="${RESTORE_SERIES:?RESTORE_SERIES is required}"

case "$REMOVE_RUN_ID/$RESTORE_RUN_ID/$RESTORE_RUN_ATTEMPT" in
*[!0-9/]* | *//* | /* | */)
	echo "run IDs must be positive integers" >&2
	exit 2
	;;
esac
case "$RESTORE_SERIES" in
daily) suffix="-daily" ;;
weekend) suffix="" ;;
*)
	echo "RESTORE_SERIES must be daily or weekend" >&2
	exit 2
	;;
esac

history="$SITE_DIR/history${suffix}.json"
summary="$RESTORE_REPORT_DIR/weekly-summary.json"
verdict="$RESTORE_REPORT_DIR/verdict.json"
report="$RESTORE_REPORT_DIR/perf-report.md"

jq -e 'type == "array"' "$history" >/dev/null
jq -e --arg run_id "$RESTORE_RUN_ID" --argjson run_attempt "$RESTORE_RUN_ATTEMPT" --arg kind "$RESTORE_SERIES" '
  type == "object"
  and (.run.run_id | tostring) == $run_id
  and .run.run_attempt == $run_attempt
  and .run.kind == $kind
  and .run.status == "complete"
' "$summary" >/dev/null
jq -e --arg run_id "$RESTORE_RUN_ID" --argjson run_attempt "$RESTORE_RUN_ATTEMPT" --arg kind "$RESTORE_SERIES" '
  type == "object"
  and (.run.run_id | tostring) == $run_id
  and .run.run_attempt == $run_attempt
  and .run.kind == $kind
  and .run.status == "complete"
' "$verdict" >/dev/null
test -s "$report"

before="$(jq 'length' "$history")"
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
jq --arg remove "$REMOVE_RUN_ID" '
  map(select((.run.run_id | tostring) != $remove))
' "$history" >"$tmp"
after="$(jq 'length' "$tmp")"
if [ "$((before - after))" -ne 1 ]; then
	echo "history must contain exactly one entry for run $REMOVE_RUN_ID" >&2
	exit 1
fi
jq -e --arg restore "$RESTORE_RUN_ID" '
  length > 0 and ((last.run.run_id | tostring) == $restore)
' "$tmp" >/dev/null
mv "$tmp" "$history"
trap - EXIT

cp "$summary" "$SITE_DIR/latest-summary${suffix}.json"
cp "$verdict" "$SITE_DIR/latest-verdict${suffix}.json"
cp "$report" "$SITE_DIR/latest-report${suffix}.md"

for spec in badge.json:badge-soak badge-stability.json:badge-stability badge-perf.json:badge-perf; do
	source="$RESTORE_REPORT_DIR/${spec%%:*}"
	target="$SITE_DIR/${spec##*:}${suffix}.json"
	if [ -s "$source" ]; then
		cp "$source" "$target"
	else
		rm -f "$target"
	fi
done

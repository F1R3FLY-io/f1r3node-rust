#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/site" "$TMP/restore"
cat >"$TMP/site/history-daily.json" <<'JSON'
[
  {"run":{"run_id":"100","kind":"daily","status":"complete","date":"2026-01-01T00:00:00Z"}},
  {"run":{"run_id":"200","kind":"daily","status":"complete","date":"2026-01-02T00:00:00Z"}}
]
JSON
printf '%s\n' '{"run":{"run_id":"200"}}' >"$TMP/site/latest-summary-daily.json"
printf '%s\n' '{"run":{"run_id":"200"}}' >"$TMP/site/latest-verdict-daily.json"
printf '%s\n' canary >"$TMP/site/latest-report-daily.md"
printf '%s\n' '{}' >"$TMP/site/badge-soak-daily.json"
printf '%s\n' '{}' >"$TMP/site/badge-stability-daily.json"
printf '%s\n' '{}' >"$TMP/site/badge-perf-daily.json"
printf '%s\n' '{"run":{"run_id":"100","run_attempt":1,"kind":"daily","status":"complete"}}' \
	>"$TMP/restore/weekly-summary.json"
printf '%s\n' '{"run":{"run_id":"100","run_attempt":1,"kind":"daily","status":"complete"},"verdict":"regress"}' \
	>"$TMP/restore/verdict.json"
printf '%s\n' restored >"$TMP/restore/perf-report.md"

SITE_DIR="$TMP/site" RESTORE_REPORT_DIR="$TMP/restore" REMOVE_RUN_ID=200 \
	RESTORE_RUN_ID=100 RESTORE_RUN_ATTEMPT=1 RESTORE_SERIES=daily \
	"$ROOT/scripts/bench/restore-soak-dashboard-run.sh"

jq -e 'length == 1 and .[0].run.run_id == "100"' "$TMP/site/history-daily.json" >/dev/null
jq -e '.run.run_id == "100"' "$TMP/site/latest-summary-daily.json" >/dev/null
jq -e '.run.run_id == "100"' "$TMP/site/latest-verdict-daily.json" >/dev/null
grep -qx restored "$TMP/site/latest-report-daily.md"
test ! -e "$TMP/site/badge-soak-daily.json"
test ! -e "$TMP/site/badge-stability-daily.json"
test ! -e "$TMP/site/badge-perf-daily.json"

if SITE_DIR="$TMP/site" RESTORE_REPORT_DIR="$TMP/restore" REMOVE_RUN_ID=999 \
	RESTORE_RUN_ID=100 RESTORE_RUN_ATTEMPT=1 RESTORE_SERIES=daily \
	"$ROOT/scripts/bench/restore-soak-dashboard-run.sh" >/dev/null 2>&1; then
	echo 'missing removal target was accepted' >&2
	exit 1
fi

printf 'soak dashboard restoration tests passed\n'

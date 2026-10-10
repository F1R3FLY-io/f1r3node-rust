#!/usr/bin/env bash
set -euo pipefail

source oci/spend-alert/handler.sh

export TENANCY_OCID=ocid1.tenancy.oc1..test
export ONS_TOPIC_OCID=ocid1.onstopic.oc1..test
export NOW_EPOCH=1791381600

publish_log="$(mktemp)"
query_log="$(mktemp)"
trap 'rm -f "$publish_log" "$query_log"' EXIT
mock_items='[]'
mock_usage_fails=false

oci() {
	case "$1 $2" in
	"usage-api usage-summary")
		printf '%s\n' "$*" >>"$query_log"
		if [ "$mock_usage_fails" = true ]; then
			echo 'ServiceError: NotAuthorizedOrNotFound' >&2
			return 1
		fi
		jq -cn --argjson items "$mock_items" '{data:{items:$items}}'
		;;
	"ons message")
		printf '%q ' "$@" >>"$publish_log"
		printf '\n' >>"$publish_log"
		;;
	*)
		printf 'unexpected oci call: %s\n' "$*" >&2
		return 1
		;;
	esac
}

items() {
	jq -cn --argjson ci "$1" --argjson devops "$2" '[
        {"compartment-name":"ci-runner","service":"Compute","computed-amount":$ci,"currency":"USD"},
        {"compartment-name":"f1r3fly-devops","service":"Compute","computed-amount":$devops,"currency":"USD"}
    ]'
}

[ "$(previous_utc_day 1791381600)" = 2026-10-06 ]
[ "$(previous_utc_day 1791331200)" = 2026-10-06 ]
[ "$(next_utc_day 2026-10-31)" = 2026-11-01 ]

: >"$publish_log"
: >"$query_log"
mock_items="$(items 60.25 28.1)"
over="$(main </dev/null)"
[ "$(jq -r .status <<<"$over")" = alerted ]
[ "$(jq -r .day <<<"$over")" = 2026-10-06 ]
[ "$(jq -r .total <<<"$over")" = 88.35 ]
[ "$(wc -l <"$publish_log" | tr -d ' ')" -eq 1 ]
grep -q -- "--topic-id $ONS_TOPIC_OCID" "$publish_log"
grep -q -- '--time-usage-started 2026-10-06T00:00:00Z' "$query_log"
grep -q -- '--time-usage-ended 2026-10-07T00:00:00Z' "$query_log"
grep -q -- '--auth resource_principal' "$query_log"

body="$(alert_body "$(summarize 2026-10-06 70 <<<"{\"data\":{\"items\":$mock_items}}")")"
grep -q 'ci-runner / Compute' <<<"$body"
[ "$(grep -n 'ci-runner' <<<"$body" | cut -d: -f1)" -lt "$(grep -n 'f1r3fly-devops' <<<"$body" | cut -d: -f1)" ]

: >"$publish_log"
mock_items="$(items 40 29.99)"
under="$(main </dev/null)"
[ "$(jq -r .status <<<"$under")" = under_threshold ]
[ ! -s "$publish_log" ]

: >"$publish_log"
mock_items="$(items 40 30)"
at="$(main </dev/null)"
[ "$(jq -r .status <<<"$at")" = under_threshold ]
[ ! -s "$publish_log" ]

: >"$publish_log"
mock_items="$(items 40 20)"
custom="$(THRESHOLD_USD=50 main </dev/null)"
[ "$(jq -r .status <<<"$custom")" = alerted ]
[ "$(jq -r .threshold <<<"$custom")" = 50 ]

: >"$publish_log"
mock_items='[]'
set +e
main </dev/null >/dev/null 2>&1
empty_status=$?
set -e
[ "$empty_status" -ne 0 ]
[ ! -s "$publish_log" ]

mock_usage_fails=true
set +e
main </dev/null >/dev/null 2>&1
usage_status=$?
set -e
[ "$usage_status" -ne 0 ]
[ ! -s "$publish_log" ]
mock_usage_fails=false

set +e
THRESHOLD_USD=abc main </dev/null >/dev/null 2>&1
threshold_status=$?
set -e
[ "$threshold_status" -ne 0 ]

printf 'OCI spend alert Bash tests passed\n'

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
mock_month_items='[]'
mock_budgets='[]'
mock_usage_fails=false
mock_month_fails=false
mock_budgets_fail=false

oci() {
	case "$1 $2" in
	"usage-api usage-summary")
		printf '%s\n' "$*" >>"$query_log"
		if [[ " $* " == *" --granularity MONTHLY "* ]]; then
			if [ "$mock_month_fails" = true ]; then
				echo 'ServiceError: NotAuthorizedOrNotFound' >&2
				return 1
			fi
			jq -cn --argjson items "$mock_month_items" '{data:{items:$items}}'
			return 0
		fi
		if [ "$mock_usage_fails" = true ]; then
			echo 'ServiceError: NotAuthorizedOrNotFound' >&2
			return 1
		fi
		jq -cn --argjson items "$mock_items" '{data:{items:$items}}'
		;;
	"budgets budget")
		printf '%s\n' "$*" >>"$query_log"
		if [ "$mock_budgets_fail" = true ]; then
			echo 'ServiceError: NotAuthorizedOrNotFound' >&2
			return 1
		fi
		jq -cn --argjson budgets "$mock_budgets" '{data:$budgets}'
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

mock_month_items='[
    {"compartment-name":"ci-runner","computed-amount":420.5},
    {"compartment-name":"f1r3fly-devops","computed-amount":180.25},
    {"compartment-name":"f1r3fly","computed-amount":0.001}
]'
mock_budgets='[
    {"display-name":"ci-runner-monthly","amount":500,"actual-spend":532.13,"forecasted-spend":2315.7},
    {"display-name":"Monthly-Tenancy-Budget","amount":5000,"actual-spend":600.75,"forecasted-spend":3510.13}
]'

[ "$(previous_utc_day 1791381600)" = 2026-10-06 ]
[ "$(previous_utc_day 1791331200)" = 2026-10-06 ]
[ "$(next_utc_day 2026-10-31)" = 2026-11-01 ]
[ "$(month_start 2026-10-06)" = 2026-10-01 ]
[ "$(month_start 2026-11-01)" = 2026-11-01 ]

: >"$publish_log"
: >"$query_log"
mock_items="$(items 60.25 28.1)"
over="$(main </dev/null)"
[ "$(jq -r .status <<<"$over")" = reported ]
[ "$(jq -r .over <<<"$over")" = true ]
[ "$(jq -r .day <<<"$over")" = 2026-10-06 ]
[ "$(jq -r .total <<<"$over")" = 88.35 ]
[ "$(jq -r .month_total <<<"$over")" = 600.75 ]
[ "$(wc -l <"$publish_log" | tr -d ' ')" -eq 1 ]
grep -q -- "--topic-id $ONS_TOPIC_OCID" "$publish_log"
grep -q -- 'ABOVE' "$publish_log"
grep -q -- '--time-usage-started 2026-10-06T00:00:00Z' "$query_log"
grep -q -- '--time-usage-ended 2026-10-07T00:00:00Z' "$query_log"
grep -q -- '--time-usage-started 2026-10-01T00:00:00Z --time-usage-ended 2026-10-07T00:00:00Z --granularity MONTHLY' "$query_log"
grep -q -- 'budgets budget budget list --auth resource_principal --compartment-id ocid1.tenancy.oc1..test' "$query_log"
grep -q -- '--auth resource_principal' "$query_log"

summary="$(summarize 2026-10-06 70 <<<"{\"data\":{\"items\":$mock_items}}")"
month="$(month_summary 2026-10-06 <<<"{\"data\":{\"items\":$mock_month_items}}")"
budgets="$(budget_summary <<<"{\"data\":$mock_budgets}")"
body="$(report_body "$summary" "$month" "$budgets")"
grep -q 'ci-runner / Compute' <<<"$body"
[ "$(grep -n 'ci-runner / Compute' <<<"$body" | cut -d: -f1)" -lt "$(grep -n 'f1r3fly-devops / Compute' <<<"$body" | cut -d: -f1)" ]
grep -qx 'Month to date (2026-10-01 to 2026-10-06): 600.75 USD' <<<"$body"
grep -qx '  420.5 USD  ci-runner' <<<"$body"
if grep -q 'USD  f1r3fly$' <<<"$body"; then exit 1; fi
grep -qx '  ci-runner-monthly: 532.13 of 500 (106 percent), forecast 2315.7' <<<"$body"
grep -qx '  Monthly-Tenancy-Budget: 600.75 of 5000 (12 percent), forecast 3510.13' <<<"$body"

: >"$publish_log"
mock_items="$(items 40 29.99)"
under="$(main </dev/null)"
[ "$(jq -r .status <<<"$under")" = reported ]
[ "$(jq -r .over <<<"$under")" = false ]
[ "$(wc -l <"$publish_log" | tr -d ' ')" -eq 1 ]
grep -q -- 'within' "$publish_log"

: >"$publish_log"
mock_items="$(items 40 30)"
at="$(main </dev/null)"
[ "$(jq -r .over <<<"$at")" = false ]
[ "$(wc -l <"$publish_log" | tr -d ' ')" -eq 1 ]

: >"$publish_log"
mock_items="$(items 40 20)"
custom="$(THRESHOLD_USD=50 main </dev/null)"
[ "$(jq -r .over <<<"$custom")" = true ]
[ "$(jq -r .threshold <<<"$custom")" = 50 ]

: >"$publish_log"
mock_month_fails=true
mock_budgets_fail=true
degraded="$(main </dev/null)"
[ "$(jq -r .status <<<"$degraded")" = reported ]
[ "$(jq -r .month_total <<<"$degraded")" = null ]
[ "$(wc -l <"$publish_log" | tr -d ' ')" -eq 1 ]
grep -q 'Month to date: unavailable' "$publish_log"
grep -q 'Budgets: unavailable' "$publish_log"
mock_month_fails=false
mock_budgets_fail=false

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

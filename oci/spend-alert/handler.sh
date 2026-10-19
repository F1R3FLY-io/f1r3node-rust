#!/usr/bin/env bash
set -euo pipefail

OCI_AUTH_ARGS=(--auth "${OCI_AUTH:-resource_principal}")
COST_ANALYSIS_URL="${COST_ANALYSIS_URL:-https://cloud.oracle.com/account-management/cost-analysis}"

required_config() {
	local name="$1"
	if [ -z "${!name:-}" ]; then
		printf '%s is required\n' "$name" >&2
		return 1
	fi
}

utc_day() {
	local epoch="$1"
	if date -u -d @0 +%s >/dev/null 2>&1; then
		date -u -d "@$epoch" +%F
	else
		date -u -r "$epoch" +%F
	fi
}

previous_utc_day() {
	local now_epoch="$1"
	utc_day "$((now_epoch - 86400))"
}

next_utc_day() {
	local day="$1"
	if date -u -d @0 +%s >/dev/null 2>&1; then
		date -u -d "$day + 1 day" +%F
	else
		date -u -j -v+1d -f %F "$day" +%F
	fi
}

daily_cost_items() {
	local day="$1" next
	next="$(next_utc_day "$day")"
	oci usage-api usage-summary request-summarized-usages \
		"${OCI_AUTH_ARGS[@]}" \
		--tenant-id "$TENANCY_OCID" \
		--time-usage-started "${day}T00:00:00Z" \
		--time-usage-ended "${next}T00:00:00Z" \
		--granularity DAILY \
		--query-type COST \
		--group-by '["compartmentName","service"]' \
		--compartment-depth 6 \
		--output json
}

month_start() {
	printf '%s-01\n' "${1%-*}"
}

month_cost_items() {
	local day="$1" start next
	start="$(month_start "$day")"
	next="$(next_utc_day "$day")"
	oci usage-api usage-summary request-summarized-usages \
		"${OCI_AUTH_ARGS[@]}" \
		--tenant-id "$TENANCY_OCID" \
		--time-usage-started "${start}T00:00:00Z" \
		--time-usage-ended "${next}T00:00:00Z" \
		--granularity MONTHLY \
		--query-type COST \
		--group-by '["compartmentName"]' \
		--compartment-depth 6 \
		--output json
}

budget_items() {
	oci budgets budget budget list \
		"${OCI_AUTH_ARGS[@]}" \
		--compartment-id "$TENANCY_OCID" \
		--target-type ALL \
		--all \
		--output json
}

month_summary() {
	local day="$1"
	jq -c --arg start "$(month_start "$day")" --arg day "$day" '
        [.data.items[]? | {compartment: (.["compartment-name"] // "-"), cost: (.["computed-amount"] // 0)}]
        | group_by(.compartment)
        | map({compartment: .[0].compartment, cost: (map(.cost) | add)})
        | {start: $start, end: $day,
           total: ((map(.cost) | add // 0) * 100 | round / 100),
           compartments: (sort_by(-.cost) | map(select(.cost >= 0.01)) | map(.cost = ((.cost * 100 | round) / 100)))}'
}

budget_summary() {
	jq -c '[.data[]? | {name: .["display-name"], amount: .amount,
        actual: ((.["actual-spend"] // 0) * 100 | round / 100),
        forecast: ((.["forecasted-spend"] // 0) * 100 | round / 100),
        percent: (if (.amount // 0) > 0 then ((.["actual-spend"] // 0) / .amount * 100 | round) else null end)}]'
}

summarize() {
	local day="$1" threshold="$2"
	jq -e --arg day "$day" --argjson threshold "$threshold" '
        [.data.items[]? | {
            compartment: (.["compartment-name"] // "-"),
            service: (.service // "-"),
            cost: (.["computed-amount"] // 0),
            currency: (.currency // "USD")
        }] as $rows
        | if ($rows | length) == 0 then error("no cost data for \($day)") else . end
        | ($rows | map(.cost) | add) as $total
        | {
            day: $day,
            total: (($total * 100 | round) / 100),
            threshold: $threshold,
            currency: $rows[0].currency,
            over: ($total > $threshold),
            top: ($rows | sort_by(-.cost) | .[:5] | map(.cost = ((.cost * 100 | round) / 100)))
        }'
}

report_body() {
	local summary="$1" month="${2:-null}" budgets="${3:-null}"
	jq -r --arg url "$COST_ANALYSIS_URL" --argjson month "$month" --argjson budgets "$budgets" '
        .currency as $currency
        | "OCI tenancy spend for \(.day) (UTC) was \(.total) \(.currency), \(if .over then "above" else "within" end) the \(.threshold) \(.currency) daily threshold.",
        "",
        "Largest compartment and service costs:",
        (.top[] | "  \(.cost) \(.currency)  \(.compartment) / \(.service)"),
        "",
        (if $month == null then "Month to date: unavailable" else
            ("Month to date (\($month.start) to \($month.end)): \($month.total) \(.currency)",
             ($month.compartments[] | "  \(.cost) \($currency)  \(.compartment)")) end),
        "",
        (if $budgets == null then "Budgets: unavailable" else
            ("Budgets (actual and forecast for the month):",
             ($budgets[] | "  \(.name): \(.actual) of \(.amount) (\(.percent // "-") percent), forecast \(.forecast)")) end),
        "",
        "Usage data can lag by several hours, so the final figure for the day can be higher.",
        "Cost Analysis: \($url)"' <<<"$summary"
}

publish_report() {
	local summary="$1" month="$2" budgets="$3" title
	required_config ONS_TOPIC_OCID
	title="$(jq -r '"OCI daily cost report \(.day): \(.total) \(.currency), \(if .over then "ABOVE" else "within" end) the \(.threshold) \(.currency) daily threshold"' <<<"$summary")"
	oci ons message publish \
		"${OCI_AUTH_ARGS[@]}" \
		--topic-id "$ONS_TOPIC_OCID" \
		--title "$title" \
		--body "$(report_body "$summary" "$month" "$budgets")" >/dev/null
}

main() {
	local now_epoch threshold day items summary month budgets
	cat >/dev/null || true
	required_config TENANCY_OCID || return 1
	threshold="${THRESHOLD_USD:-70}"
	if ! [[ "$threshold" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
		echo 'THRESHOLD_USD must be a non-negative number' >&2
		return 1
	fi
	now_epoch="${NOW_EPOCH:-$(date -u +%s)}"
	day="$(previous_utc_day "$now_epoch")" || return 1
	items="$(daily_cost_items "$day")" || return 1
	summary="$(summarize "$day" "$threshold" <<<"$items")" || return 1
	month="$(month_cost_items "$day" | month_summary "$day")" || month=null
	budgets="$(budget_items | budget_summary)" || budgets=null
	publish_report "$summary" "$month" "$budgets" || return 1
	jq -c --argjson month "$month" '{status:"reported",day,total,threshold,over,month_total:($month.total? // null)}' <<<"$summary"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
	main
fi

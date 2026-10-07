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

alert_body() {
	local summary="$1"
	jq -r --arg url "$COST_ANALYSIS_URL" '
        "OCI tenancy spend for \(.day) (UTC) was \(.total) \(.currency), above the \(.threshold) \(.currency) daily threshold.",
        "",
        "Largest compartment and service costs:",
        (.top[] | "  \(.cost) \(.currency)  \(.compartment) / \(.service)"),
        "",
        "Usage data can lag by several hours, so the final figure for the day can be higher.",
        "Cost Analysis: \($url)"' <<<"$summary"
}

publish_alert() {
	local summary="$1" title
	required_config ONS_TOPIC_OCID
	title="$(jq -r '"OCI daily spend \(.total) \(.currency) on \(.day) is above \(.threshold) \(.currency)"' <<<"$summary")"
	oci ons message publish \
		"${OCI_AUTH_ARGS[@]}" \
		--topic-id "$ONS_TOPIC_OCID" \
		--title "$title" \
		--body "$(alert_body "$summary")" >/dev/null
}

main() {
	local now_epoch threshold day items summary
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
	if [ "$(jq -r .over <<<"$summary")" = true ]; then
		publish_alert "$summary" || return 1
		jq -c '{status:"alerted",day,total,threshold}' <<<"$summary"
	else
		jq -c '{status:"under_threshold",day,total,threshold}' <<<"$summary"
	fi
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
	main
fi

#!/usr/bin/env bash
set -euo pipefail

source oci/soak-scheduler/handler.sh

assert_slot() {
	local now_epoch="$1" hour="$2" expected_series="$3" expected_ref="$4" result
	result="$(resolve_slot "$now_epoch" "$hour" 900)"
	IFS=$'\t' read -r _ series target_ref _ <<<"$result"
	[ "$series" = "$expected_series" ]
	[ "$target_ref" = "$expected_ref" ]
}

assert_ineligible() {
	local now_epoch="$1" hour="$2" status
	set +e
	resolve_slot "$now_epoch" "$hour" 900 >/dev/null
	status=$?
	set -e
	[ "$status" -eq 10 ]
}

assert_slot 1785551460 2 weekend master
assert_ineligible 1785555060 3
assert_slot 1796441460 3 weekend master
assert_ineligible 1796437860 2
assert_slot 1785810660 2 daily dev
assert_ineligible 1785637860 2

set +e
resolve_slot 1785552361 2 900 >/dev/null 2>&1
late_status=$?
set -e
[ "$late_status" -ne 0 ]

export GITHUB_REPOSITORY=F1R3FLY-io/f1r3node-rust
export GITHUB_WORKFLOW=merge-recovery-soak.yml
export GITHUB_WORKFLOW_REF=master
request_log="$(mktemp)"
TMP_FILES+=("$request_log")
mock_duplicate=false

github_api() {
	local method="$1" payload="${4:-}"
	if [ "$method" = GET ]; then
		if [ "$mock_duplicate" = true ]; then
			printf '{"workflow_runs":[{"display_title":"Merge Recovery Soak [scheduled:300]"}]}\n'
		else
			printf '{"workflow_runs":[]}\n'
		fi
	else
		printf '%s\n' "$payload" >>"$request_log"
		printf '{}\n'
	fi
}

daily_result="$(dispatch_slot token 100 daily dev daily-24h)"
[ "$(jq -r .status <<<"$daily_result")" = dispatched ]
[ "$(jq -r .inputs.target_ref <"$request_log")" = dev ]
[ "$(jq -r .inputs.duration <"$request_log")" = daily-24h ]

: >"$request_log"
weekend_result="$(dispatch_slot token 200 weekend master weekend-60h)"
[ "$(jq -r .series <<<"$weekend_result")" = weekend ]
[ "$(jq -r .inputs.target_ref <"$request_log")" = master ]
[ "$(jq -r .inputs.duration <"$request_log")" = weekend-60h ]

: >"$request_log"
mock_duplicate=true
duplicate_result="$(dispatch_slot token 300 daily dev daily-24h)"
[ "$(jq -r .status <<<"$duplicate_result")" = duplicate ]
[ ! -s "$request_log" ]

printf 'OCI soak scheduler Bash tests passed\n'

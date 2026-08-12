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

pdt_friday_0231_utc=1785551460
pdt_friday_0331_utc=1785555060
pst_friday_0331_utc=1796441460
pst_friday_0231_utc=1796437860
pdt_monday_0231_utc=1785810660
pdt_saturday_0231_utc=1785637860

assert_slot "$pdt_friday_0231_utc" 2 weekend master
assert_ineligible "$pdt_friday_0331_utc" 3
assert_slot "$pst_friday_0331_utc" 3 weekend master
assert_ineligible "$pst_friday_0231_utc" 2
assert_slot "$pdt_monday_0231_utc" 2 daily dev
assert_ineligible "$pdt_saturday_0231_utc" 2

set +e
resolve_slot 1785552361 2 900 >/dev/null 2>&1
late_status=$?
set -e
[ "$late_status" -ne 0 ]

# github_api retry behavior, exercised against the real function with a curl
# stub. Attempt counting goes through a file because github_api runs inside
# command substitutions, where a shell variable counter would be lost.
curl_attempts_file="$(mktemp)"
TMP_FILES+=("$curl_attempts_file")
curl_fail_until=0
curl_mock_status=200
curl() {
	printf 'x' >>"$curl_attempts_file"
	local out="" count
	count="$(wc -c <"$curl_attempts_file" | tr -d ' ')"
	while [ $# -gt 0 ]; do
		case "$1" in
		--output)
			out="$2"
			shift 2
			;;
		*) shift ;;
		esac
	done
	if [ "$count" -le "$curl_fail_until" ]; then
		return 35
	fi
	printf '{"ok":true}' >"$out"
	printf '%s' "$curl_mock_status"
}

# Transport failure (TLS-style, curl exit 35) on the first two attempts must
# be retried and then succeed — the 2026-08-12T02:30Z lost-slot regression.
: >"$curl_attempts_file"
curl_fail_until=2
retry_result="$(GITHUB_API_RETRY_DELAY_SECONDS=0 github_api GET /probe token)"
[ "$retry_result" = '{"ok":true}' ]
[ "$(wc -c <"$curl_attempts_file" | tr -d ' ')" -eq 3 ]

# Persistent transport failure exhausts the attempts and fails.
: >"$curl_attempts_file"
curl_fail_until=99
set +e
GITHUB_API_RETRY_DELAY_SECONDS=0 github_api GET /probe token >/dev/null 2>&1
transport_status=$?
set -e
[ "$transport_status" -ne 0 ]
[ "$(wc -c <"$curl_attempts_file" | tr -d ' ')" -eq 3 ]

# 4xx is deterministic: fail on the first attempt, no retries.
: >"$curl_attempts_file"
curl_fail_until=0
curl_mock_status=404
set +e
github_api GET /probe token >/dev/null 2>&1
notfound_status=$?
set -e
[ "$notfound_status" -ne 0 ]
[ "$(wc -c <"$curl_attempts_file" | tr -d ' ')" -eq 1 ]
unset -f curl

export GITHUB_REPOSITORY=F1R3FLY-io/f1r3node-rust
export GITHUB_WORKFLOW=merge-recovery-soak.yml
export GITHUB_WORKFLOW_REF=master
request_log="$(mktemp)"
TMP_FILES+=("$request_log")
mock_duplicate=false
mock_run_missing=false
DISPATCH_VERIFY_ATTEMPTS=2
DISPATCH_VERIFY_DELAY_SECONDS=0

# GETs answer like the live runs endpoint: once a dispatch has been logged,
# the [scheduled:<epoch>] run exists — unless mock_run_missing simulates a
# dispatch GitHub accepted but never turned into a run.
github_api() {
	local method="$1" payload="${4:-}"
	if [ "$method" = GET ]; then
		if [ "$mock_duplicate" = true ]; then
			printf '{"workflow_runs":[{"display_title":"Merge Recovery Soak [scheduled:300]"}]}\n'
		elif [ -s "$request_log" ] && [ "$mock_run_missing" = false ]; then
			local epoch
			epoch="$(tail -1 "$request_log" | jq -r '.inputs.scheduled_slot_epoch')"
			printf '{"workflow_runs":[{"display_title":"Merge Recovery Soak [scheduled:%s]"}]}\n' "$epoch"
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

# Accepted-but-vanished dispatch: the POST succeeds, the run never appears,
# and dispatch_slot must exit nonzero instead of reporting "dispatched".
: >"$request_log"
mock_duplicate=false
mock_run_missing=true
set +e
dispatch_slot token 400 daily dev daily-24h >/dev/null 2>&1
missing_status=$?
set -e
[ "$missing_status" -ne 0 ]
[ -s "$request_log" ]

printf 'OCI soak scheduler Bash tests passed\n'

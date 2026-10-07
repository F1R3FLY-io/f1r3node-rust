#!/usr/bin/env bash
set -euo pipefail

API_ROOT=https://api.github.com
API_VERSION=2022-11-28
USER_AGENT=f1r3node-oci-soak-scheduler
TMP_FILES=()

cleanup() {
	if [ "${#TMP_FILES[@]}" -gt 0 ]; then
		rm -f "${TMP_FILES[@]}"
	fi
}
trap cleanup EXIT

required_config() {
	local name="$1"
	if [ -z "${!name:-}" ]; then
		printf '%s is required\n' "$name" >&2
		return 1
	fi
}

epoch_format() {
	local epoch="$1" zone="$2" format="$3"
	if date -u -d @0 +%s >/dev/null 2>&1; then
		TZ="$zone" date -d "@$epoch" "+$format"
	else
		TZ="$zone" date -r "$epoch" "+$format"
	fi
}

utc_slot_epoch() {
	local now_epoch="$1" hour="$2" day candidate
	day="$(epoch_format "$now_epoch" UTC %F)"
	if date -u -d @0 +%s >/dev/null 2>&1; then
		candidate="$(date -u -d "$day ${hour}:30:00" +%s)"
	else
		candidate="$(TZ=UTC date -j -f '%Y-%m-%d %H:%M:%S' "$day ${hour}:30:00" +%s)"
	fi
	if [ "$candidate" -gt "$now_epoch" ]; then
		candidate=$((candidate - 86400))
	fi
	printf '%s\n' "$candidate"
}

resolve_slot() {
	local now_epoch="$1" hour="$2" max_delay="$3"
	case "$hour" in
	2 | 3) ;;
	*)
		echo 'slot_hour_utc must be 2 or 3' >&2
		return 1
		;;
	esac
	case "$max_delay" in
	'' | *[!0-9]*)
		echo 'MAX_TRIGGER_DELAY_SECONDS must be a non-negative integer' >&2
		return 1
		;;
	esac
	local slot_epoch delay pacific_hm pacific_weekday series target_ref duration
	slot_epoch="$(utc_slot_epoch "$now_epoch" "$hour")"
	delay=$((now_epoch - slot_epoch))
	if [ "$delay" -gt "$max_delay" ]; then
		printf 'scheduled invocation is %ss late; limit is %ss\n' "$delay" "$max_delay" >&2
		return 1
	fi
	pacific_hm="$(epoch_format "$slot_epoch" America/Los_Angeles %H%M)"
	pacific_weekday="$(epoch_format "$slot_epoch" America/Los_Angeles %u)"
	if [ "$pacific_hm" != 1930 ] || [ "$pacific_weekday" -lt 1 ] || [ "$pacific_weekday" -gt 5 ]; then
		return 10
	fi
	if [ "$pacific_weekday" -eq 5 ]; then
		series=weekend
		target_ref=master
		duration=weekend-60h
	else
		series=daily
		target_ref=dev
		duration=daily-24h
	fi
	printf '%s\t%s\t%s\t%s\n' "$slot_epoch" "$series" "$target_ref" "$duration"
}

base64url() {
	openssl base64 -A | tr '+/' '-_' | tr -d '='
}

github_api() {
	local method="$1" path="$2" token="$3" payload="${4:-}" response_file status curl_status attempt
	local attempts="${GITHUB_API_ATTEMPTS:-3}" retry_delay="${GITHUB_API_RETRY_DELAY_SECONDS:-2}"
	response_file="$(mktemp)"
	TMP_FILES+=("$response_file")
	local args=(
		--silent --show-error
		--connect-timeout 5
		--max-time 20
		--output "$response_file"
		--write-out '%{http_code}'
		--request "$method"
		--header 'Accept: application/vnd.github+json'
		--header "Authorization: Bearer $token"
		--header 'Content-Type: application/json'
		--header "User-Agent: $USER_AGENT"
		--header "X-GitHub-Api-Version: $API_VERSION"
	)
	if [ -n "$payload" ]; then
		args+=(--data "$payload")
	fi
	# Transport failures and 5xx are retried; 4xx are deterministic and are
	# not. The 2026-08-12T02:30Z slot was lost to a single intermittent TLS
	# handshake failure against api.github.com — one bad handshake must not
	# cost a night's soak. The retry loop lives here in bash because the
	# container's curl (Oracle Linux 8, 7.61) predates --retry-all-errors,
	# and plain --retry does not cover TLS failures. Worst case per call is
	# attempts x max-time plus the fixed delays, well inside the 120s
	# function timeout for the realistic instant-abort failure mode.
	for attempt in $(seq 1 "$attempts"); do
		curl_status=0
		status="$(curl "${args[@]}" "$API_ROOT$path")" || curl_status=$?
		if [ "$curl_status" -eq 0 ]; then
			case "$status" in
			5??) ;;
			*) break ;;
			esac
		fi
		if [ "$attempt" -lt "$attempts" ]; then
			printf 'GitHub API %s %s attempt %s/%s failed (curl exit %s, HTTP %s); retrying\n' \
				"$method" "$path" "$attempt" "$attempts" "$curl_status" "${status:-000}" >&2
			sleep "$retry_delay"
		fi
	done
	if [ "$curl_status" -ne 0 ]; then
		printf 'GitHub API %s %s failed: curl exit %s after %s attempt(s)\n' \
			"$method" "$path" "$curl_status" "$attempts" >&2
		return 1
	fi
	case "$status" in
	200 | 201) cat "$response_file" ;;
	204) printf '{}\n' ;;
	*)
		printf 'GitHub API %s %s failed with %s: ' "$method" "$path" "$status" >&2
		head -c 1000 "$response_file" >&2
		printf '\n' >&2
		return 1
		;;
	esac
}

read_private_key() {
	required_config GITHUB_APP_PRIVATE_KEY_SECRET_OCID
	oci secrets secret-bundle get \
		--auth resource_principal \
		--secret-id "$GITHUB_APP_PRIVATE_KEY_SECRET_OCID" \
		--stage CURRENT \
		--query 'data."secret-bundle-content".content' \
		--raw-output |
		base64 -d
}

github_installation_token() {
	local now_epoch="$1" private_key_file header claims signing_input signature app_jwt result token
	required_config GITHUB_APP_ID
	required_config GITHUB_APP_INSTALLATION_ID
	private_key_file="$(mktemp)"
	TMP_FILES+=("$private_key_file")
	chmod 600 "$private_key_file"
	read_private_key >"$private_key_file"
	[ -s "$private_key_file" ] || {
		echo 'OCI Vault returned an empty GitHub App private key' >&2
		return 1
	}
	header="$(printf '%s' '{"alg":"RS256","typ":"JWT"}' | base64url)"
	claims="$(jq -cn \
		--argjson iat "$((now_epoch - 60))" \
		--argjson exp "$((now_epoch + 540))" \
		--arg iss "$GITHUB_APP_ID" \
		'{iat:$iat,exp:$exp,iss:$iss}' |
		base64url)"
	signing_input="${header}.${claims}"
	signature="$(printf '%s' "$signing_input" |
		openssl dgst -sha256 -sign "$private_key_file" |
		base64url)"
	app_jwt="${signing_input}.${signature}"
	result="$(github_api POST \
		"/app/installations/${GITHUB_APP_INSTALLATION_ID}/access_tokens" \
		"$app_jwt" \
		'{"permissions":{"actions":"write"}}')"
	token="$(jq -er '.token | select(type == "string" and length > 0)' <<<"$result")"
	printf '%s\n' "$token"
}

workflow_api_path() {
	required_config GITHUB_REPOSITORY
	required_config GITHUB_WORKFLOW
	if ! [[ "$GITHUB_REPOSITORY" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
		echo 'GITHUB_REPOSITORY must have owner/repository form' >&2
		return 1
	fi
	if ! [[ "$GITHUB_WORKFLOW" =~ ^[A-Za-z0-9_.-]+$ ]]; then
		echo 'GITHUB_WORKFLOW must be a workflow file name' >&2
		return 1
	fi
	printf '/repos/%s/actions/workflows/%s' "$GITHUB_REPOSITORY" "$GITHUB_WORKFLOW"
}

# A 204 from the dispatch POST only means GitHub accepted the event, and the
# scheduling stack above this handler treats any zero exit as proof the slot
# was served (hotwrap masks handler failures as successful invocations, and
# the Resource Scheduler work request ignores the response body entirely).
# Polling until the [scheduled:<epoch>] run actually exists is therefore the
# only place "the soak is really launched" can be asserted; a dispatch that
# never materializes becomes a nonzero exit instead of a silent no-op.
verify_dispatched_run() {
	local token="$1" workflow_path="$2" title="$3" attempt runs
	local attempts="${DISPATCH_VERIFY_ATTEMPTS:-6}" delay="${DISPATCH_VERIFY_DELAY_SECONDS:-5}"
	for attempt in $(seq 1 "$attempts"); do
		sleep "$delay"
		if runs="$(github_api GET "${workflow_path}/runs?event=workflow_dispatch&per_page=100" "$token")" &&
			jq -e --arg title "$title" \
				'.workflow_runs[]? | select(.display_title == $title)' <<<"$runs" >/dev/null; then
			return 0
		fi
	done
	printf 'dispatch was accepted but no run titled "%s" appeared after %s check(s)\n' \
		"$title" "$attempts" >&2
	return 1
}

dispatch_slot() {
	local token="$1" slot_epoch="$2" series="$3" target_ref="$4" duration="$5"
	local workflow_path title runs payload
	workflow_path="$(workflow_api_path)"
	title="Merge Recovery Soak [scheduled:${slot_epoch}]"
	runs="$(github_api GET "${workflow_path}/runs?event=workflow_dispatch&per_page=100" "$token")"
	if jq -e --arg title "$title" '.workflow_runs[]? | select(.display_title == $title)' <<<"$runs" >/dev/null; then
		jq -cn --argjson slot_epoch "$slot_epoch" --arg series "$series" \
			'{status:"duplicate",slot_epoch:$slot_epoch,series:$series}'
		return 0
	fi
	payload="$(jq -cn \
		--arg ref "${GITHUB_WORKFLOW_REF:-master}" \
		--arg target_ref "$target_ref" \
		--arg duration "$duration" \
		--arg slot_epoch "$slot_epoch" \
		'{ref:$ref,inputs:{target_ref:$target_ref,duration:$duration,scheduled_slot_epoch:$slot_epoch}}')"
	github_api POST "${workflow_path}/dispatches" "$token" "$payload" >/dev/null || return 1
	verify_dispatched_run "$token" "$workflow_path" "$title" || return 1
	jq -cn --argjson slot_epoch "$slot_epoch" --arg series "$series" \
		'{status:"dispatched",slot_epoch:$slot_epoch,series:$series}'
}

main() {
	local payload hour max_delay now_epoch slot status slot_epoch series target_ref duration token result pacific_slot
	payload="$(cat)"
	hour="$(jq -er '.slot_hour_utc | select(type == "number" and (. == 2 or . == 3))' <<<"$payload")" || {
		echo 'slot_hour_utc must be 2 or 3' >&2
		return 1
	}
	max_delay="${MAX_TRIGGER_DELAY_SECONDS:-900}"
	now_epoch="${NOW_EPOCH:-$(date -u +%s)}"
	set +e
	slot="$(resolve_slot "$now_epoch" "$hour" "$max_delay")"
	status=$?
	set -e
	if [ "$status" -eq 10 ]; then
		jq -cn --argjson slot_hour_utc "$hour" '{status:"ineligible",slot_hour_utc:$slot_hour_utc}'
		return 0
	fi
	[ "$status" -eq 0 ] || return "$status"
	IFS=$'\t' read -r slot_epoch series target_ref duration <<<"$slot"
	token="$(github_installation_token "$now_epoch")"
	result="$(dispatch_slot "$token" "$slot_epoch" "$series" "$target_ref" "$duration")"
	pacific_slot="$(epoch_format "$slot_epoch" America/Los_Angeles '%Y-%m-%dT%H:%M:%S%z')"
	jq -c --arg pacific_slot "$pacific_slot" '. + {pacific_slot:$pacific_slot}' <<<"$result"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
	main
fi

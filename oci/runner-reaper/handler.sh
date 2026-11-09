#!/usr/bin/env bash
set -euo pipefail

OCI_AUTH_ARGS=(--auth "${OCI_AUTH:-resource_principal}")

required_config() {
	local name="$1"
	if [ -z "${!name:-}" ]; then
		printf '%s is required\n' "$name" >&2
		return 1
	fi
}

utc_stamp() {
	local epoch="$1"
	if date -u -d @0 +%s >/dev/null 2>&1; then
		date -u -d "@$epoch" +%Y-%m-%dT%H:%M:%S
	else
		date -u -r "$epoch" +%Y-%m-%dT%H:%M:%S
	fi
}

list_instances() {
	oci compute instance list \
		"${OCI_AUTH_ARGS[@]}" \
		--compartment-id "$COMPARTMENT_OCID" \
		--all \
		--output json
}

# Same rule as .github/workflows/ci-runner-reaper.yml. Every rejection of the
# soak-deadline-epoch tag fails toward cleanup: Infinity, NaN, overflowing
# literals, unparseable values, and deadlines more than 7 days ahead are
# reaped like an untagged instance.
select_leaked() {
	local now_epoch="$1" max_age_hours="$2" cutoff
	cutoff="$(utc_stamp "$((now_epoch - max_age_hours * 3600))")"
	jq -r --arg cutoff "$cutoff" --argjson now "$now_epoch" '
        (.data // [])[]
        | select(."lifecycle-state" == "RUNNING" or ."lifecycle-state" == "STOPPED")
        | select(."display-name" | startswith("ci-eph-"))
        | select(."time-created" < $cutoff)
        | ((."freeform-tags"["soak-deadline-epoch"] // "0") | tonumber? // 0) as $deadline
        | select($deadline < $now
                 or ($deadline | isinfinite)
                 or ($deadline | isnan)
                 or $deadline > ($now + 604800))
        | "\(.id)\t\(."display-name")\t\(."time-created")"'
}

terminate_instance() {
	oci compute instance terminate \
		"${OCI_AUTH_ARGS[@]}" \
		--instance-id "$1" \
		--force >/dev/null
}

main() {
	local payload dry_run max_age_hours now_epoch instances leaked id name created
	local found=0 terminated=0 failed=0 skipped=0
	payload="$(cat || true)"
	required_config COMPARTMENT_OCID || return 1
	max_age_hours="${MAX_AGE_HOURS:-2}"
	case "$max_age_hours" in
	'' | *[!0-9]* | 0)
		echo 'MAX_AGE_HOURS must be a positive integer' >&2
		return 1
		;;
	esac
	dry_run="${DRY_RUN:-false}"
	if [ -n "$payload" ] && [ "$(jq -r '.dry_run // false' <<<"$payload" 2>/dev/null)" = true ]; then
		dry_run=true
	fi
	now_epoch="${NOW_EPOCH:-$(date -u +%s)}"
	instances="$(list_instances)" || return 1
	leaked="$(select_leaked "$now_epoch" "$max_age_hours" <<<"$instances")" || return 1
	while IFS=$'\t' read -r id name created; do
		[ -n "$id" ] || continue
		found=$((found + 1))
		case "$name" in
		ci-eph-*) ;;
		*)
			printf 'SKIP unexpected name: %s\n' "$name" >&2
			skipped=$((skipped + 1))
			continue
			;;
		esac
		if [ "$dry_run" = true ]; then
			printf 'Would terminate %s (created %s)\n' "$name" "$created" >&2
		elif terminate_instance "$id"; then
			printf 'Terminated %s (created %s)\n' "$name" "$created" >&2
			terminated=$((terminated + 1))
		else
			printf 'FAILED to terminate %s\n' "$name" >&2
			failed=$((failed + 1))
		fi
	done <<<"$leaked"
	jq -cn \
		--argjson found "$found" \
		--argjson terminated "$terminated" \
		--argjson failed "$failed" \
		--argjson skipped "$skipped" \
		--argjson dry_run "$([ "$dry_run" = true ] && echo true || echo false)" \
		--argjson max_age_hours "$max_age_hours" \
		'{status:(if $failed > 0 then "partial" elif $found == 0 then "clean" elif $dry_run then "dry_run" else "reaped" end),found:$found,terminated:$terminated,failed:$failed,skipped:$skipped,dry_run:$dry_run,max_age_hours:$max_age_hours}'
	[ "$failed" -eq 0 ]
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
	main
fi

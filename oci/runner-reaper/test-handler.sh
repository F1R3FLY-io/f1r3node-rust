#!/usr/bin/env bash
set -euo pipefail

source oci/runner-reaper/handler.sh

export COMPARTMENT_OCID=ocid1.compartment.oc1..test
export NOW_EPOCH=1791381600
now=$NOW_EPOCH

terminate_log="$(mktemp)"
list_log="$(mktemp)"
trap 'rm -f "$terminate_log" "$list_log"' EXIT
fail_terminate_for=""
mock_list_fails=false

instance() {
	local id="$1" name="$2" state="$3" created="$4" deadline="${5:-}"
	jq -cn --arg id "$id" --arg name "$name" --arg state "$state" --arg created "$created" --arg deadline "$deadline" '
        {id:$id,"display-name":$name,"lifecycle-state":$state,"time-created":$created,
         "freeform-tags":(if $deadline == "" then {} else {"soak-deadline-epoch":$deadline} end)}'
}

old=2026-10-07T10:00:00.000000+00:00
young=2026-10-07T13:00:00.000000+00:00

fixture="$(jq -s '{data:.}' <<EOF
$(instance old-untagged ci-eph-f1r3node-rust-amd64-a RUNNING "$old")
$(instance young-untagged ci-eph-f1r3node-rust-amd64-b RUNNING "$young")
$(instance old-stopped ci-eph-f1r3node-rust-arm64-c STOPPED "$old")
$(instance old-terminated ci-eph-f1r3node-rust-amd64-d TERMINATED "$old")
$(instance old-other-name soak-host RUNNING "$old")
$(instance soak-future ci-eph-f1r3node-rust-amd64-e RUNNING "$old" "$((now + 3600))")
$(instance soak-expired ci-eph-f1r3node-rust-amd64-f RUNNING "$old" "$((now - 1))")
$(instance soak-infinity ci-eph-f1r3node-rust-amd64-g RUNNING "$old" Infinity)
$(instance soak-nan ci-eph-f1r3node-rust-amd64-h RUNNING "$old" nan)
$(instance soak-overflow ci-eph-f1r3node-rust-amd64-i RUNNING "$old" 1e309)
$(instance soak-garbage ci-eph-f1r3node-rust-amd64-j RUNNING "$old" abc)
$(instance soak-far ci-eph-f1r3node-rust-amd64-k RUNNING "$old" "$((now + 8 * 86400))")
EOF
)"

oci() {
	case "$1 $2 $3" in
	"compute instance list")
		printf '%s\n' "$*" >>"$list_log"
		if [ "$mock_list_fails" = true ]; then
			echo 'ServiceError: NotAuthorizedOrNotFound' >&2
			return 1
		fi
		printf '%s\n' "$fixture"
		;;
	"compute instance terminate")
		local id=""
		while [ $# -gt 0 ]; do
			case "$1" in
			--instance-id)
				id="$2"
				shift 2
				;;
			*) shift ;;
			esac
		done
		printf '%s\n' "$id" >>"$terminate_log"
		[ "$id" != "$fail_terminate_for" ]
		;;
	*)
		printf 'unexpected oci call: %s\n' "$*" >&2
		return 1
		;;
	esac
}

expected_reaped="old-stopped
old-untagged
soak-expired
soak-far
soak-garbage
soak-infinity
soak-nan
soak-overflow"

selected="$(select_leaked "$now" 2 <<<"$fixture" | cut -f1 | sort)"
[ "$selected" = "$expected_reaped" ]

[ -z "$(select_leaked "$now" 5 <<<"$fixture" | cut -f1)" ]

workflow_filter="$(awk '
    /\| jq -r --arg cutoff "\$cutoff" --argjson now "\$now_epoch" '"'"'$/ { inside = 1; next }
    inside && /^[[:space:]]*\| "\\\(\.id\) / { print "| .id"; exit }
    inside { print }
' .github/workflows/ci-runner-reaper.yml)"
[ -n "$workflow_filter" ]
workflow_selected="$(jq -r --arg cutoff "$(utc_stamp "$((now - 2 * 3600))")" --argjson now "$now" "$workflow_filter" <<<"$fixture" | sort)"
[ "$workflow_selected" = "$expected_reaped" ]

: >"$terminate_log"
result="$(main </dev/null 2>/dev/null)"
[ "$(jq -r .status <<<"$result")" = reaped ]
[ "$(jq -r .found <<<"$result")" -eq 8 ]
[ "$(jq -r .terminated <<<"$result")" -eq 8 ]
[ "$(sort "$terminate_log")" = "$expected_reaped" ]
grep -q -- "--compartment-id $COMPARTMENT_OCID" "$list_log"
grep -q -- '--auth resource_principal' "$list_log"

: >"$terminate_log"
dry="$(DRY_RUN=true main </dev/null 2>/dev/null)"
[ "$(jq -r .status <<<"$dry")" = dry_run ]
[ "$(jq -r .found <<<"$dry")" -eq 8 ]
[ ! -s "$terminate_log" ]

: >"$terminate_log"
dry_payload="$(main <<<'{"dry_run":true}' 2>/dev/null)"
[ "$(jq -r .dry_run <<<"$dry_payload")" = true ]
[ ! -s "$terminate_log" ]

: >"$terminate_log"
clean="$(MAX_AGE_HOURS=5 main </dev/null 2>/dev/null)"
[ "$(jq -r .status <<<"$clean")" = clean ]
[ ! -s "$terminate_log" ]

: >"$terminate_log"
fail_terminate_for=old-untagged
set +e
partial="$(main </dev/null 2>/dev/null)"
partial_status=$?
set -e
[ "$partial_status" -ne 0 ]
[ "$(jq -r .status <<<"$partial")" = partial ]
[ "$(jq -r .failed <<<"$partial")" -eq 1 ]
[ "$(wc -l <"$terminate_log" | tr -d ' ')" -eq 8 ]
fail_terminate_for=""

mock_list_fails=true
set +e
main </dev/null >/dev/null 2>&1
list_status=$?
set -e
[ "$list_status" -ne 0 ]
mock_list_fails=false

for bad in 0 abc -1 ''; do
	set +e
	MAX_AGE_HOURS="$bad" main </dev/null >/dev/null 2>&1
	bad_status=$?
	set -e
	if [ -n "$bad" ]; then
		[ "$bad_status" -ne 0 ]
	fi
done

set +e
COMPARTMENT_OCID='' main </dev/null >/dev/null 2>&1
missing_status=$?
set -e
[ "$missing_status" -ne 0 ]

printf 'OCI runner reaper Bash tests passed\n'

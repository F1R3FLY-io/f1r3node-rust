#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
log="$work/calls.log"

cat >"$work/bin/oci" <<'STUB'
#!/usr/bin/env bash
printf '%q ' "$@" >>"$STUB_LOG"
printf '\n' >>"$STUB_LOG"
case "$1 $2 $3" in
"os ns get") echo testns ;;
"fn application list") echo '{"data":[{"display-name":"f1r3node-ci-schedulers","lifecycle-state":"ACTIVE","id":"app-1"}]}' ;;
"artifacts container repository") echo '{"data":{"items":[]}}' ;;
"fn function list") echo '{"data":[]}' ;;
"fn function create") echo fn-1 ;;
"resource-scheduler schedule list") echo '{"data":{"items":[]}}' ;;
"resource-scheduler schedule create") echo sched-1 ;;
"iam dynamic-group list") echo '{"data":[]}' ;;
"iam dynamic-group create")
	while [ $# -gt 0 ]; do
		[ "$1" = --matching-rule ] && printf '%s' "$2" >"$STUB_LOG.rule"
		shift
	done
	echo dg-1
	;;
"iam dynamic-group get") cat "$STUB_LOG.rule" ;;
"iam policy list") echo '{"data":[]}' ;;
"iam policy create") echo pol-1 ;;
*) ;;
esac
STUB
cat >"$work/bin/docker" <<'STUB'
#!/usr/bin/env bash
printf 'docker %s\n' "$*" >>"$STUB_LOG"
STUB
chmod +x "$work/bin/oci" "$work/bin/docker"
printf '[DEFAULT]\nregion=us-sanjose-1\ntenancy=ocid1.tenancy.oc1..test\n' >"$work/config"

run() {
	env PATH="$work/bin:$PATH" STUB_LOG="$log" OCI_CLI_CONFIG_FILE="$work/config" IMAGE_TAG=test "$@"
}

workflow_compartment="$(grep -hoE 'CI_RUNNER_COMPARTMENT_OCID:[[:space:]]*"ocid1\.compartment\.[A-Za-z0-9._-]+"' \
	"$ROOT/.github/workflows/ci-runner-reaper.yml" | sed -E 's/.*"(ocid1[^"]+)"/\1/')"
[ -n "$workflow_compartment" ]

: >"$log"
reaper_out="$(run "$ROOT/scripts/oci/deploy-runner-reaper.sh")"
[ "$(jq -r .function_id <<<"$reaper_out")" = fn-1 ]
[ "$(jq -r .schedule_cron_utc <<<"$reaper_out")" = '*/30 * * * *' ]
grep -q "docker build --platform linux/amd64 -t us-sanjose-1.ocir.io/testns/f1r3node/runner-reaper:test" "$log"
grep -q "COMPARTMENT_OCID.*$workflow_compartment" "$log"
grep -q "MAX_AGE_HOURS" "$log"
grep -q "manage\\\\ instance-family\\\\ in\\\\ compartment\\\\ id\\\\ $workflow_compartment" "$log"
grep -q "use\\\\ fn-invocation" "$log"
if grep -q 'usage-report' "$log"; then echo "unexpected match: 'usage-report'" >&2; exit 1; fi

: >"$log"
set +e
run env OCI_COMPARTMENT_OCID=ocid1.compartment.oc1..other "$ROOT/scripts/oci/deploy-runner-reaper.sh" >/dev/null 2>&1
mismatch_status=$?
set -e
[ "$mismatch_status" -ne 0 ]
if grep -q 'docker build' "$log"; then echo "unexpected match: 'docker build'" >&2; exit 1; fi

: >"$log"
set +e
run env MAX_AGE_HOURS=0 "$ROOT/scripts/oci/deploy-runner-reaper.sh" >/dev/null 2>&1
age_status=$?
set -e
[ "$age_status" -ne 0 ]

: >"$log"
alert_out="$(run env OCI_COMPARTMENT_OCID=ocid1.compartment.oc1..ci ONS_TOPIC_OCID=ocid1.onstopic.oc1..t "$ROOT/scripts/oci/deploy-spend-alert.sh")"
[ "$(jq -r .threshold_usd <<<"$alert_out")" = 70 ]
[ "$(jq -r .schedule_cron_utc <<<"$alert_out")" = '0 14 * * *' ]
grep -q "read\\\\ usage-report\\\\ in\\\\ tenancy" "$log"
grep -q "use\\\\ ons-topics\\\\ in\\\\ compartment\\\\ id\\\\ ocid1.compartment.oc1..ci" "$log"
if grep -q 'manage\\ instance-family' "$log"; then echo "unexpected match: 'manage\\ instance-family'" >&2; exit 1; fi

printf 'OCI Function deploy script tests passed\n'

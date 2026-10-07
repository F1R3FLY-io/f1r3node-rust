#!/usr/bin/env bash
# Runs the run-scoped VM cleanup steps against stub gh and oci commands:
# the two steps of ci-runner-reclaim.yml and the in-run reclaim step of
# _integration-pipeline.yml (EPIC-022 TASK-022-4).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

extract_step() {
	local file="$1" step="$2" out="$3"
	awk -v step="$step" '
        $0 == "      - name: " step { inside = 1; next }
        inside && /^        run: \|$/ { body = 1; next }
        body && /^      - name: / { exit }
        body && /^  [a-z_]+:$/ { exit }
        body { sub(/^          /, ""); print }
    ' "$file" >"$out"
	[ -s "$out" ]
	bash -n "$out"
}

extract_step "$ROOT/.github/workflows/ci-runner-reclaim.yml" "Check whether the run launched OCI runners" "$work/detect.sh"
extract_step "$ROOT/.github/workflows/ci-runner-reclaim.yml" "Terminate VMs tagged with this run attempt" "$work/terminate.sh"
extract_step "$ROOT/.github/workflows/_integration-pipeline.yml" "Terminate this run's remaining VMs" "$work/inrun.sh"

mkdir -p "$work/bin"
cat >"$work/bin/sleep" <<'STUB'
#!/usr/bin/env bash
exit 0
STUB
cat >"$work/bin/gh" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$STUB_DIR/gh.log"
case "$*" in
*"/attempts/"*"/jobs"*) cat "$STUB_DIR/launch_states" ;;
"api repos/"*"/actions/runners --paginate"*) cat "$STUB_DIR/runners" ;;
"api -X DELETE "*) exit 0 ;;
*) echo "unexpected gh call: $*" >&2; exit 1 ;;
esac
STUB
cat >"$work/bin/oci" <<'STUB'
#!/usr/bin/env bash
case "$1 $2 $3" in
"compute instance list")
	query=""
	for arg in "$@"; do
		if [ "$prev" = --query ]; then query="$arg"; fi
		prev="$arg"
	done
	if [ -n "$query" ]; then
		name="$(sed -E "s/.*display-name\"=='([^']+)'.*/\1/" <<<"$query")"
		jq -r --arg n "$name" '[.data[] | select(."display-name" == $n and ."lifecycle-state" != "TERMINATED")][0].id // "null"' "$STUB_DIR/instances.json"
	else
		cat "$STUB_DIR/instances.json"
	fi
	;;
"compute instance terminate")
	iid=""
	while [ $# -gt 0 ]; do [ "$1" = --instance-id ] && iid="$2"; shift; done
	printf '%s\n' "$iid" >>"$STUB_DIR/terminated"
	[ "$iid" != "$(cat "$STUB_DIR/fail_for" 2>/dev/null)" ]
	;;
*) echo "unexpected oci call: $*" >&2; exit 1 ;;
esac
STUB
chmod +x "$work/bin/"*

vm() {
	jq -cn --arg id "$1" --arg name "$2" --arg state "$3" --arg rid "$4" --arg key "$5" '
        {id:$id,"display-name":$name,"lifecycle-state":$state,
         "freeform-tags":({} + (if $rid == "" then {} else {"github-run-id":$rid} end)
                             + (if $key == "" then {} else {"github-run-key":$key} end))}'
}
jq -s '{data:.}' >"$work/instances.json" <<EOF
$(vm ocid-a ci-eph-f1r3node-rust-amd64-a RUNNING 500 500-1)
$(vm ocid-b ci-eph-f1r3node-rust-arm64-b PROVISIONING 500 500-1)
$(vm ocid-c ci-eph-f1r3node-rust-amd64-c RUNNING 500 500-2)
$(vm ocid-d ci-eph-f1r3node-rust-amd64-d TERMINATED 500 500-1)
$(vm ocid-e soak-host RUNNING 500 500-1)
$(vm ocid-f ci-eph-f1r3node-rust-amd64-f RUNNING "" "")
$(vm ocid-g ci-eph-f1r3node-rust-amd64-g RUNNING 501 500-1)
EOF

run() {
	(cd "$work" && env PATH="$work/bin:$PATH" STUB_DIR="$work" GH_REPO=F1R3FLY-io/f1r3node-rust GH_TOKEN=t \
		CI_RUNNER_COMPARTMENT_OCID=ocid1.compartment.oc1..ci GITHUB_OUTPUT="$work/output" \
		GITHUB_STEP_SUMMARY="$work/summary" "$@")
}
reset() {
	rm -f "$work/terminated" "$work/output" "$work/summary" "$work/fail_for" "$work/gh.log"
}

for state in success failure cancelled timed_out; do
	reset
	printf '%s\n' "$state" >"$work/launch_states"
	run env RUN_ID=500 RUN_ATTEMPT=1 bash -e detect.sh >/dev/null
	grep -qx 'launched=true' "$work/output"
	grep -qx 'run_id=500' "$work/output"
	grep -q '/runs/500/attempts/1/jobs' "$work/gh.log"
done
for state in skipped ''; do
	reset
	printf '%s\n' "$state" >"$work/launch_states"
	run env RUN_ID=500 RUN_ATTEMPT=1 bash -e detect.sh >/dev/null
	grep -qx 'launched=false' "$work/output"
done
reset
set +e
run env RUN_ID='500;x' RUN_ATTEMPT=1 bash -e detect.sh >/dev/null 2>&1
bad_status=$?
set -e
[ "$bad_status" -ne 0 ]
[ ! -e "$work/gh.log" ]

reset
run env RUN_ID=500 RUN_ATTEMPT=1 DRY_RUN= bash -e terminate.sh >/dev/null
[ "$(sort "$work/terminated" | tr '\n' ' ')" = "ocid-a ocid-b " ]
grep -q 'reclaimed=2 failed=0 run_key=500-1' "$work/summary"

reset
run env RUN_ID=500 RUN_ATTEMPT=2 DRY_RUN= bash -e terminate.sh >/dev/null
[ "$(cat "$work/terminated")" = ocid-c ]

reset
out="$(run env RUN_ID=500 RUN_ATTEMPT=1 DRY_RUN=1 bash -e terminate.sh)"
[ ! -e "$work/terminated" ]
[ "$(grep -c 'Would terminate' <<<"$out")" -eq 2 ]

reset
out="$(run env RUN_ID=999 RUN_ATTEMPT=1 DRY_RUN= bash -e terminate.sh)"
grep -q 'No live VM is tagged' <<<"$out"
[ ! -e "$work/terminated" ]

reset
printf 'ocid-a' >"$work/fail_for"
set +e
run env RUN_ID=500 RUN_ATTEMPT=1 DRY_RUN= bash -e terminate.sh >/dev/null 2>&1
fail_status=$?
set -e
[ "$fail_status" -ne 0 ]
[ "$(wc -l <"$work/terminated" | tr -d ' ')" -eq 2 ]

mkdir -p "$work/system-integration/ci/oci-runners"
printf 'COMP=ocid1.compartment.oc1..ci\nREGION=us-sanjose-1\n' >"$work/system-integration/ci/oci-runners/state.env"

reset
printf '11\tci-eph-f1r3node-rust-amd64-a\tfalse\n' >"$work/runners"
out="$(run env GITHUB_RUN_ID=500 GITHUB_RUN_ATTEMPT=1 bash -e inrun.sh)"
[ "$(sort "$work/terminated" | tr '\n' ' ')" = "ocid-a ocid-b " ]
grep -q 'deregistered runner 11' <<<"$out"
grep -q 'ci-eph-f1r3node-rust-arm64-b (tagged, not reclaimed through its runner)' <<<"$out"
grep -q 'reclaim finished with 0 problem' <<<"$out"

reset
: >"$work/runners"
out="$(run env GITHUB_RUN_ID=500 GITHUB_RUN_ATTEMPT=1 bash -e inrun.sh)"
grep -q 'No runners registered' <<<"$out"
[ "$(sort "$work/terminated" | tr '\n' ' ')" = "ocid-a ocid-b " ]

reset
: >"$work/runners"
out="$(run env GITHUB_RUN_ID=777 GITHUB_RUN_ATTEMPT=1 bash -e inrun.sh)"
[ ! -e "$work/terminated" ]
grep -q 'reclaim finished with 0 problem' <<<"$out"

printf 'CI runner reclaim tests passed\n'

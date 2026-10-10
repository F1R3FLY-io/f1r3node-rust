#!/usr/bin/env bash
# Runs the "Tag launched VMs with this run" step of _integration-pipeline.yml
# against a stub oci CLI. The step must stay inline in the workflow because the
# launch job holds the OCI secrets and checks out only system-integration.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKFLOW="$ROOT/.github/workflows/_integration-pipeline.yml"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

awk '
    /^      - name: Tag launched VMs with this run$/ { step = 1; next }
    step && /^        run: \|$/ { body = 1; next }
    body && /^      - name: / { exit }
    body && /^  [a-z_]+:$/ { exit }
    body { sub(/^          /, ""); print }
' "$WORKFLOW" >"$work/step.sh"
[ -s "$work/step.sh" ]
grep -q 'github-run-key' "$work/step.sh"
bash -n "$work/step.sh"

mkdir -p "$work/bin" "$work/system-integration/ci/oci-runners"
printf 'COMP=ocid1.compartment.oc1..ci\nREGION=us-sanjose-1\n' >"$work/system-integration/ci/oci-runners/state.env"

cat >"$work/bin/oci" <<'STUB'
#!/usr/bin/env bash
case "$1 $2 $3" in
"compute instance list")
	count="$(cat "$STUB_DIR/lists" 2>/dev/null || echo 0)"
	echo $((count + 1)) >"$STUB_DIR/lists"
	while read -r name; do
		[ -e "$STUB_DIR/missing.$name" ] && continue
		if [ -e "$STUB_DIR/late.$name" ] && [ "$count" -lt 1 ]; then continue; fi
		jq -cn --arg n "$name" '{"display-name":$n,"lifecycle-state":"PROVISIONING",id:("ocid1.instance.oc1..\($n)")}'
	done <"$STUB_DIR/names" | jq -s '{data:.}'
	;;
"compute instance get")
	echo '{"cost-center":"ci"}'
	;;
"compute instance update")
	iid="" tags=""
	while [ $# -gt 0 ]; do
		case "$1" in
		--instance-id) iid="$2"; shift 2 ;;
		--freeform-tags) tags="$2"; shift 2 ;;
		*) shift ;;
		esac
	done
	printf '%s\t%s\n' "$iid" "$tags" >>"$STUB_DIR/updates"
	;;
*) echo "unexpected oci call: $*" >&2; exit 1 ;;
esac
STUB
chmod +x "$work/bin/oci"
cat >"$work/bin/sleep" <<'STUB'
#!/usr/bin/env bash
exit 0
STUB
chmod +x "$work/bin/sleep"

run_step() {
	(cd "$work" && env PATH="$work/bin:$PATH" STUB_DIR="$work" GITHUB_RUN_ID=777 GITHUB_RUN_ATTEMPT=2 \
		RUNNER_NAMES_FILE="$work/names" bash -e "$work/step.sh")
}

: >"$work/names"
out="$(run_step)"
grep -q 'nothing to tag' <<<"$out"
[ ! -e "$work/updates" ]

rm -f "$work/updates" "$work/lists"
printf '%s\n' ci-eph-f1r3node-rust-amd64-20261007-1-aa ci-eph-f1r3node-rust-arm64-20261007-1-bb >"$work/names"
touch "$work/late.ci-eph-f1r3node-rust-arm64-20261007-1-bb"
out="$(run_step)"
[ "$(wc -l <"$work/updates" | tr -d ' ')" -eq 2 ]
while IFS=$'\t' read -r iid tags; do
	[ "$(jq -r '."github-run-id"' <<<"$tags")" = 777 ]
	[ "$(jq -r '."github-run-key"' <<<"$tags")" = 777-2 ]
	[ "$(jq -r '."cost-center"' <<<"$tags")" = ci ]
	case "$iid" in ocid1.instance.oc1..ci-eph-*) ;; *) exit 1 ;; esac
done <"$work/updates"
[ "$(cat "$work/lists")" -eq 2 ]
grep -q '0 untagged' <<<"$out"

rm -f "$work/updates" "$work/lists" "$work"/late.*
printf '%s\n' "ci-eph-x'] | evil" ci-eph-f1r3node-rust-amd64-20261007-1-cc >"$work/names"
touch "$work/missing.ci-eph-f1r3node-rust-amd64-20261007-1-cc"
out="$(run_step)"
grep -q 'skip unexpected runner name' <<<"$out"
grep -q 'could not tag ci-eph-f1r3node-rust-amd64-20261007-1-cc' <<<"$out"
grep -q '1 untagged' <<<"$out"
[ ! -e "$work/updates" ]
[ "$(cat "$work/lists")" -eq 6 ]

printf 'Ephemeral run tagging step tests passed\n'

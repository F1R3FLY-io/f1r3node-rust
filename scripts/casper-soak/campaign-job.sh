#!/usr/bin/env bash
set -euo pipefail
umask 077
[[ $# == 5 ]] || exit 2
root="$(realpath -e -- "$1")"
configuration="$(realpath -e -- "$2")"
request="$(realpath -e -- "$3")"
receipt="$(realpath -e -- "$4")"
out="$5"
[[ ! -e "$out" && ! -L "$out" ]] || exit 2
mkdir -m 700 -- "$out"
out="$(realpath -e -- "$out")"
[[ "${GITHUB_REPOSITORY:-}" == F1R3FLY-io/f1r3node-rust && "${GITHUB_EVENT_NAME:-}" == workflow_dispatch && "${GITHUB_RUN_ATTEMPT:-}" == 1 ]] || exit 2
[[ "${GITHUB_RUN_ID:-}" =~ ^[1-9][0-9]{0,19}$ ]]
jq -e --arg run "$GITHUB_RUN_ID" --arg config "${CASPER_CAMPAIGN_CONFIG_SHA256:?}" '
  .schema_version==1 and .control_result=="passed" and .config_digest==$config and
  (.receipt as $r | $r.scope=="campaign-launch-control" and $r.launch_submissions==1 and
    $r.workload_admitted==false and $r.termination_confirmed==false and $r.config_digest==$config and
    ($r.slot=="preflight" or $r.slot=="baseline-dev-amd64" or $r.slot=="baseline-dev-arm64" or
      $r.slot=="stability-dev-amd64" or $r.slot=="stability-dev-arm64") and
    ($r.reservation_id|type=="string" and test("^[a-f0-9]{64}$")) and
    $r.snapshot.slots[$r.slot].run_id==$run and $r.snapshot.slots[$r.slot].reservation_id==$r.reservation_id and
    $r.snapshot.slots[$r.slot].instance_id==$r.instance_id and $r.snapshot.slots[$r.slot].state=="launched")
' "$receipt" >/dev/null
slot="$(jq -er .receipt.slot "$receipt")"
reservation="$(jq -er .receipt.reservation_id "$receipt")"
jq '.receipt.snapshot' "$receipt" > "$out/snapshot.json"
jq --arg slot "$slot" '.receipt.snapshot.slots[$slot].plan' "$receipt" > "$out/plan.json"
export CASPER_CAMPAIGN_SNAPSHOT_SHA256
CASPER_CAMPAIGN_SNAPSHOT_SHA256="$(jq -er .receipt.snapshot_sha256 "$receipt")"
bash "$root/scripts/casper-soak/campaign-host.sh" "$root" "$configuration" "$request" \
  "$out/plan.json" "$slot" "$reservation" "$out/host" "$out/snapshot.json"

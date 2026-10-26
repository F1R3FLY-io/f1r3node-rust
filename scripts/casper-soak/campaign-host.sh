#!/usr/bin/env bash
set -euo pipefail
umask 077
fail() { printf 'Campaign host rejected: %s\n' "$1" >&2; exit 2; }
[[ $# == 8 ]] || fail 'Expected a root, configuration, request, plan, slot, reservation, output directory, and state snapshot.'
root="$(realpath -e -- "$1")"
configuration="$(realpath -e -- "$2")"
request="$(realpath -e -- "$3")"
plan="$(realpath -e -- "$4")"
slot="$5" reservation="$6" out="$7"
snapshot="$(realpath -e -- "$8")"
[[ "$reservation" =~ ^[a-f0-9]{64}$ ]] || fail 'The reservation identifier is invalid.'
[[ "$slot" == preflight || "$slot" == baseline-dev-amd64 || "$slot" == baseline-dev-arm64 || "$slot" == stability-dev-amd64 || "$slot" == stability-dev-arm64 ]] || fail 'The reservation slot is invalid.'
[[ ! -e "$out" && ! -L "$out" ]] || fail 'The output directory already exists.'
mkdir -m 700 -- "$out"
out="$(realpath -e -- "$out")"
image_container=""
started_epoch="$(date +%s)"
finish_host() {
  local status=$? finished_epoch protection=passed
  trap - EXIT
  finished_epoch="$(date +%s)"
  if [[ -n "$image_container" ]]; then docker rm "$image_container" >/dev/null || true; fi
  if [[ -f "/var/lib/casper/$reservation/guard-failure.json" ]]; then
    cp "/var/lib/casper/$reservation/guard-failure.json" "$out/guard-failure.json"
    protection=failed
  fi
  systemctl --user is-active --quiet "casper-$reservation-guardian.service" || protection=failed
  [[ "$(systemctl --user show "casper-$reservation.slice" --property MemoryMax --value)" == 47244640256 ]] || protection=failed
  local adapter="$out/workload/result.json" admission="$out/admission.json"
  [[ -f "$adapter" && ! -L "$adapter" && "$(stat -c %s "$adapter")" -le 16384 ]] || adapter=/dev/null
  [[ -f "$admission" ]] || admission=/dev/null
  jq -n --slurpfile a "$adapter" --slurpfile p "$plan" --slurpfile s "$snapshot" --slurpfile h "$admission" \
    --arg slot "$slot" --arg reservation "$reservation" --arg run "$GITHUB_RUN_ID" \
    --arg protection "$protection" --argjson status "$status" --argjson start "$started_epoch" --argjson finish "$finished_epoch" \
    --arg digest "$(jq -cS . "$plan" | sha256sum | cut -d ' ' -f1)" \
    '($a[0]//{}) as $a | $s[0].slots[$slot] as $s |
     {schema_version:1,repository:"F1R3FLY-io/f1r3node-rust",run_id:$run,run_attempt:1,
      reservation_id:$reservation,config_digest:$s.config_digest,plan_sha256:$digest,
      instance_id:$s.instance_id,runner_label:("casper-"+$reservation),admission:($h[0].receipt.admission//"rejected"),
      host_protection:$protection,evidence_kind:($a.evidence_kind//"incomplete"),node_launch_count:($a.node_launch_count//0),
      started_epoch:$start,finished_epoch:$finish,workload_elapsed_seconds:($a.workload_elapsed_seconds//0),
      integration_preflight:($a.integration_preflight//"incomplete"),measurement_completeness:($a.measurement_completeness//"incomplete"),
      profile_verdicts:($a.profile_verdicts//{}),product_failures:($a.product_failures//[]),
      result:(if $status==0 and $protection=="passed" and $a.result=="passed" then "passed" else "failed" end)}' > "$out/worker-result.json"
  exit "$status"
}
trap finish_host EXIT
[[ "${RUNNER_NAME:-}" == "casper-$reservation" ]] || fail 'The runner identity is not exclusive.'
[[ "${DOCKER_HOST:-}" == "unix:///run/user/$(id -u)/casper-$reservation/docker.sock" ]] || fail 'The private Docker socket is missing.'
docker info --format '{{json .SecurityOptions}}' | jq -e 'any(.[];contains("rootless"))' >/dev/null || fail 'The Docker daemon is not rootless.'
slice="casper-$reservation.slice"
cgroup="$(systemctl --user show "$slice" --property ControlGroup --value)"
[[ "$cgroup" == /user.slice/*/"$slice" ]] || fail 'The campaign cgroup is invalid.'
[[ "$(systemctl --user show "$slice" --property MemoryMax --value)" == 47244640256 ]] || fail 'The memory ceiling is not active.'
[[ "$(systemctl --user show "$slice" --property MemorySwapMax --value)" == 0 ]] || fail 'The swap limit is not active.'
for service in "casper-$reservation-docker.service" "casper-$reservation-guardian.service"; do
  systemctl --user is-active --quiet "$service" || fail 'A required host service is inactive.'
done
daemon_pid="$(systemctl --user show "casper-$reservation-docker.service" --property MainPID --value)"
[[ "$daemon_pid" =~ ^[1-9][0-9]*$ ]] || fail 'The Docker daemon PID is invalid.'
actual_cgroup="$(awk -F: '$1=="0"{print $3}' "/proc/$daemon_pid/cgroup")"
[[ "$actual_cgroup" == "$cgroup/"* ]] || fail 'The Docker daemon escaped the campaign memory ceiling.'
receipt="/run/casper/$reservation/host.json"
[[ -f "$receipt" && ! -L "$receipt" && "$(stat -c %u "$receipt")" == 0 && "$(stat -c %a "$receipt")" == 444 ]] || fail 'The trusted bootstrap receipt is missing.'
jq -e --arg reservation "$reservation" '.reservation_id==$reservation and .metadata_access_blocked==true and .runner_exclusive==true' "$receipt" >/dev/null || fail 'The bootstrap protection receipt differs.'
if timeout 3 curl -fsS -H 'Authorization: Bearer Oracle' http://169.254.169.254/opc/v2/instance/ >/dev/null 2>&1; then
  fail 'The workload user can access instance metadata.'
fi
image="$(jq -er .image_reference "$plan")"
platform="$(jq -er .platform "$plan")"
[[ "$image" =~ ^docker.io/f1r3flyindustries/f1r3fly-rust@sha256:[a-f0-9]{64}$ ]] || fail 'The container image is mutable.'
docker pull --platform "$platform" "$image" > "$out/image-pull.log" 2>&1
docker image inspect "$image" > "$out/image.json"
jq -e --arg platform "$platform" --arg digest "$(jq -r .image_config_digest "$plan")" \
  'length==1 and (.[0].Os+"/"+.[0].Architecture)==$platform and .[0].Id==$digest' "$out/image.json" >/dev/null || fail 'The container identity differs.'
image_container="$(docker create --network none --entrypoint /bin/true "$image")"
docker cp "$image_container:/opt/docker/bin/node" "$out/node"
binary_digest="sha256:$(sha256sum "$out/node" | cut -d ' ' -f1)"
[[ "$binary_digest" == "$(jq -r .node_binary_digest "$plan")" ]] || fail 'The node executable differs.'
memory_total="$(awk '/^MemTotal:/{print int($2/1024)}' /proc/meminfo)"
memory_available="$(awk '/^MemAvailable:/{print int($2/1024)}' /proc/meminfo)"
disk_free="$(df -Pm -- "$root" | awk 'NR==2{print $4}')"
jq -n --slurpfile r "$receipt" --slurpfile p "$plan" --arg reservation "$reservation" \
  --arg label "$RUNNER_NAME" --arg binary "$binary_digest" \
  --argjson total "$memory_total" --argjson available "$memory_available" --argjson disk "$disk_free" \
  '{instance_id:$r[0].instance_id,reservation_id:$reservation,platform:$p[0].platform,
    image_digest:$p[0].image_digest,image_config_digest:$p[0].image_config_digest,node_binary_digest:$binary,
    runner_label:$label,runner_exclusive:true,memory_total_mib:$total,memory_available_mib:$available,
    memory_max_bytes:47244640256,memory_swap_max_bytes:0,disk_free_mib:$disk,disk_floor_mib:4096,
    disk_guardian_active:true,memory_guardian_active:true,metadata_access_blocked:true}' > "$out/observations.json"
"$root/target/debug/casper-campaign-control" host-admission --root "$root" --config "$configuration" \
  --request "$request" --plan "$plan" --run "$GITHUB_RUN_ID" --slot "$slot" \
  --snapshot "$snapshot" --snapshot-sha256 "${CASPER_CAMPAIGN_SNAPSHOT_SHA256:?}" \
  --observations "$out/observations.json" --evidence "$out/admission" > "$out/admission.json"
workload_path="$(jq -er .workload.path "$plan")"
workload="$(realpath -e -- "$root/$workload_path")"
[[ "$workload" == "$root/"* && "$(sha256sum "$workload" | cut -d ' ' -f1)" == "$(jq -r .workload.sha256 "$plan")" ]] || fail 'The workload differs from its pin.'
entrypoint_path="$(jq -er .entrypoint.path "$workload")"
entrypoint="$(realpath -e -- "$root/$entrypoint_path")"
[[ "$entrypoint" == "$root/"* && "$(sha256sum "$entrypoint" | cut -d ' ' -f1)" == "$(jq -r .entrypoint.sha256 "$workload")" ]] || fail 'The workload adapter differs from its pin.'
duration="$(jq -er .duration_seconds "$plan")"
deadline="$(jq -er .receipt.workload_deadline_epoch "$out/admission.json")"
mkdir -m 700 "$out/workload-home"
started_epoch="$(date +%s)"
systemd-run --user --wait --collect --unit "casper-$reservation-workload" --slice "$slice" \
  --property MemoryMax=45056M --property MemorySwapMax=0 \
  env -i PATH="$PATH" DOCKER_HOST="$DOCKER_HOST" HOME="$out/workload-home" \
  F1R3FLY_NODE_IMAGE="$image" SOAK_DISK_FREE_FLOOR_MB=4096 SOAK_DISK_HYGIENE_BAND_MB=4096 \
  CASPER_CAMPAIGN_STAGE="$(jq -er .stage "$plan")" CASPER_CAMPAIGN_DURATION_SECONDS="$duration" CASPER_CAMPAIGN_DEADLINE_EPOCH="$deadline" \
  bash "$entrypoint" "$workload" "$out/workload"

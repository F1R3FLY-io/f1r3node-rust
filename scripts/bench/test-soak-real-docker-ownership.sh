#!/usr/bin/env bash
set -euo pipefail

SOURCE="${1:?source directory is required}"
EVIDENCE="${2:?evidence directory is required}"
IMAGE="${SOAK_REAL_DOCKER_IMAGE:?an immutable fixture image is required}"
ACK="${SOAK_REAL_DOCKER_ACK:?the disposable instance identity is required}"
[[ -f /opt/d2-provisioning.json && -s /opt/d2-ready && "$IMAGE" == *@sha256:* ]] || exit 2
INSTANCE="$(/usr/bin/curl -fsS --connect-timeout 3 --max-time 10 -H 'Authorization: Bearer Oracle' http://169.254.169.254/opc/v2/instance/id)"
[[ "$INSTANCE" == "$ACK" && "$INSTANCE" == ocid1.instance.oc1.us-sanjose-1.* ]] || exit 2
jq -e '.purpose == "D2 isolated diagnostic runner" and .github_registration == false' /opt/d2-provisioning.json >/dev/null
[[ -z "${DOCKER_HOST:-}" && "$(docker context show)" == default ]] || exit 2
[[ "$(docker context inspect default --format '{{.Endpoints.docker.Host}}')" == unix:///var/run/docker.sock ]] || exit 2
[[ ! -e "$EVIDENCE" ]] || exit 2
mkdir -p "$EVIDENCE/bin" "$EVIDENCE/harness" "$EVIDENCE/tmp" "$EVIDENCE/runner"
EVIDENCE="$(cd "$EVIDENCE" && pwd)"
SOURCE="$(cd "$SOURCE" && pwd)"
SESSION="d2-ownership-$(date +%s)-$$"
CONTAINER=""
NETWORK=""
REQUIRED_IMAGE=""
cleanup() {
    [[ -z "$CONTAINER" ]] || /usr/bin/docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
    [[ -z "$NETWORK" ]] || /usr/bin/docker network rm "$NETWORK" >/dev/null 2>&1 || true
    [[ -z "$REQUIRED_IMAGE" ]] || /usr/bin/docker image rm "$REQUIRED_IMAGE" >/dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'printf "ERROR: The real-Docker fixture failed before its behavioral verdict.\n" >&2; exit 2' ERR
printf '%s\n' "$INSTANCE" >"$EVIDENCE/instance-id.txt"
/usr/bin/docker version --format '{{json .}}' >"$EVIDENCE/docker-version.json"
/usr/bin/df -Pm / >"$EVIDENCE/host-disk-before.txt"
(cd "$SOURCE" && sha256sum scripts/run-merge-recovery-soak.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json) >"$EVIDENCE/source-sha256.txt"
CONTAINER="$(/usr/bin/docker create --name "rnode.$SESSION" --label fixture.owner=unrelated --network none --memory 32m --cpus 0.5 --pids-limit 32 --cap-drop ALL --security-opt no-new-privileges --user 65534:65534 "$IMAGE" /bin/sh -c 'printf retained-data >/tmp/retained-data')"
/usr/bin/docker start "$CONTAINER" >/dev/null
[[ "$(/usr/bin/docker wait "$CONTAINER")" == 0 ]]
NETWORK="$(/usr/bin/docker network create --label fixture.owner=unrelated "$SESSION")"
/usr/bin/docker export "$CONTAINER" >"$EVIDENCE/required-rootfs.tar"
REQUIRED_IMAGE="$(/usr/bin/docker import --change 'CMD ["/bin/true"]' "$EVIDENCE/required-rootfs.tar")"
/usr/bin/docker inspect "$CONTAINER" >"$EVIDENCE/container-before.json"
/usr/bin/docker network inspect "$NETWORK" >"$EVIDENCE/network-before.json"
/usr/bin/docker image inspect "$REQUIRED_IMAGE" >"$EVIDENCE/image-before.json"
[[ "$(/usr/bin/docker inspect -f '{{.State.Status}}' "$CONTAINER")" == exited ]]
/usr/bin/docker image ls --no-trunc --filter dangling=true --format '{{.ID}}' >"$EVIDENCE/dangling-images-before.txt"
grep -Fx "$REQUIRED_IMAGE" "$EVIDENCE/dangling-images-before.txt" >/dev/null
cat >"$EVIDENCE/bin/df" <<'SH'
#!/usr/bin/env bash
printf '7000\n' >>"$SOAK_REAL_EVIDENCE/probe-samples.txt"
printf 'Filesystem 1M-blocks Used Available Capacity Mounted on\n/dev/fixture 47000 40000 7000 85%% /\n'
SH
cat >"$EVIDENCE/bin/docker" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$SOAK_REAL_EVIDENCE/docker-commands.txt"
exec /usr/bin/docker "$@"
SH
cat >"$EVIDENCE/bin/poetry" <<'SH'
#!/usr/bin/env bash
printf 'unexpected workload\n' >"$SOAK_REAL_EVIDENCE/workload-started.txt"
exit 1
SH
cat >"$EVIDENCE/bin/oci" <<'SH'
#!/usr/bin/env bash
exit 1
SH
chmod +x "$EVIDENCE/bin/df" "$EVIDENCE/bin/docker" "$EVIDENCE/bin/poetry" "$EVIDENCE/bin/oci"
status=0
PATH="$EVIDENCE/bin:$PATH" SOAK_REAL_EVIDENCE="$EVIDENCE" \
    SOAK_DURATION_SECONDS=30 SYSTEM_INTEGRATION_DIR="$EVIDENCE/harness" \
    SOAK_OUTPUT_DIR="$EVIDENCE/output" SOAK_TMP_ROOT="$EVIDENCE/tmp" \
    SOAK_RUNNER_ROOT="$EVIDENCE/runner" SOAK_RUN_BENCHMARKS=false \
    SOAK_RSS_CEILING_MB=0 SOAK_HOST_FREE_FLOOR_MB=0 \
    SOAK_DISK_FREE_FLOOR_MB=4096 SOAK_DISK_HYGIENE_BAND_MB=4096 \
    SOAK_DISK_HYGIENE_SECONDS=10 SOAK_DISK_DIAGNOSTIC_SECONDS=1 \
    SOAK_DISK_STOP_SECONDS=1 SOAK_GUARDIAN_POLL_SECONDS=0.1 \
    timeout --signal=TERM --kill-after=2 40 bash "$SOURCE/scripts/run-merge-recovery-soak.sh" \
    >"$EVIDENCE/driver.log" 2>&1 || status=$?
printf '%s\n' "$status" >"$EVIDENCE/driver-exit.txt"
container_present=false
network_present=false
image_present=false
if /usr/bin/docker inspect "$CONTAINER" >"$EVIDENCE/container-after.json" 2>"$EVIDENCE/container-after.stderr"; then container_present=true; fi
if /usr/bin/docker network inspect "$NETWORK" >"$EVIDENCE/network-after.json" 2>"$EVIDENCE/network-after.stderr"; then network_present=true; fi
if /usr/bin/docker image inspect "$REQUIRED_IMAGE" >"$EVIDENCE/image-after.json" 2>"$EVIDENCE/image-after.stderr"; then image_present=true; fi
jq -n --arg container "$CONTAINER" --arg network "$NETWORK" --arg image "$REQUIRED_IMAGE" \
    --argjson container_present "$container_present" --argjson network_present "$network_present" --argjson image_present "$image_present" \
    '{container:$container,network:$network,image:$image,container_preserved:$container_present,network_preserved:$network_present,image_preserved:$image_present}' >"$EVIDENCE/observations.json"
/usr/bin/df -Pm / >"$EVIDENCE/host-disk-after.txt"
[[ "$status" == 1 && ! -e "$EVIDENCE/workload-started.txt" ]]
jq -e '.iterations == 0 and .failures == 1' "$EVIDENCE/output/summary.json" >/dev/null
grep -F 'inside hygiene band' "$EVIDENCE/driver.log" >/dev/null
trap - ERR
if [[ "$container_present" != true || "$network_present" != true || "$image_present" != true ]]; then
    printf 'FAIL: Disk hygiene deleted an unrelated Docker resource or the reserved image.\n' >&2
    exit 1
fi
printf 'PASS: Disk hygiene preserved unrelated Docker resources and the reserved image, then refused new work.\n'

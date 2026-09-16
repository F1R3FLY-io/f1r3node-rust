#!/usr/bin/env bash
set -euo pipefail

SOURCE="${1:?source directory is required}"
EVIDENCE="${2:?evidence directory is required}"
IMAGE="${SOAK_REAL_DOCKER_IMAGE:?an immutable fixture image is required}"
ACK="${SOAK_REAL_DOCKER_ACK:?the disposable instance identity is required}"
[[ -f /opt/d2-provisioning.json && -s /opt/d2-ready && "$IMAGE" =~ @sha256:[a-f0-9]{64}$ ]] || exit 2
INSTANCE="$(/usr/bin/curl -fsS --connect-timeout 3 --max-time 10 -H 'Authorization: Bearer Oracle' http://169.254.169.254/opc/v2/instance/id)"
[[ "$INSTANCE" == "$ACK" && "$INSTANCE" == ocid1.instance.oc1.us-sanjose-1.* ]] || exit 2
jq -e '.purpose == "D2 isolated diagnostic runner" and .github_registration == false' /opt/d2-provisioning.json >/dev/null
[[ -z "${DOCKER_HOST:-}" && "$(docker context show)" == default ]] || exit 2
[[ "$(docker context inspect default --format '{{.Endpoints.docker.Host}}')" == unix:///var/run/docker.sock ]] || exit 2
[[ ! -e "$EVIDENCE" ]] || exit 2
mkdir -p "$EVIDENCE/bin" "$EVIDENCE/harness" "$EVIDENCE/tmp" "$EVIDENCE/runner"
EVIDENCE="$(cd "$EVIDENCE" && pwd)"
SOURCE="$(cd "$SOURCE" && pwd)"
DRIVER_PID=""
cleanup() {
    [[ -z "$DRIVER_PID" ]] || kill -KILL -- "-$DRIVER_PID" 2>/dev/null || true
    if [[ -s "$EVIDENCE/writer-id.txt" ]]; then
        local cid
        cid="$(<"$EVIDENCE/writer-id.txt")"
        [[ "$cid" =~ ^[a-f0-9]{64}$ ]] || return 0
        /usr/bin/docker rm -f "$cid" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT
trap 'printf "ERROR: The failed-stop fixture failed before its behavioral verdict.\n" >&2; exit 2' ERR
printf '%s\n' "$INSTANCE" >"$EVIDENCE/instance-id.txt"
(cd "$SOURCE" && sha256sum scripts/run-merge-recovery-soak.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json) >"$EVIDENCE/source-sha256.txt"
cat >"$EVIDENCE/bin/poetry" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
trap 'exit 143' TERM INT
cid=$(docker run -d --name "rnode.test.d2-stop-failure-$$" --label fixture.owner=workload --network none --memory 32m --cpus 0.5 --pids-limit 32 --cap-drop ALL --security-opt no-new-privileges --user 65534:65534 "$SOAK_REAL_DOCKER_IMAGE" /bin/sh -c 'i=0; while [ "$i" -lt 600 ]; do printf "%s\n" "$i" >>/tmp/writes; i=$((i+1)); sleep 0.1; done')
printf '%s\n' "$cid" >"$SOAK_REAL_EVIDENCE/writer-id.txt"
while :; do sleep 0.1; done
SH
cat >"$EVIDENCE/bin/docker" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == kill ]]; then
    printf '%s\n' "$*" >>"$SOAK_REAL_EVIDENCE/rejected-stop.txt"
    printf 'The fixture rejected the Docker stop command.\n' >&2
    exit 42
fi
exec /usr/bin/docker "$@"
SH
cat >"$EVIDENCE/bin/oci" <<'SH'
#!/usr/bin/env bash
exit 1
SH
chmod +x "$EVIDENCE/bin/poetry" "$EVIDENCE/bin/docker" "$EVIDENCE/bin/oci"
PATH="$EVIDENCE/bin:$PATH" SOAK_REAL_EVIDENCE="$EVIDENCE" SOAK_REAL_DOCKER_IMAGE="$IMAGE" \
    SOAK_DURATION_SECONDS=120 SYSTEM_INTEGRATION_DIR="$EVIDENCE/harness" \
    SOAK_OUTPUT_DIR="$EVIDENCE/output" SOAK_TMP_ROOT="$EVIDENCE/tmp" \
    SOAK_RUNNER_ROOT="$EVIDENCE/runner" SOAK_RUN_BENCHMARKS=false \
    SOAK_RSS_CEILING_MB=0 SOAK_HOST_FREE_FLOOR_MB=0 \
    SOAK_DISK_FREE_FLOOR_MB=4096 SOAK_DISK_HYGIENE_BAND_MB=4096 \
    SOAK_DISK_STOP_SECONDS=2 SOAK_GUARDIAN_POLL_SECONDS=0.1 SOAK_MONITOR_SNAPSHOT_SECONDS=0.1 \
    setsid bash "$SOURCE/scripts/run-merge-recovery-soak.sh" >"$EVIDENCE/driver.log" 2>&1 &
DRIVER_PID=$!
for _ in $(seq 1 100); do
    [[ ! -s "$EVIDENCE/writer-id.txt" ]] || break
    kill -0 "$DRIVER_PID"
    sleep 0.1
done
[[ -s "$EVIDENCE/writer-id.txt" ]]
CID="$(<"$EVIDENCE/writer-id.txt")"
[[ "$CID" =~ ^[a-f0-9]{64}$ ]]
/usr/bin/docker inspect "$CID" >"$EVIDENCE/writer-before.json"
jq -e '.[0].State | .Running == true and .Pid > 0' "$EVIDENCE/writer-before.json" >/dev/null
date -u +%FT%TZ >"$EVIDENCE/signal-at.txt"
kill -TERM "$DRIVER_PID"
for _ in $(seq 1 150); do
    kill -0 "$DRIVER_PID" 2>/dev/null || break
    sleep 0.1
done
if kill -0 "$DRIVER_PID" 2>/dev/null; then
    printf 'ERROR: The driver exceeded the failed-stop observation deadline.\n' >&2
    exit 2
fi
status=0
wait "$DRIVER_PID" || status=$?
printf '%s\n' "$status" >"$EVIDENCE/driver-exit.txt"
DRIVER_PID=""
[[ -s "$EVIDENCE/rejected-stop.txt" ]]
/usr/bin/docker inspect "$CID" >"$EVIDENCE/writer-after.json"
jq -e '.[0].State | .Running == true and .Pid > 0' "$EVIDENCE/writer-after.json" >/dev/null
cp "$EVIDENCE/output/.soak-state" "$EVIDENCE/state-after-stop.txt"
date -u +%FT%TZ >"$EVIDENCE/observed-at.txt"
trap - ERR
if [[ "$status" == 0 ]] ||
    [[ "$(awk -F= '$1 == "FAILURES" {print $2}' "$EVIDENCE/state-after-stop.txt")" != 1 ]] ||
    [[ "$(awk -F= '$1 == "INFLIGHT_ITERATION" {print $2}' "$EVIDENCE/state-after-stop.txt")" != 2 ]] ||
    ! grep -qF 'Writer termination is unconfirmed' "$EVIDENCE/output/writer-stop-failure.txt" 2>/dev/null ||
    ! grep -qF 'writer_stop_failed' "$EVIDENCE/output/early-exit.txt" 2>/dev/null; then
    printf 'FAIL: Driver exit lost the rejected Docker stop and its unconfirmed writer termination.\n' >&2
    exit 1
fi
printf 'PASS: Driver exit retained the rejected Docker stop, one interruption failure, and unconfirmed writer termination.\n'

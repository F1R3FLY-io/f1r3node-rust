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
DRIVER_PID=""
cleanup() {
    [[ -z "$DRIVER_PID" ]] || kill -KILL -- "-$DRIVER_PID" 2>/dev/null || true
    if [[ -s "$EVIDENCE/writer-id.txt" ]]; then
        /usr/bin/docker rm -f "$(<"$EVIDENCE/writer-id.txt")" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT
trap 'printf "ERROR: The crash fixture failed before its behavioral verdict.\n" >&2; exit 2' ERR
printf '%s\n' "$INSTANCE" >"$EVIDENCE/instance-id.txt"
(cd "$SOURCE" && sha256sum scripts/run-merge-recovery-soak.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json) >"$EVIDENCE/source-sha256.txt"
cat >"$EVIDENCE/bin/poetry" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$SOAK_CRASH_STAGE" == recovery ]]; then
    printf 'new work after crash\n' >>"$SOAK_REAL_EVIDENCE/recovery-workload.txt"
    exit 0
fi
cid=$(docker run -d --name "rnode.test.d2-crash-$$" --label fixture.owner=workload --network none --memory 32m --cpus 0.5 --pids-limit 32 --cap-drop ALL --security-opt no-new-privileges --user 65534:65534 "$SOAK_REAL_DOCKER_IMAGE" /bin/sh -c 'i=0; while [ "$i" -lt 600 ]; do printf "%s\n" "$i" >>/tmp/writes; i=$((i+1)); sleep 0.1; done')
printf '%s\n' "$cid" >"$SOAK_REAL_EVIDENCE/writer-id.txt"
while :; do sleep 0.1; done
SH
cat >"$EVIDENCE/bin/oci" <<'SH'
#!/usr/bin/env bash
exit 1
SH
chmod +x "$EVIDENCE/bin/poetry" "$EVIDENCE/bin/oci"
export PATH="$EVIDENCE/bin:$PATH" SOAK_REAL_EVIDENCE="$EVIDENCE" SOAK_REAL_DOCKER_IMAGE="$IMAGE"
export SOAK_DURATION_SECONDS=120 SYSTEM_INTEGRATION_DIR="$EVIDENCE/harness"
export SOAK_OUTPUT_DIR="$EVIDENCE/output" SOAK_TMP_ROOT="$EVIDENCE/tmp" SOAK_RUNNER_ROOT="$EVIDENCE/runner"
export SOAK_RUN_BENCHMARKS=false SOAK_RSS_CEILING_MB=0 SOAK_HOST_FREE_FLOOR_MB=0
export SOAK_DISK_FREE_FLOOR_MB=4096 SOAK_DISK_HYGIENE_BAND_MB=4096
export SOAK_DISK_STOP_SECONDS=2 SOAK_GUARDIAN_POLL_SECONDS=0.1 SOAK_MONITOR_SNAPSHOT_SECONDS=0.1
SOAK_CRASH_STAGE=work setsid bash "$SOURCE/scripts/run-merge-recovery-soak.sh" >"$EVIDENCE/driver-first.log" 2>&1 &
DRIVER_PID=$!
for _ in $(seq 1 100); do
    [[ ! -s "$EVIDENCE/writer-id.txt" ]] || break
    kill -0 "$DRIVER_PID"
    sleep 0.1
done
[[ -s "$EVIDENCE/writer-id.txt" ]]
CID="$(<"$EVIDENCE/writer-id.txt")"
/usr/bin/docker inspect "$CID" >"$EVIDENCE/writer-before-crash.json"
jq -e '.[0].State.Running == true' "$EVIDENCE/writer-before-crash.json" >/dev/null
cp "$EVIDENCE/output/.soak-state" "$EVIDENCE/state-before-crash.txt"
date -u +%FT%TZ >"$EVIDENCE/crash-at.txt"
kill -KILL -- "-$DRIVER_PID"
status=0
wait "$DRIVER_PID" || status=$?
printf '%s\n' "$status" >"$EVIDENCE/crashed-driver-exit.txt"
[[ "$status" == 137 ]]
/usr/bin/docker inspect "$CID" >"$EVIDENCE/writer-after-crash.json"
/usr/bin/docker kill "$CID" >"$EVIDENCE/fixture-supervisor-stop.txt"
/usr/bin/docker inspect "$CID" >"$EVIDENCE/writer-after-supervisor.json"
jq -e '.[0].State | .Running == false and .Pid == 0' "$EVIDENCE/writer-after-supervisor.json" >/dev/null
for attempt in 1 2; do
    status=0
    SOAK_CRASH_STAGE=recovery SOAK_DEADLINE_EPOCH="$(($(date +%s) + 3))" \
        timeout --signal=TERM --kill-after=2 20 bash "$SOURCE/scripts/run-merge-recovery-soak.sh" \
        >"$EVIDENCE/recovery-$attempt.log" 2>&1 || status=$?
    printf '%s\n' "$status" >"$EVIDENCE/recovery-$attempt-exit.txt"
    cp "$EVIDENCE/output/.soak-state" "$EVIDENCE/recovery-$attempt-state.txt"
    cp "$EVIDENCE/output/summary.json" "$EVIDENCE/recovery-$attempt-summary.json"
done
trap - ERR
if [[ -e "$EVIDENCE/recovery-workload.txt" ]] ||
    [[ "$(<"$EVIDENCE/recovery-1-exit.txt")" != 1 || "$(<"$EVIDENCE/recovery-2-exit.txt")" != 1 ]] ||
    ! jq -e '.iterations == 1 and .failures == 1' "$EVIDENCE/recovery-1-summary.json" >/dev/null ||
    ! jq -e '.iterations == 1 and .failures == 1' "$EVIDENCE/recovery-2-summary.json" >/dev/null; then
    printf 'FAIL: Restart admitted work or lost the interrupted iteration after the driver crash.\n' >&2
    exit 1
fi
printf 'PASS: Two restarts refused work and retained one interrupted-iteration failure after the driver crash.\n'

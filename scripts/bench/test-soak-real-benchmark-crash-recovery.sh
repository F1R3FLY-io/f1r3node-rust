#!/usr/bin/env bash
set -euo pipefail

SOURCE="${1:?source directory is required}"
EVIDENCE="${2:?new evidence directory is required}"
IMAGE="${SOAK_REAL_DOCKER_IMAGE:?an immutable fixture image is required}"
ACK="${SOAK_REAL_DOCKER_ACK:?the disposable instance identity is required}"
[[ -f /opt/d2-provisioning.json && -s /opt/d2-ready && "$IMAGE" =~ @sha256:[a-f0-9]{64}$ ]] || exit 2
INSTANCE="$(/usr/bin/curl -fsS --connect-timeout 3 --max-time 10 -H 'Authorization: Bearer Oracle' http://169.254.169.254/opc/v2/instance/id)"
[[ "$INSTANCE" == "$ACK" && "$INSTANCE" == ocid1.instance.oc1.us-sanjose-1.* ]] || exit 2
jq -e '.purpose == "D2 isolated diagnostic runner" and .github_registration == false' /opt/d2-provisioning.json >/dev/null
[[ -z "${DOCKER_HOST:-}" && "$(docker context show)" == default ]] || exit 2
[[ "$(docker context inspect default --format '{{.Endpoints.docker.Host}}')" == unix:///var/run/docker.sock ]] || exit 2
[[ ! -e "$EVIDENCE" && -z "$(/usr/bin/docker ps -aq --filter label=com.docker.compose.project=soak-bench)" ]] || exit 2
mkdir -p "$EVIDENCE/bin" "$EVIDENCE/harness" "$EVIDENCE/tmp" "$EVIDENCE/runner" "$EVIDENCE/node/docker"
EVIDENCE="$(cd "$EVIDENCE" && pwd)"
SOURCE="$(cd "$SOURCE" && pwd)"
cd "$EVIDENCE"
DRIVER_PID=""
FIXTURE_GROUP="$(ps -o pgid= -p "$$" | tr -d ' ')"
stop_fixture_clients() {
    local file group
    for file in "$EVIDENCE"/*-client-group.txt; do
        [[ -f "$file" ]] || continue
        group="$(<"$file")"
        [[ "$group" =~ ^[1-9][0-9]*$ && "$group" != "$FIXTURE_GROUP" ]] || return 1
        kill -KILL -- "-$group" 2>/dev/null || true
    done
}
cleanup() {
    [[ -z "$DRIVER_PID" ]] || kill -KILL -- "-$DRIVER_PID" 2>/dev/null || true
    stop_fixture_clients || true
    local file cid
    for file in "$EVIDENCE"/*-created-ids.txt; do
        [[ -f "$file" ]] || continue
        while IFS= read -r cid; do
            [[ "$cid" =~ ^[a-f0-9]{64}$ ]] || continue
            /usr/bin/docker rm -f "$cid" >/dev/null 2>&1 || true
        done <"$file"
    done
}
trap cleanup EXIT
trap 'printf "ERROR: The benchmark crash fixture failed before its behavioral verdict.\n" >&2; exit 2' ERR
printf '%s\n' "$INSTANCE" >"$EVIDENCE/instance-id.txt"
(cd "$SOURCE" && sha256sum scripts/run-merge-recovery-soak.sh scripts/bench/run-bench-segment.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json) >"$EVIDENCE/source-sha256.txt"
jq -n --arg image "$IMAGE" '{services:{writer:{image:$image,labels:{"fixture.owner":"benchmark-crash"},network_mode:"none",mem_limit:33554432,cpus:0.5,pids_limit:32,cap_drop:["ALL"],security_opt:["no-new-privileges"],user:"65534:65534",command:["/bin/sh","-c","i=0; while [ \"$$i\" -lt 1200 ]; do printf \"%s\\n\" \"$$i\" >>/tmp/writes; i=$$((i+1)); sleep 0.1; done"]}}}' >"$EVIDENCE/node/docker/shard.yml"
cat >"$EVIDENCE/bin/docker" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == compose && " $* " == *' up '* ]]; then
    ps -o pgid= -p "$PPID" | tr -d ' ' >"$SOAK_REAL_EVIDENCE/$SOAK_FIXTURE_PHASE-client-group.txt"
    ps -o sid= -p "$PPID" | tr -d ' ' >"$SOAK_REAL_EVIDENCE/$SOAK_FIXTURE_PHASE-client-session.txt"
    /usr/bin/docker "$@"
    /usr/bin/docker ps -q --no-trunc --filter label=com.docker.compose.project=soak-bench >>"$SOAK_REAL_EVIDENCE/$SOAK_FIXTURE_PHASE-created-ids.txt"
    exit 0
fi
exec /usr/bin/docker "$@"
SH
cat >"$EVIDENCE/bin/curl" <<'SH'
#!/usr/bin/env bash
exit 1
SH
cp "$EVIDENCE/bin/curl" "$EVIDENCE/bin/oci"
cat >"$EVIDENCE/bin/poetry" <<'SH'
#!/usr/bin/env bash
printf 'An iteration was invoked in the benchmark crash fixture.\n' >>"$SOAK_REAL_EVIDENCE/iteration-invoked.txt"
exit 42
SH
chmod +x "$EVIDENCE/bin/"*
launch_driver() {
    local phase="$1"
    PATH="$EVIDENCE/bin:$PATH" SOAK_REAL_EVIDENCE="$EVIDENCE" SOAK_FIXTURE_PHASE="$phase" \
        DEPLOYER_KEY=fixture-unused SOAK_NODE_REPO_DIR="$EVIDENCE/node" \
        SOAK_DURATION_SECONDS=1200 SOAK_RUN_BENCHMARKS=true SOAK_BENCH_DURATION=1 \
        SYSTEM_INTEGRATION_DIR="$EVIDENCE/harness" SOAK_OUTPUT_DIR="$EVIDENCE/output" \
        SOAK_TMP_ROOT="$EVIDENCE/tmp" SOAK_RUNNER_ROOT="$EVIDENCE/runner" \
        SOAK_RSS_CEILING_MB=0 SOAK_HOST_FREE_FLOOR_MB=0 \
        SOAK_DISK_FREE_FLOOR_MB=4096 SOAK_DISK_HYGIENE_BAND_MB=4096 \
        SOAK_DISK_STOP_SECONDS=2 SOAK_GUARDIAN_POLL_SECONDS=0.1 \
        setsid bash "$SOURCE/scripts/run-merge-recovery-soak.sh" >"$EVIDENCE/$phase-driver.log" 2>&1 &
    DRIVER_PID=$!
}
launch_driver initial
for _ in $(seq 1 150); do
    [[ ! -s "$EVIDENCE/initial-created-ids.txt" ]] || break
    kill -0 "$DRIVER_PID"
    sleep 0.1
done
[[ "$(wc -l <"$EVIDENCE/initial-created-ids.txt")" == 1 ]]
[[ "$(<"$EVIDENCE/initial-client-session.txt")" == "$DRIVER_PID" ]]
CID="$(<"$EVIDENCE/initial-created-ids.txt")"
[[ "$CID" =~ ^[a-f0-9]{64}$ ]]
/usr/bin/docker inspect "$CID" >"$EVIDENCE/writer-before.json"
jq -e '.[0] | .State.Running == true and .State.Pid > 0 and (.Mounts | length) == 0 and .HostConfig.NetworkMode == "none" and .HostConfig.Privileged == false and .HostConfig.PidMode == "" and .Config.User == "65534:65534" and .HostConfig.CapDrop == ["ALL"] and .HostConfig.Memory == 33554432 and .HostConfig.PidsLimit == 32 and (.HostConfig.SecurityOpt | index("no-new-privileges") != null)' "$EVIDENCE/writer-before.json" >/dev/null
cp "$EVIDENCE/output/.soak-state" "$EVIDENCE/state-before-crash.txt"
[[ ! -e "$EVIDENCE/output/writer-stop-failure.txt" && ! -e "$EVIDENCE/output/bench-segment-00001/metrics.json" ]]
date -u +%FT%TZ >"$EVIDENCE/crash-at.txt"
kill -KILL -- "-$DRIVER_PID"
status=0
wait "$DRIVER_PID" || status=$?
DRIVER_PID=""
printf '%s\n' "$status" >"$EVIDENCE/crashed-driver-exit.txt"
[[ "$status" == 137 ]]
stop_fixture_clients
printf 'The fixture stopped its recorded benchmark client group after the driver crash. Docker writer termination remains unconfirmed.\n' >"$EVIDENCE/fixture-client-stop.txt"
/usr/bin/docker inspect "$CID" >"$EVIDENCE/writer-after-crash.json"
jq -e '.[0].State | .Running == true and .Pid > 0' "$EVIDENCE/writer-after-crash.json" >/dev/null
[[ ! -e "$EVIDENCE/output/writer-stop-failure.txt" && ! -e "$EVIDENCE/output/bench-segment-00001/metrics.json" ]]
for phase in restart-1 restart-2; do
    launch_driver "$phase"
    for _ in $(seq 1 100); do
        kill -0 "$DRIVER_PID" 2>/dev/null || break
        [[ ! -s "$EVIDENCE/$phase-created-ids.txt" && ! -e "$EVIDENCE/iteration-invoked.txt" ]] || break
        sleep 0.1
    done
    if kill -0 "$DRIVER_PID" 2>/dev/null || [[ -s "$EVIDENCE/$phase-created-ids.txt" || -e "$EVIDENCE/iteration-invoked.txt" ]]; then
        printf 'FAIL: Restart admitted work after a benchmark crash without a committed outcome.\n' >&2
        exit 1
    fi
    status=0
    wait "$DRIVER_PID" || status=$?
    DRIVER_PID=""
    printf '%s\n' "$status" >"$EVIDENCE/$phase-driver-exit.txt"
    cp "$EVIDENCE/output/.soak-state" "$EVIDENCE/$phase-state.txt"
    cp "$EVIDENCE/output/summary.json" "$EVIDENCE/$phase-summary.json"
    if [[ "$status" == 0 ]] || ! jq -e '.iterations == 0 and .failures == 1 and .bench_segments == 1 and .bench_failures == 1' "$EVIDENCE/$phase-summary.json" >/dev/null; then
        printf 'FAIL: Restart lost or duplicated the interrupted benchmark failure.\n' >&2
        exit 1
    fi
done
/usr/bin/docker inspect "$CID" >"$EVIDENCE/writer-after-restarts.json"
jq -e '.[0].State | .Running == true and .Pid > 0' "$EVIDENCE/writer-after-restarts.json" >/dev/null
trap - ERR
printf 'PASS: Two restarts refused work and retained one interrupted benchmark failure while its Docker writer remained running.\n'

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
UNRELATED_ID=""
FIXTURE_GROUP="$(ps -o pgid= -p "$$" | tr -d ' ')"
cleanup() {
    [[ -z "$DRIVER_PID" ]] || kill -KILL -- "-$DRIVER_PID" 2>/dev/null || true
    local group cid
    if [[ -s "$EVIDENCE/client-group.txt" ]]; then
        group="$(<"$EVIDENCE/client-group.txt")"
        if [[ "$group" =~ ^[1-9][0-9]*$ && "$group" != "$FIXTURE_GROUP" ]]; then
            kill -KILL -- "-$group" 2>/dev/null || true
        fi
    fi
    if [[ -f "$EVIDENCE/created-ids.txt" ]]; then
        while IFS= read -r cid; do
            [[ "$cid" =~ ^[a-f0-9]{64}$ ]] || continue
            /usr/bin/docker rm -f "$cid" >/dev/null 2>&1 || true
        done <"$EVIDENCE/created-ids.txt"
    fi
    if [[ "$UNRELATED_ID" =~ ^[a-f0-9]{64}$ ]]; then
        /usr/bin/docker rm -f "$UNRELATED_ID" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT
trap 'printf "ERROR: The crash-stop fixture failed before its behavioral verdict.\n" >&2; exit 2' ERR
printf '%s\n' "$INSTANCE" >"$EVIDENCE/instance-id.txt"
(cd "$SOURCE" && sha256sum scripts/run-merge-recovery-soak.sh scripts/bench/run-bench-segment.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json) >"$EVIDENCE/source-sha256.txt"
UNRELATED_ID="$(/usr/bin/docker run -d --name "rnode.b38-unrelated.$$" --label fixture.owner=crash-stop-unrelated --network none --memory 32m --cpus 0.5 --pids-limit 32 --cap-drop ALL --security-opt no-new-privileges --user 65534:65534 "$IMAGE" /bin/sh -c 'i=0; while [ "$i" -lt 1200 ]; do printf "%s\n" "$i" >>/tmp/writes; i=$((i+1)); sleep 0.1; done')"
[[ "$UNRELATED_ID" =~ ^[a-f0-9]{64}$ ]]
printf '%s\n' "$UNRELATED_ID" >"$EVIDENCE/unrelated-id.txt"
jq -n --arg image "$IMAGE" '{services:{writer:{image:$image,labels:{"fixture.owner":"crash-stop-owned"},network_mode:"none",mem_limit:33554432,cpus:0.5,pids_limit:32,cap_drop:["ALL"],security_opt:["no-new-privileges"],user:"65534:65534",command:["/bin/sh","-c","i=0; while [ \"$$i\" -lt 1200 ]; do printf \"%s\\n\" \"$$i\" >>/tmp/writes; i=$$((i+1)); sleep 0.1; done"]}}}' >"$EVIDENCE/node/docker/shard.yml"
cat >"$EVIDENCE/bin/docker" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == compose && " $* " == *' up '* ]]; then
    ps -o pgid= -p "$PPID" | tr -d ' ' >"$SOAK_REAL_EVIDENCE/client-group.txt"
    ps -o sid= -p "$PPID" | tr -d ' ' >"$SOAK_REAL_EVIDENCE/client-session.txt"
    /usr/bin/docker "$@"
    /usr/bin/docker ps -q --no-trunc --filter label=com.docker.compose.project=soak-bench >>"$SOAK_REAL_EVIDENCE/created-ids.txt"
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
printf 'Unexpected iteration admission.\n' >>"$SOAK_REAL_EVIDENCE/iteration-invoked.txt"
exit 42
SH
chmod +x "$EVIDENCE/bin/"*
PATH="$EVIDENCE/bin:$PATH" SOAK_REAL_EVIDENCE="$EVIDENCE" \
    DEPLOYER_KEY=fixture-unused SOAK_NODE_REPO_DIR="$EVIDENCE/node" \
    SOAK_DURATION_SECONDS=1200 SOAK_RUN_BENCHMARKS=true SOAK_BENCH_DURATION=1 \
    SYSTEM_INTEGRATION_DIR="$EVIDENCE/harness" SOAK_OUTPUT_DIR="$EVIDENCE/output" \
    SOAK_TMP_ROOT="$EVIDENCE/tmp" SOAK_RUNNER_ROOT="$EVIDENCE/runner" \
    SOAK_RSS_CEILING_MB=0 SOAK_HOST_FREE_FLOOR_MB=0 \
    SOAK_DISK_FREE_FLOOR_MB=4096 SOAK_DISK_HYGIENE_BAND_MB=4096 \
    SOAK_DISK_STOP_SECONDS=2 SOAK_GUARDIAN_POLL_SECONDS=0.1 \
    setsid bash "$SOURCE/scripts/run-merge-recovery-soak.sh" >"$EVIDENCE/driver.log" 2>&1 &
DRIVER_PID=$!
for _ in $(seq 1 150); do
    [[ ! -s "$EVIDENCE/created-ids.txt" ]] || break
    kill -0 "$DRIVER_PID"
    sleep 0.1
done
[[ "$(wc -l <"$EVIDENCE/created-ids.txt")" == 1 ]]
[[ "$(<"$EVIDENCE/client-session.txt")" == "$DRIVER_PID" ]]
CID="$(<"$EVIDENCE/created-ids.txt")"
[[ "$CID" =~ ^[a-f0-9]{64}$ && "$CID" != "$UNRELATED_ID" ]]
for entry in "owned:$CID" "unrelated:$UNRELATED_ID"; do
    label="${entry%%:*}"
    /usr/bin/docker inspect "${entry#*:}" >"$EVIDENCE/$label-before.json"
    jq -e '.[0] | .State.Running == true and .State.Pid > 0 and (.Mounts | length) == 0 and .HostConfig.NetworkMode == "none" and .HostConfig.Privileged == false and .HostConfig.PidMode == "" and .Config.User == "65534:65534" and .HostConfig.CapDrop == ["ALL"] and .HostConfig.Memory == 33554432 and .HostConfig.PidsLimit == 32 and (.HostConfig.SecurityOpt | index("no-new-privileges") != null)' "$EVIDENCE/$label-before.json" >/dev/null
    /usr/bin/docker cp "${entry#*:}:/tmp/writes" "$EVIDENCE/$label-before.txt"
done
jq -e '.[0].Config.Labels["io.f1r3fly.soak.owner"] | test("^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$")' "$EVIDENCE/owned-before.json" >/dev/null
jq -e '.[0].Config.Labels["io.f1r3fly.soak.owner"] == null' "$EVIDENCE/unrelated-before.json" >/dev/null
cp "$EVIDENCE/output/.soak-state" "$EVIDENCE/state-before-crash.txt"
[[ ! -e "$EVIDENCE/output/bench-segment-00001/metrics.json" && ! -e "$EVIDENCE/iteration-invoked.txt" ]]
date -u +%FT%TZ >"$EVIDENCE/crash-at.txt"
read -r started _ </proc/uptime
kill -KILL -- "-$DRIVER_PID"
status=0
wait "$DRIVER_PID" || status=$?
DRIVER_PID=""
printf '%s\n' "$status" >"$EVIDENCE/crashed-driver-exit.txt"
[[ "$status" == 137 ]]
for _ in $(seq 1 100); do
    timeout --kill-after=1 2 /usr/bin/docker inspect "$CID" >"$EVIDENCE/owned-after.json"
    jq -e '.[0].State | .Running == false and .Pid == 0' "$EVIDENCE/owned-after.json" >/dev/null && break
    sleep 0.1
done
read -r finished _ </proc/uptime
printf 'start=%s\nfinish=%s\n' "$started" "$finished" >"$EVIDENCE/observation-clock.txt"
/usr/bin/docker cp "$CID:/tmp/writes" "$EVIDENCE/owned-after.txt"
/usr/bin/docker inspect "$UNRELATED_ID" >"$EVIDENCE/unrelated-after.json"
/usr/bin/docker cp "$UNRELATED_ID:/tmp/writes" "$EVIDENCE/unrelated-after.txt"
if ! jq -e '.[0].State | .Running == true and .Pid > 0' "$EVIDENCE/unrelated-after.json" >/dev/null || [[ "$(wc -c <"$EVIDENCE/unrelated-after.txt")" -le "$(wc -c <"$EVIDENCE/unrelated-before.txt")" ]]; then
    printf 'FAIL: Crash response stopped or stalled the unrelated Docker writer.\n' >&2
    exit 1
fi
if ! jq -e '.[0].State | .Running == false and .Pid == 0' "$EVIDENCE/owned-after.json" >/dev/null; then
    [[ "$(wc -c <"$EVIDENCE/owned-after.txt")" -gt "$(wc -c <"$EVIDENCE/owned-before.txt")" ]]
    printf 'FAIL: The owned Docker writer remained active after the driver process group crashed.\n' >&2
    exit 1
fi
sleep 0.5
/usr/bin/docker cp "$CID:/tmp/writes" "$EVIDENCE/owned-stable.txt"
cmp "$EVIDENCE/owned-after.txt" "$EVIDENCE/owned-stable.txt"
[[ ! -e "$EVIDENCE/output/bench-segment-00001/metrics.json" && ! -e "$EVIDENCE/iteration-invoked.txt" ]]
trap - ERR
printf 'PASS: Production crash response stopped the owned Docker writer and preserved the unrelated writer without fixture intervention.\n'

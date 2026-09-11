#!/usr/bin/env bash
set -euo pipefail

SCENARIO="${SOAK_HOST_STOP_SCENARIO:-exit}"
[[ "$SCENARIO" == exit || "$SCENARIO" == memory ]] || exit 2
if [[ "${1:-}" == --inside ]]; then
    [[ -f /.dockerenv && "$(id -u)" == 65534 && ! -S /var/run/docker.sock ]] || exit 2
    cd /case
    mkdir -p evidence bin harness tmp runner integration-tests/test/tests/custom
    trap 'printf "ERROR: The host ownership fixture failed before its behavioral verdict.\n" >&2; exit 2' ERR
    cat >evidence/writer.sh <<'SH'
#!/usr/bin/env bash
set -euo pipefail
trap '' TERM
printf '%s\n' "$$" >"$1.pid"
for n in $(seq 1 600); do printf '%s\n' "$n" >>"$1.writes"; sleep 0.1; done
SH
    cp evidence/writer.sh /tmp/rnode-unrelated
    cp evidence/writer.sh /tmp/rnode-owned
    cp evidence/writer.sh integration-tests/test/tests/custom/test_load.py
    setsid bash /tmp/rnode-unrelated /case/evidence/unrelated-node &
    UNRELATED_NODE=$!
    setsid bash integration-tests/test/tests/custom/test_load.py /case/evidence/unrelated-client &
    UNRELATED_CLIENT=$!
    cat >bin/poetry <<'SH'
#!/usr/bin/env bash
set -euo pipefail
trap 'exit 143' TERM INT
setsid bash /tmp/rnode-owned /case/evidence/owned-node &
while :; do sleep 0.1; done
SH
    cat >bin/docker <<'SH'
#!/usr/bin/env bash
exit 0
SH
    cp bin/docker bin/oci
    cat >bin/awk <<'SH'
#!/usr/bin/env bash
if [[ "${*: -1}" == /proc/meminfo && "$1" == *MemAvailable* ]]; then
    available=16384
    [[ ! -e /case/evidence/activate-memory-fault ]] || available=1024
    printf '%s\n' "$available" >>/case/evidence/memory-samples.txt
    printf '%s\n' "$available"
    exit 0
fi
exec /usr/bin/awk "$@"
SH
    HOST_FLOOR=0
    [[ "$SCENARIO" != memory ]] || HOST_FLOOR=4096
    chmod +x bin/*
    (cd repo && sha256sum scripts/run-merge-recovery-soak.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json) >evidence/source-sha256.txt
    PATH="/case/bin:$PATH" SYSTEM_INTEGRATION_DIR=/case/harness SOAK_OUTPUT_DIR=/case/evidence/output \
        SOAK_TMP_ROOT=/case/tmp SOAK_RUNNER_ROOT=/case/runner SOAK_DURATION_SECONDS=120 \
        SOAK_RUN_BENCHMARKS=false SOAK_RSS_CEILING_MB=0 SOAK_HOST_FREE_FLOOR_MB="$HOST_FLOOR" \
        SOAK_DISK_FREE_FLOOR_MB=0 SOAK_DISK_STOP_SECONDS=2 SOAK_GUARDIAN_POLL_SECONDS=0.1 \
        setsid bash repo/scripts/run-merge-recovery-soak.sh >evidence/driver.log 2>&1 &
    DRIVER=$!
    for _ in $(seq 1 100); do
        [[ ! -s evidence/owned-node.pid ]] || break
        kill -0 "$DRIVER"
        sleep 0.1
    done
    [[ -s evidence/owned-node.pid ]]
    OWNED_NODE="$(<evidence/owned-node.pid)"
    [[ "$OWNED_NODE" =~ ^[1-9][0-9]*$ ]]
    sleep 0.3
    for role in owned-node unrelated-node unrelated-client; do
        pid="$(<"evidence/$role.pid")"
        [[ "$pid" =~ ^[1-9][0-9]*$ ]]
        state="$(ps -o stat= -p "$pid")"
        [[ -n "$state" && "$state" != Z* ]]
        printf '%s\n' "$state" >"evidence/$role-before.txt"
    done
    date -u +%FT%TZ >evidence/signal-at.txt
    if [[ "$SCENARIO" == memory ]]; then
        for _ in $(seq 1 100); do
            grep -Fxq 16384 evidence/memory-samples.txt 2>/dev/null && break
            kill -0 "$DRIVER"
            sleep 0.1
        done
        grep -Fxq 16384 evidence/memory-samples.txt
        touch evidence/activate-memory-fault
    else
        kill -TERM "$DRIVER"
    fi
    for _ in $(seq 1 120); do kill -0 "$DRIVER" 2>/dev/null || break; sleep 0.1; done
    if kill -0 "$DRIVER" 2>/dev/null; then
        printf 'ERROR: The driver exceeded the host ownership observation deadline.\n' >&2
        exit 2
    fi
    status=0
    wait "$DRIVER" || status=$?
    printf '%s\n' "$status" >evidence/driver-exit.txt
    for role in owned-node unrelated-node unrelated-client; do
        pid="$(<"evidence/$role.pid")"
        ps -o stat= -p "$pid" >"evidence/$role-after.txt" || true
        cp "evidence/$role.writes" "evidence/$role-after.writes"
    done
    sleep 0.5
    for role in owned-node unrelated-node unrelated-client; do cp "evidence/$role.writes" "evidence/$role-confirmed.writes"; done
    date -u +%FT%TZ >evidence/observed-at.txt
    if [[ "$SCENARIO" == memory ]]; then
        grep -Fxq 16384 evidence/memory-samples.txt
        grep -Fxq 1024 evidence/memory-samples.txt
        [[ -s evidence/output/host-guardian-breach.txt ]]
    fi
    trap - ERR
    for role in unrelated-node unrelated-client; do
        state="$(<"evidence/$role-after.txt")"
        if [[ -z "$state" || "$state" == Z* ]] || cmp -s "evidence/$role-after.writes" "evidence/$role-confirmed.writes"; then
            printf 'FAIL: Driver exit stopped an unrelated host writer selected by its command line.\n' >&2
            exit 1
        fi
    done
    state="$(<evidence/owned-node-after.txt)"
    if [[ -n "$state" && "$state" != Z* ]] || ! cmp -s evidence/owned-node-after.writes evidence/owned-node-confirmed.writes; then
        printf 'FAIL: Driver exit did not stop its detached host writer.\n' >&2
        exit 1
    fi
    kill -KILL "$UNRELATED_NODE" "$UNRELATED_CLIENT" 2>/dev/null || true
    printf 'PASS: Driver exit stopped its detached host writer and preserved both unrelated host writers.\n'
    exit 0
fi

SOURCE="$(cd "${1:?source directory is required}" && pwd)"
EVIDENCE="${2:?new evidence directory is required}"
IMAGE="${SOAK_DISK_TEST_IMAGE:?an immutable fixture image is required}"
[[ "$IMAGE" =~ ^sha256:[a-f0-9]{64}$ && ! -e "$EVIDENCE" ]] || exit 2
mkdir -p "$EVIDENCE"
EVIDENCE="$(cd "$EVIDENCE" && pwd)"
CID=""
trap '[[ ! "$CID" =~ ^[a-f0-9]{64}$ ]] || docker rm -f "$CID" >/dev/null 2>&1 || true' EXIT
CID="$(docker create --pull=never --network none --cap-drop ALL --security-opt no-new-privileges --pids-limit 128 --memory 256m --cpus 1 --user 65534:65534 --workdir /case --env "SOAK_HOST_STOP_SCENARIO=$SCENARIO" --entrypoint bash "$IMAGE" /case/test.sh --inside)"
[[ "$CID" =~ ^[a-f0-9]{64}$ ]]
docker inspect "$CID" >"$EVIDENCE/container-before.json"
jq -e '.[0] | (.Mounts | length) == 0 and .HostConfig.NetworkMode == "none" and .HostConfig.Privileged == false and .HostConfig.PidMode == "" and .Config.User == "65534:65534" and .HostConfig.CapDrop == ["ALL"] and (.HostConfig.SecurityOpt | index("no-new-privileges") != null)' "$EVIDENCE/container-before.json" >/dev/null
tar -C "$SOURCE" --mode='a+rX' -cf - scripts/run-merge-recovery-soak.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json | docker cp - "$CID:/case/repo/"
docker cp "${BASH_SOURCE[0]}" "$CID:/case/test.sh"
status=0
timeout --signal=TERM --kill-after=5 40 docker start -a "$CID" >"$EVIDENCE/result.txt" 2>&1 || status=$?
printf '%s\n' "$status" >"$EVIDENCE/fixture-exit.txt"
docker inspect "$CID" >"$EVIDENCE/container-after.json"
jq -e '.[0].State | .Running == false and .OOMKilled == false' "$EVIDENCE/container-after.json" >/dev/null || exit 2
[[ "$(jq -r '.[0].State.ExitCode' "$EVIDENCE/container-after.json")" == "$status" ]] || exit 2
docker cp "$CID:/case/evidence" "$EVIDENCE/evidence"
exit "$status"

#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == --inside ]]; then
    [[ -f /.dockerenv && "$(id -u)" == 65534 && ! -S /var/run/docker.sock ]] || exit 2
    cd /case
    mkdir -p evidence bin harness tmp runner
    trap 'printf "ERROR: The monitor-death fixture failed before its behavioral verdict.\n" >&2; exit 2' ERR
    cat >evidence/writer.sh <<'SH'
#!/usr/bin/env bash
set -euo pipefail
trap '' TERM
printf '%s\n' "$$" >"$1.pid"
for n in $(seq 1 600); do printf '%s\n' "$n" >>"$1.writes"; sleep 0.1; done
SH
    setsid bash evidence/writer.sh /case/evidence/unrelated >/dev/null 2>&1 &
    cat >bin/poetry <<'SH'
#!/usr/bin/env bash
set -euo pipefail
trap 'exit 143' TERM INT
printf 'Iteration admitted.\n' >>/case/evidence/admissions.txt
setsid bash /case/evidence/writer.sh /case/evidence/owned >/dev/null 2>&1 &
while :; do sleep 0.1; done
SH
    cat >bin/docker <<'SH'
#!/usr/bin/env bash
exit 0
SH
    cp bin/docker bin/oci
    chmod +x bin/*
    (cd repo && sha256sum scripts/run-merge-recovery-soak.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json) >evidence/source-sha256.txt
    driver() {
        exec env PATH="/case/bin:$PATH" SYSTEM_INTEGRATION_DIR=/case/harness SOAK_OUTPUT_DIR=/case/evidence/output \
            SOAK_TMP_ROOT=/case/tmp SOAK_RUNNER_ROOT=/case/runner SOAK_DURATION_SECONDS=120 \
            SOAK_RUN_BENCHMARKS=false SOAK_RSS_CEILING_MB=0 SOAK_HOST_FREE_FLOOR_MB=0 \
            SOAK_DISK_FREE_FLOOR_MB=0 SOAK_DISK_STOP_SECONDS=2 SOAK_GUARDIAN_POLL_SECONDS=0.1 \
            setsid bash repo/scripts/run-merge-recovery-soak.sh
    }
    (driver) >evidence/driver.log 2>&1 &
    DRIVER=$!
    for _ in $(seq 1 100); do
        [[ ! -s evidence/owned.pid ]] || break
        kill -0 "$DRIVER"
        sleep 0.1
    done
    [[ -s evidence/owned.pid && "$(wc -l <evidence/admissions.txt)" == 1 ]]
    sleep 0.3
    for role in owned unrelated; do
        pid="$(<"evidence/$role.pid")"
        [[ "$pid" =~ ^[1-9][0-9]*$ ]]
        state="$(ps -o stat= -p "$pid")"
        [[ -n "$state" && "$state" != Z* ]]
        printf '%s\n' "$state" >"evidence/$role-before.txt"
        cp "evidence/$role.writes" "evidence/$role-before.writes"
        tr '\0' '\n' <"/proc/$pid/environ" | grep '^SOAK_PROCESS_OWNER=' >"evidence/$role-owner.txt" || true
    done
    grep -Eq '^SOAK_PROCESS_OWNER=[a-f0-9]{8}(-[a-f0-9]{4}){3}-[a-f0-9]{12}$' evidence/owned-owner.txt
    [[ ! -s evidence/unrelated-owner.txt ]]
    shopt -s nullglob
    readiness=(/case/evidence/output/.crash-monitor.*/ready)
    [[ "${#readiness[@]}" == 1 ]]
    MONITOR="$(<"${readiness[0]}")"
    [[ "$MONITOR" =~ ^[1-9][0-9]*$ ]]
    ps -eo pid,ppid,sid,stat,args >evidence/processes-before.txt
    cp evidence/output/.soak-state evidence/state-before.txt
    [[ ! -s evidence/output/host-guardian-breach.txt && ! -e evidence/output/summary.json ]]
    date -u +%FT%TZ >evidence/fault-at.txt
    read -r started _ </proc/uptime
    python3 - "$MONITOR" "$DRIVER" "${readiness[0]%/ready}" >evidence/fault.txt <<'PY'
import os
import select
import signal
import sys
import time

pid, driver = map(int, sys.argv[1:3])
fd = os.pidfd_open(pid, 0)
expected = [b"python3", b"-", sys.argv[2].encode(), sys.argv[3].encode(), b"/case/evidence/output", b""]
with open(f"/proc/{pid}/cmdline", "rb") as source:
    assert source.read().split(b"\0") == expected
assert os.stat(f"/proc/{pid}").st_uid == os.geteuid()
assert os.getsid(pid) == pid
with open(f"/proc/{pid}/stat") as source:
    assert int(source.read().rsplit(") ", 1)[1].split()[1]) == driver
poller = select.poll()
poller.register(fd, select.POLLIN)
assert not poller.poll(0)
driver_fd = os.pidfd_open(driver, 0)
assert os.stat(f"/proc/{driver}").st_uid == os.geteuid()
assert os.getsid(driver) == driver
with open(f"/proc/{driver}/cmdline", "rb") as source:
    assert source.read().split(b"\0") == [b"bash", b"repo/scripts/run-merge-recovery-soak.sh", b""]
for target, descriptor in [(driver, driver_fd), (pid, fd)]:
    signal.pidfd_send_signal(descriptor, signal.SIGSTOP)
    for _ in range(100):
        with open(f"/proc/{target}/stat") as source:
            state = source.read().rsplit(") ", 1)[1].split()[0]
        if state == "T":
            break
        time.sleep(0.01)
    assert state == "T"
for descriptor in [driver_fd, fd]:
    signal.pidfd_send_signal(descriptor, signal.SIGKILL)
    dead = select.poll()
    dead.register(descriptor, select.POLLIN)
    assert dead.poll(2000)
    os.close(descriptor)
print("The fixture confirmed driver and monitor death through their pidfds.")
PY
    for _ in $(seq 1 120); do
        owned_state="$(ps -o stat= -p "$(<evidence/owned.pid)" || true)"
        if ! kill -0 "$DRIVER" 2>/dev/null && [[ -z "$owned_state" || "$owned_state" == Z* ]]; then
            break
        fi
        sleep 0.1
    done
    read -r finished _ </proc/uptime
    printf 'start=%s\nfinish=%s\n' "$started" "$finished" >evidence/observation-clock.txt
    ps -eo pid,ppid,sid,stat,args >evidence/processes-after.txt
    running=0
    kill -0 "$DRIVER" 2>/dev/null && running=1
    printf '%s\n' "$running" >evidence/driver-running.txt
    for role in owned unrelated; do
        pid="$(<"evidence/$role.pid")"
        ps -o stat= -p "$pid" >"evidence/$role-after.txt" || true
        cp "evidence/$role.writes" "evidence/$role-after.writes"
    done
    sleep 0.5
    for role in owned unrelated; do cp "evidence/$role.writes" "evidence/$role-confirmed.writes"; done
    state="$(<evidence/unrelated-after.txt)"
    if [[ -z "$state" || "$state" == Z* ]] || cmp -s evidence/unrelated-after.writes evidence/unrelated-confirmed.writes; then
        printf 'FAIL: Monitor failure stopped or stalled the unrelated host writer.\n' >&2
        exit 1
    fi
    state="$(<evidence/owned-after.txt)"
    if [[ "$running" == 1 && -n "$state" && "$state" != Z* ]] && ! cmp -s evidence/owned-after.writes evidence/owned-confirmed.writes; then
        printf 'FAIL: The driver continued its owned host writer after the crash monitor died.\n' >&2
        exit 1
    fi
    [[ "$running" == 0 ]]
    status=0
    wait "$DRIVER" || status=$?
    printf '%s\n' "$status" >evidence/driver-exit.txt
    if [[ "$status" == 0 || (-n "$state" && "$state" != Z*) ]] || ! cmp -s evidence/owned-after.writes evidence/owned-confirmed.writes; then
        printf 'FAIL: Both controllers exited but the owned writer remained active.\n' >&2
        exit 1
    fi
    for pass in restart-1 restart-2; do
        status=0
        (driver) >"evidence/$pass.log" 2>&1 || status=$?
        printf '%s\n' "$status" >"evidence/$pass-exit.txt"
        cp evidence/output/summary.json "evidence/$pass-summary.json"
        if [[ "$status" == 0 || "$(wc -l <evidence/admissions.txt)" != 1 ]] || ! jq -e '[.iterations,.failures,.bench_segments,.bench_failures] == [1,1,0,0]' "evidence/$pass-summary.json" >/dev/null; then
            printf 'FAIL: Monitor failure lost its retained failure or admitted more work.\n' >&2
            exit 1
        fi
    done
    trap - ERR
    printf 'PASS: Both controllers exited, the owned writer stopped, and two restarts retained one failure without new admissions.\n'
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
CID="$(docker create --pull=never --network none --cap-drop ALL --security-opt no-new-privileges --pids-limit 128 --memory 256m --cpus 1 --user 65534:65534 --workdir /case --entrypoint bash "$IMAGE" /case/test.sh --inside)"
[[ "$CID" =~ ^[a-f0-9]{64}$ ]]
docker inspect "$CID" >"$EVIDENCE/container-before.json"
jq -e '.[0] | (.Mounts | length) == 0 and .HostConfig.NetworkMode == "none" and .HostConfig.Privileged == false and .HostConfig.PidMode == "" and .Config.User == "65534:65534" and .HostConfig.CapDrop == ["ALL"] and .HostConfig.Memory == 268435456 and .HostConfig.PidsLimit == 128 and (.HostConfig.SecurityOpt | index("no-new-privileges") != null)' "$EVIDENCE/container-before.json" >/dev/null
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

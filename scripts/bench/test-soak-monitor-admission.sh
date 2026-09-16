#!/usr/bin/env bash
set -euo pipefail

MODE="${SOAK_MONITOR_ADMISSION_MODE:-benchmark}"
[[ "$MODE" == benchmark || "$MODE" == iteration ]] || exit 2
if [[ "${1:-}" == --inside ]]; then
    [[ -f /.dockerenv && "$(id -u)" == 65534 && ! -S /var/run/docker.sock ]] || exit 2
    cd /case
    mkdir -p evidence bin harness tmp runner node/docker
    trap 'printf "ERROR: The monitor admission fixture failed before its behavioral verdict.\n" >&2; exit 2' ERR
    printf '{"services":{"writer":{"image":"fixture-unused"}}}\n' >node/docker/shard.yml
    cat >bin/df <<'SH'
#!/usr/bin/env bash
set -euo pipefail
python3 - <<'PY'
import os
from pathlib import Path
import select
import signal

root = Path('/case/evidence')
ready = list((root / 'output').glob('.crash-monitor.*/ready'))
log = (root / 'driver.log').read_text()
guardians = [int(line.split()[-1]) for line in log.splitlines() if line.startswith('orchestrator host guardian watching')]
if len(ready) == 1 and len(guardians) == 1 and not (root / 'fault.txt').exists():
    ancestor = os.getppid()
    from_guardian = False
    for _ in range(32):
        if ancestor == guardians[0]:
            from_guardian = True
            break
        if ancestor <= 1:
            break
        ancestor = int(Path(f'/proc/{ancestor}/stat').read_text().rsplit(') ', 1)[1].split()[1])
    if not from_guardian:
        pid = int(ready[0].read_text())
        fd = os.pidfd_open(pid, 0)
        stat = Path(f'/proc/{pid}/stat').read_text().rsplit(') ', 1)[1].split()
        driver = int(stat[1])
        expected = [b'python3', b'-', str(driver).encode(), str(ready[0].parent).encode(), b'/case/evidence/output', b'']
        assert Path(f'/proc/{pid}/cmdline').read_bytes().split(b'\0') == expected
        assert os.stat(f'/proc/{pid}').st_uid == os.geteuid()
        assert os.getsid(pid) == pid
        poller = select.poll()
        poller.register(fd, select.POLLIN)
        assert not poller.poll(0)
        signal.pidfd_send_signal(fd, signal.SIGKILL)
        assert poller.poll(1000)
        os.close(fd)
        (root / 'fault.txt').write_text('Monitor death was confirmed before the admission probe returned 16384 MiB.\n')
print('Filesystem 1024-blocks Used Available Capacity Mounted on')
print('fixture 32768 16384 16384 50% /case')
PY
SH
    cat >bin/docker <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == compose && " $* " == *' config '* ]]; then
    printf '{"services":{"writer":{"image":"fixture-unused"}}}\n'
elif [[ "${1:-}" == compose && " $* " == *' up '* ]]; then
    printf 'A benchmark was admitted.\n' >>/case/evidence/benchmark-admitted.txt
fi
exit 0
SH
    cat >bin/poetry <<'SH'
#!/usr/bin/env bash
trap 'exit 143' TERM INT
printf 'An iteration was admitted.\n' >>/case/evidence/iteration-admitted.txt
while :; do sleep 0.1; done
SH
    printf '#!/usr/bin/env bash\nexit 1\n' >bin/curl
    cp bin/curl bin/oci
    chmod +x bin/*
    cat >evidence/unrelated.sh <<'SH'
#!/usr/bin/env bash
trap '' TERM
for n in $(seq 1 600); do printf '%s\n' "$n" >>/case/evidence/unrelated.writes; sleep 0.1; done
SH
    setsid bash evidence/unrelated.sh >/dev/null 2>&1 &
    UNRELATED=$!
    (cd repo && sha256sum scripts/run-merge-recovery-soak.sh scripts/bench/run-bench-segment.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json) >evidence/source-sha256.txt
    benchmarks=false
    [[ "$MODE" != benchmark ]] || benchmarks=true
    driver() {
        exec env PATH="/case/bin:$PATH" SYSTEM_INTEGRATION_DIR=/case/harness SOAK_OUTPUT_DIR=/case/evidence/output \
            SOAK_TMP_ROOT=/case/tmp SOAK_RUNNER_ROOT=/case/runner SOAK_DURATION_SECONDS=1200 \
            SOAK_RUN_BENCHMARKS="$benchmarks" SOAK_BENCH_DURATION=1 SOAK_NODE_REPO_DIR=/case/node DEPLOYER_KEY=fixture-unused \
            SOAK_RSS_CEILING_MB=0 SOAK_HOST_FREE_FLOOR_MB=0 SOAK_DISK_FREE_FLOOR_MB=4096 \
            SOAK_DISK_HYGIENE_BAND_MB=4096 SOAK_DISK_STOP_SECONDS=2 SOAK_GUARDIAN_POLL_SECONDS=0.1 \
            setsid bash repo/scripts/run-merge-recovery-soak.sh
    }
    (driver) >evidence/driver.log 2>&1 &
    DRIVER=$!
    for _ in $(seq 1 150); do
        kill -0 "$DRIVER" 2>/dev/null || break
        sleep 0.1
    done
    [[ -s evidence/fault.txt ]]
    if kill -0 "$DRIVER" 2>/dev/null; then
        printf 'FAIL: The driver did not finish after monitor death at admission.\n' >&2
        exit 1
    fi
    status=0
    wait "$DRIVER" || status=$?
    printf '%s\n' "$status" >evidence/driver-exit.txt
    ps -o stat= -p "$UNRELATED" >evidence/unrelated-after.txt
    cp evidence/unrelated.writes evidence/unrelated-before.writes
    sleep 0.5
    cp evidence/unrelated.writes evidence/unrelated-after.writes
    [[ "$(<evidence/unrelated-after.txt)" != Z* ]] && ! cmp -s evidence/unrelated-before.writes evidence/unrelated-after.writes
    for pass in initial restart-1 restart-2; do
        if [[ "$pass" != initial ]]; then
            status=0
            (driver) >"evidence/$pass.log" 2>&1 || status=$?
            printf '%s\n' "$status" >"evidence/$pass-exit.txt"
        fi
        cp evidence/output/summary.json "evidence/$pass-summary.json"
        if [[ -e evidence/benchmark-admitted.txt || -e evidence/iteration-admitted.txt ]] || ! jq -e '.iterations == 0 and .bench_segments == 0' "evidence/$pass-summary.json" >/dev/null; then
            printf 'FAIL: The driver admitted %s work after confirmed monitor death during its admission probe.\n' "$MODE" >&2
            exit 1
        fi
        if [[ "$status" == 0 || ! -s evidence/output/host-guardian-breach.txt ]] || ! jq -e '.failures == 1 and .bench_failures == 0' "evidence/$pass-summary.json" >/dev/null; then
            printf 'FAIL: Monitor admission refusal lost or duplicated its retained failure.\n' >&2
            exit 1
        fi
    done
    trap - ERR
    printf 'PASS: Monitor death prevents %s admission and retains one failure across two refused restarts.\n' "$MODE"
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
CID="$(docker create --pull=never --network none --cap-drop ALL --security-opt no-new-privileges --pids-limit 128 --memory 256m --cpus 1 --user 65534:65534 --workdir /case --env "SOAK_MONITOR_ADMISSION_MODE=$MODE" --entrypoint bash "$IMAGE" /case/test.sh --inside)"
[[ "$CID" =~ ^[a-f0-9]{64}$ ]]
docker inspect "$CID" >"$EVIDENCE/container-before.json"
jq -e '.[0] | (.Mounts | length) == 0 and .HostConfig.NetworkMode == "none" and .HostConfig.Privileged == false and .HostConfig.PidMode == "" and .Config.User == "65534:65534" and .HostConfig.CapDrop == ["ALL"] and .HostConfig.Memory == 268435456 and .HostConfig.PidsLimit == 128 and (.HostConfig.SecurityOpt | index("no-new-privileges") != null)' "$EVIDENCE/container-before.json" >/dev/null
tar -C "$SOURCE" --mode='a+rX' -cf - scripts/run-merge-recovery-soak.sh scripts/bench/run-bench-segment.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json | docker cp - "$CID:/case/repo/"
docker cp "${BASH_SOURCE[0]}" "$CID:/case/test.sh"
status=0
timeout --signal=TERM --kill-after=5 40 docker start -a "$CID" >"$EVIDENCE/result.txt" 2>&1 || status=$?
printf '%s\n' "$status" >"$EVIDENCE/fixture-exit.txt"
docker inspect "$CID" >"$EVIDENCE/container-after.json"
jq -e '.[0].State | .Running == false and .OOMKilled == false' "$EVIDENCE/container-after.json" >/dev/null || exit 2
[[ "$(jq -r '.[0].State.ExitCode' "$EVIDENCE/container-after.json")" == "$status" ]] || exit 2
docker cp "$CID:/case/evidence" "$EVIDENCE/evidence"
exit "$status"

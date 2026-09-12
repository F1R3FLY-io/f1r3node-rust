#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == --inside ]]; then
    [[ -f /.dockerenv && "$(id -u)" == 65534 && ! -S /var/run/docker.sock ]] || exit 2
    cd /case
    mkdir -p evidence bin harness/integration-tests tmp runner
    trap 'printf "ERROR: The emergency deadline fixture failed before its behavioral verdict.\n" >&2; exit 2' ERR
    DRIVER=""
    stop_driver() {
        [[ -z "$DRIVER" ]] || kill -TERM -- "-$DRIVER" 2>/dev/null || true
        [[ -z "$DRIVER" ]] || wait "$DRIVER" 2>/dev/null || true
        DRIVER=""
    }
    for root in data log-archive .subprocess-data; do
        for session in $(seq 1 8); do
            mkdir -p "harness/integration-tests/$root/session-$session"
            printf 'session %s telemetry\n' "$session" >"harness/integration-tests/$root/session-$session/resource-timeseries.csv"
            printf 'session %s log\n' "$session" >"harness/integration-tests/$root/session-$session/node.log"
        done
    done
    cat >bin/df <<'SH'
#!/usr/bin/env bash
available=16384
if [[ -f /case/evidence/workload-started.txt ]]; then
    available=1024
fi
printf 'valid=%s\n' "$available" >>/case/evidence/probe-samples.txt
printf 'Filesystem 1M-blocks Used Available Capacity Mounted on\n'
printf '/dev/fixture 47000 %s %s 85%% /\n' "$((47000 - available))" "$available"
SH
    cat >bin/tar <<'SH'
#!/usr/bin/env bash
trap '' TERM
if [[ -s /case/evidence/output/protection-breach.txt && -s /case/evidence/output/early-exit.txt ]]; then
    printf 'present\n' >>/case/evidence/record-at-copy.txt
else
    printf 'absent\n' >>/case/evidence/record-at-copy.txt
fi
date -u +%FT%TZ >>/case/evidence/copy-started.txt
sleep 600
SH
    cat >bin/docker <<'SH'
#!/usr/bin/env bash
exit 0
SH
    cat >bin/poetry <<'SH'
#!/usr/bin/env bash
trap 'exit 143' TERM INT
printf '%s\n' "$*" >/case/evidence/workload-started.txt
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
    (cd repo && sha256sum scripts/run-merge-recovery-soak.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json) >evidence/source-sha256.txt
    DEADLINE_SECONDS=10
    SLACK_SECONDS=15
    driver() {
        exec env PATH="/case/bin:$PATH" SYSTEM_INTEGRATION_DIR=/case/harness SOAK_OUTPUT_DIR=/case/evidence/output \
            SOAK_TMP_ROOT=/case/tmp SOAK_RUNNER_ROOT=/case/runner SOAK_DURATION_SECONDS=1200 \
            SOAK_RUN_BENCHMARKS=false SOAK_RSS_CEILING_MB=0 SOAK_HOST_FREE_FLOOR_MB=0 SOAK_DISK_FREE_FLOOR_MB=4096 \
            SOAK_DISK_HYGIENE_BAND_MB=4096 SOAK_DISK_STOP_SECONDS=2 SOAK_DISK_DIAGNOSTIC_SECONDS=2 \
            SOAK_EMERGENCY_DEADLINE_SECONDS="$DEADLINE_SECONDS" SOAK_GUARDIAN_POLL_SECONDS=0.1 \
            setsid bash repo/scripts/run-merge-recovery-soak.sh
    }
    (driver) >evidence/driver.log 2>&1 &
    DRIVER=$!
    for _ in $(seq 1 200); do
        [[ ! -s evidence/output/host-guardian-breach.txt ]] || break
        kill -0 "$DRIVER" 2>/dev/null || break
        sleep 0.1
    done
    if [[ ! -s evidence/output/host-guardian-breach.txt || ! -s evidence/workload-started.txt ]]; then
        stop_driver
        printf 'ERROR: The guardian did not fire during the iteration within the fixture budget.\n' >&2
        exit 2
    fi
    read -r breach_at _ </proc/uptime
    breach_at="${breach_at%%.*}"
    printf '%s\n' "$breach_at" >evidence/breach-clock.txt
    budget=$((DEADLINE_SECONDS + SLACK_SECONDS))
    verdict=missed
    for _ in $(seq 1 $((budget * 10))); do
        if ! kill -0 "$DRIVER" 2>/dev/null && [[ -s evidence/output/summary.json ]]; then
            verdict=met
            break
        fi
        sleep 0.1
    done
    read -r finished_at _ </proc/uptime
    finished_at="${finished_at%%.*}"
    printf 'breach=%s\nfinished=%s\nbudget=%s\n%s\n' "$breach_at" "$finished_at" "$budget" "$verdict" >evidence/emergency-deadline.txt
    running=0
    kill -0 "$DRIVER" 2>/dev/null && running=1
    status=0
    if [[ "$running" == 1 ]]; then
        stop_driver
        status=143
    else
        wait "$DRIVER" || status=$?
        DRIVER=""
    fi
    printf '%s\n' "$status" >evidence/driver-exit.txt
    ps -o stat= -p "$UNRELATED" >evidence/unrelated-after.txt
    cp evidence/unrelated.writes evidence/unrelated-before.writes
    sleep 0.5
    cp evidence/unrelated.writes evidence/unrelated-after.writes
    [[ "$(<evidence/unrelated-after.txt)" != Z* ]] && ! cmp -s evidence/unrelated-before.writes evidence/unrelated-after.writes
    if [[ ! -s evidence/copy-started.txt ]]; then
        printf 'ERROR: The fixture did not exercise the stalled evidence copy.\n' >&2
        exit 2
    fi
    if [[ "$(head -1 evidence/record-at-copy.txt)" != present ]]; then
        printf 'FAIL: The evidence copy started before the driver published its breach record.\n' >&2
        exit 1
    fi
    if [[ "$verdict" != met ]]; then
        printf 'FAIL: The emergency response exceeded the composed deadline. Failure publication was delayed by a stalled evidence copy.\n' >&2
        exit 1
    fi
    if [[ "$status" != 1 ]] || ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
        ! jq -e '[.iterations,.failures,.bench_segments,.bench_failures] == [1,1,0,0]' evidence/output/summary.json >/dev/null; then
        printf 'FAIL: The bounded emergency response lost the protection failure.\n' >&2
        exit 1
    fi
    trap - ERR
    printf 'PASS: The breach record was published before evidence copying, and the emergency response published the failure within the composed deadline despite a stalled evidence copy across 24 session roots.\n'
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
CID="$(docker create --pull=never --network none --cap-drop ALL --security-opt no-new-privileges --pids-limit 256 --memory 256m --cpus 1 --user 65534:65534 --workdir /case --entrypoint bash "$IMAGE" /case/test.sh --inside)"
[[ "$CID" =~ ^[a-f0-9]{64}$ ]]
docker inspect "$CID" >"$EVIDENCE/container-before.json"
jq -e '.[0] | (.Mounts | length) == 0 and .HostConfig.NetworkMode == "none" and .HostConfig.Privileged == false and .HostConfig.PidMode == "" and .Config.User == "65534:65534" and .HostConfig.CapDrop == ["ALL"] and .HostConfig.Memory == 268435456 and .HostConfig.PidsLimit == 256 and (.HostConfig.SecurityOpt | index("no-new-privileges") != null)' "$EVIDENCE/container-before.json" >/dev/null
tar -C "$SOURCE" --mode='a+rX' -cf - scripts/run-merge-recovery-soak.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json | docker cp - "$CID:/case/repo/"
docker cp "${BASH_SOURCE[0]}" "$CID:/case/test.sh"
status=0
timeout --signal=TERM --kill-after=5 120 docker start -a "$CID" >"$EVIDENCE/result.txt" 2>&1 || status=$?
printf '%s\n' "$status" >"$EVIDENCE/fixture-exit.txt"
docker inspect "$CID" >"$EVIDENCE/container-after.json"
jq -e '.[0].State | .Running == false and .OOMKilled == false' "$EVIDENCE/container-after.json" >/dev/null || exit 2
[[ "$(jq -r '.[0].State.ExitCode' "$EVIDENCE/container-after.json")" == "$status" ]] || exit 2
docker cp "$CID:/case/evidence" "$EVIDENCE/evidence"
exit "$status"

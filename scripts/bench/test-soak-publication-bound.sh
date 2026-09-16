#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == --inside ]]; then
    [[ -f /.dockerenv && "$(id -u)" == 65534 && ! -S /var/run/docker.sock ]] || exit 2
    cd /case
    mkdir -p evidence bin harness/integration-tests/data/session-1 tmp runner
    trap 'printf "ERROR: The publication-bound fixture failed before its behavioral verdict.\n" >&2; exit 2' ERR
    DRIVER=""
    stop_driver() {
        [[ -z "$DRIVER" ]] || kill -TERM -- "-$DRIVER" 2>/dev/null || true
        [[ -z "$DRIVER" ]] || wait "$DRIVER" 2>/dev/null || true
        DRIVER=""
    }
    printf 'session 1 telemetry\n' >harness/integration-tests/data/session-1/resource-timeseries.csv
    cat >bin/df <<'SH'
#!/usr/bin/env bash
available=16384
if [[ -f /case/evidence/workload-started.txt ]]; then
    available=1024
fi
printf 'Filesystem 1M-blocks Used Available Capacity Mounted on\n'
printf '/dev/fixture 47000 %s %s 85%% /\n' "$((47000 - available))" "$available"
SH
    cat >bin/sync <<'SH'
#!/usr/bin/env bash
if [[ -f /case/evidence/workload-started.txt ]]; then
    printf 'stall %s\n' "$*" >>/case/evidence/sync-calls.txt
    sleep 600
fi
printf 'noop %s\n' "$*" >>/case/evidence/sync-calls.txt
exit 0
SH
    cat >bin/tar <<'SH'
#!/usr/bin/env bash
exit 0
SH
    cat >bin/docker <<'SH'
#!/usr/bin/env bash
exit 0
SH
    cat >bin/poetry <<'SH'
#!/usr/bin/env bash
trap 'date -u +%FT%TZ >/case/evidence/workload-stopped.txt; exit 143' TERM INT
printf '%s\n' "$*" >/case/evidence/workload-started.txt
while :; do sleep 0.1; done
SH
    printf '#!/usr/bin/env bash\nexit 1\n' >bin/curl
    cp bin/curl bin/oci
    chmod +x bin/*
    : >evidence/sync-calls.txt
    (cd repo && sha256sum scripts/run-merge-recovery-soak.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json) >evidence/source-sha256.txt
    cat >evidence/unrelated.sh <<'SH'
#!/usr/bin/env bash
trap '' TERM
for n in $(seq 1 600); do printf '%s\n' "$n" >>/case/evidence/unrelated.writes; sleep 0.1; done
SH
    setsid bash evidence/unrelated.sh >/dev/null 2>&1 &
    UNRELATED=$!
    DEADLINE_SECONDS=15
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
        [[ ! -s evidence/workload-started.txt ]] || break
        kill -0 "$DRIVER" 2>/dev/null || break
        sleep 0.1
    done
    if [[ ! -s evidence/workload-started.txt ]]; then
        stop_driver
        printf 'ERROR: The workload did not start within the fixture budget.\n' >&2
        exit 2
    fi
    read -r breach_at _ </proc/uptime
    breach_at="${breach_at%%.*}"
    budget=$((DEADLINE_SECONDS + SLACK_SECONDS))
    exited=no
    for _ in $(seq 1 $((budget * 10))); do
        if ! kill -0 "$DRIVER" 2>/dev/null; then
            exited=yes
            break
        fi
        sleep 0.1
    done
    read -r finished_at _ </proc/uptime
    finished_at="${finished_at%%.*}"
    printf 'breach=%s\nfinished=%s\nbudget=%s\nexited=%s\n' "$breach_at" "$finished_at" "$budget" "$exited" >evidence/publication-bound.txt
    status=0
    if [[ "$exited" == yes ]]; then
        wait "$DRIVER" || status=$?
        DRIVER=""
    else
        stop_driver
        status=143
    fi
    printf '%s\n' "$status" >evidence/driver-exit.txt
    ps -o stat= -p "$UNRELATED" >evidence/unrelated-after.txt 2>/dev/null || true
    if [[ "$exited" != yes ]]; then
        printf 'FAIL: The driver did not complete its bounded response within the deadline. A stalled storage sync blocked the emergency response.\n' >&2
        exit 1
    fi
    if [[ ! -s evidence/output/host-guardian-breach.txt ]]; then
        printf 'FAIL: The guardian breach record never became visible while the storage sync stalled.\n' >&2
        exit 1
    fi
    if [[ ! -s evidence/output/publication-unconfirmed.txt ]]; then
        printf 'FAIL: The stalled durability sync did not produce an explicit unconfirmed record.\n' >&2
        exit 1
    fi
    if [[ "$status" != 1 ]] || ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
        ! jq -e '[.iterations,.failures,.bench_segments,.bench_failures] == [1,1,0,0]' evidence/output/summary.json >/dev/null; then
        printf 'FAIL: The bounded publication lost the protection failure.\n' >&2
        exit 1
    fi
    trap - ERR
    printf 'PASS: The writer stop and the failure publication completed within the deadline despite a stalled storage sync, the breach record was visible, and the unconfirmed durability result was recorded.\n'
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

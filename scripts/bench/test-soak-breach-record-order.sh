#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == --inside ]]; then
    [[ -f /.dockerenv && "$(id -u)" == 65534 && ! -S /var/run/docker.sock ]] || exit 2
    cd /case
    mkdir -p evidence bin harness tmp runner
    trap 'printf "ERROR: The breach record order fixture failed before its behavioral verdict.\n" >&2; exit 2' ERR
    DRIVER=""
    stop_driver() {
        [[ -z "$DRIVER" ]] || kill -TERM -- "-$DRIVER" 2>/dev/null || true
        [[ -z "$DRIVER" ]] || wait "$DRIVER" 2>/dev/null || true
        DRIVER=""
    }
    cat >bin/df <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$$" >>/case/evidence/df-calls.txt
printf 'Filesystem 1024-blocks Used Available Capacity Mounted on\n'
if [[ -e /case/evidence/reclaimed.txt ]]; then
    printf 'fixture 32768 29696 3072 91%% /case\n'
else
    printf 'fixture 32768 26624 6144 82%% /case\n'
fi
SH
    cat >bin/du <<'SH'
#!/usr/bin/env bash
trap '' TERM
if [[ -e /case/evidence/output/protection-breach.txt && -e /case/evidence/output/early-exit.txt ]]; then
    printf 'present\n' >>/case/evidence/record-at-attribution.txt
else
    printf 'absent\n' >>/case/evidence/record-at-attribution.txt
fi
date -u +%FT%TZ >>/case/evidence/attribution-started.txt
sleep 600
SH
    cat >bin/docker <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == system && "${2:-}" == df ]]; then
    printf 'TYPE TOTAL ACTIVE SIZE RECLAIMABLE\n'
    exit 0
fi
if [[ "${1:-}" == builder || "${1:-}" == image || "${1:-}" == container || "${1:-}" == volume ]]; then
    printf 'reclaimed\n' >>/case/evidence/reclaimed.txt
fi
exit 0
SH
    cat >bin/poetry <<'SH'
#!/usr/bin/env bash
trap 'exit 143' TERM INT
printf 'An iteration was admitted.\n' >>/case/evidence/admitted.txt
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
    driver() {
        exec env PATH="/case/bin:$PATH" SYSTEM_INTEGRATION_DIR=/case/harness SOAK_OUTPUT_DIR=/case/evidence/output \
            SOAK_TMP_ROOT=/case/tmp SOAK_RUNNER_ROOT=/case/runner SOAK_DURATION_SECONDS=1200 \
            SOAK_RUN_BENCHMARKS=false SOAK_RSS_CEILING_MB=0 SOAK_HOST_FREE_FLOOR_MB=0 SOAK_DISK_FREE_FLOOR_MB=4096 \
            SOAK_DISK_HYGIENE_BAND_MB=4096 SOAK_DISK_STOP_SECONDS=2 SOAK_DISK_HYGIENE_SECONDS=5 \
            SOAK_DISK_DIAGNOSTIC_SECONDS=10 SOAK_GUARDIAN_POLL_SECONDS=0.1 \
            setsid bash repo/scripts/run-merge-recovery-soak.sh
    }
    (driver) >evidence/driver.log 2>&1 &
    DRIVER=$!
    for _ in $(seq 1 450); do
        kill -0 "$DRIVER" 2>/dev/null || break
        [[ "$(wc -l <evidence/attribution-started.txt 2>/dev/null || echo 0)" -lt 2 ]] || break
        sleep 0.1
    done
    if [[ ! -s evidence/attribution-started.txt ]]; then
        stop_driver
        printf 'ERROR: The driver did not reach any attribution within the fixture budget.\n' >&2
        exit 2
    fi
    running=0
    kill -0 "$DRIVER" 2>/dev/null && running=1
    printf '%s\n' "$running" >evidence/driver-running-at-check.txt
    cp evidence/output/protection-breach.txt evidence/protection-breach-at-check.txt 2>/dev/null || true
    cp evidence/output/early-exit.txt evidence/early-exit-at-check.txt 2>/dev/null || true
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
    [[ ! -e evidence/admitted.txt && -e evidence/reclaimed.txt ]]
    if [[ "$(tail -1 evidence/record-at-attribution.txt)" != present ]]; then
        printf 'FAIL: Disk attribution started before the minimal breach record was published.\n' >&2
        exit 1
    fi
    if ! grep -q 'disk floor' evidence/protection-breach-at-check.txt || ! grep -q '^host_protection_breach: disk floor' evidence/early-exit-at-check.txt; then
        printf 'FAIL: The minimal breach record was incomplete when attribution started.\n' >&2
        exit 1
    fi
    if [[ "$running" == 1 ]] || ! jq -e '[.iterations,.failures,.bench_segments,.bench_failures] == [0,1,0,0]' evidence/output/summary.json >/dev/null; then
        printf 'FAIL: A stalled attribution delayed failure publication past the fixture budget.\n' >&2
        exit 1
    fi
    trap - ERR
    printf 'PASS: The minimal breach record was published before disk attribution started, and a stalled attribution could not delay it.\n'
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
timeout --signal=TERM --kill-after=5 90 docker start -a "$CID" >"$EVIDENCE/result.txt" 2>&1 || status=$?
printf '%s\n' "$status" >"$EVIDENCE/fixture-exit.txt"
docker inspect "$CID" >"$EVIDENCE/container-after.json"
jq -e '.[0].State | .Running == false and .OOMKilled == false' "$EVIDENCE/container-after.json" >/dev/null || exit 2
[[ "$(jq -r '.[0].State.ExitCode' "$EVIDENCE/container-after.json")" == "$status" ]] || exit 2
docker cp "$CID:/case/evidence" "$EVIDENCE/evidence"
exit "$status"

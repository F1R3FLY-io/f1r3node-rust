#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == --inside ]]; then
    [[ -f /.dockerenv && "$(id -u)" == 65534 && ! -S /var/run/docker.sock ]] || exit 2
    cd /case
    mkdir -p evidence bin harness/integration-tests/data/session-1 tmp runner
    trap 'printf "ERROR: The durable record fixture failed before its behavioral verdict.\n" >&2; exit 2' ERR
    DRIVER=""
    POLLER=""
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
printf 'valid=%s\n' "$available" >>/case/evidence/probe-samples.txt
printf 'Filesystem 1M-blocks Used Available Capacity Mounted on\n'
printf '/dev/fixture 47000 %s %s 85%% /\n' "$((47000 - available))" "$available"
SH
    cat >bin/sync <<'SH'
#!/usr/bin/env bash
for target in "$@"; do
    if [[ -d "$target" ]]; then
        printf 'dir - - %s\n' "$target" >>/case/evidence/sync-calls.txt
    elif [[ -f "$target" ]]; then
        printf 'file %s %s %s\n' "$(stat -c %s "$target")" "$(sha256sum "$target" | cut -c1-64)" "$target" >>/case/evidence/sync-calls.txt
    else
        printf 'missing - - %s\n' "$target" >>/case/evidence/sync-calls.txt
    fi
done
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
    : >evidence/sync-calls.txt
    (cd repo && sha256sum scripts/run-merge-recovery-soak.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json) >evidence/source-sha256.txt
    cat >evidence/poller.sh <<'SH'
#!/usr/bin/env bash
declare -A last
while :; do
    for name in host-guardian-breach.txt protection-breach.txt early-exit.txt summary.txt summary.json .soak-state; do
        path="/case/evidence/output/$name"
        size=absent
        [[ ! -e "$path" ]] || size="$(stat -c %s "$path" 2>/dev/null || echo absent)"
        if [[ "${last[$name]:-}" != "$size" ]]; then
            printf '%s %s\n' "$name" "$size" >>/case/evidence/observations.txt
            last[$name]="$size"
        fi
    done
    sleep 0.01
done
SH
    setsid bash evidence/poller.sh >/dev/null 2>&1 &
    POLLER=$!
    driver() {
        exec env PATH="/case/bin:$PATH" SYSTEM_INTEGRATION_DIR=/case/harness SOAK_OUTPUT_DIR=/case/evidence/output \
            SOAK_TMP_ROOT=/case/tmp SOAK_RUNNER_ROOT=/case/runner SOAK_DURATION_SECONDS=1200 \
            SOAK_RUN_BENCHMARKS=false SOAK_RSS_CEILING_MB=0 SOAK_HOST_FREE_FLOOR_MB=0 SOAK_DISK_FREE_FLOOR_MB=4096 \
            SOAK_DISK_HYGIENE_BAND_MB=4096 SOAK_DISK_STOP_SECONDS=2 SOAK_DISK_DIAGNOSTIC_SECONDS=2 \
            SOAK_EMERGENCY_DEADLINE_SECONDS=30 SOAK_GUARDIAN_POLL_SECONDS=0.1 \
            setsid bash repo/scripts/run-merge-recovery-soak.sh
    }
    (driver) >evidence/driver.log 2>&1 &
    DRIVER=$!
    for _ in $(seq 1 600); do
        kill -0 "$DRIVER" 2>/dev/null || break
        sleep 0.1
    done
    status=0
    if kill -0 "$DRIVER" 2>/dev/null; then
        stop_driver
        status=143
    else
        wait "$DRIVER" || status=$?
        DRIVER=""
    fi
    printf '%s\n' "$status" >evidence/driver-exit.txt
    kill -TERM -- "-$POLLER" 2>/dev/null || true
    wait "$POLLER" 2>/dev/null || true
    grep -c ' 0$' evidence/observations.txt >evidence/partial-observations.txt || true
    if [[ ! -s evidence/workload-started.txt || ! -s evidence/output/host-guardian-breach.txt ]]; then
        printf 'ERROR: The guardian did not fire during the iteration within the fixture budget.\n' >&2
        exit 2
    fi
    if [[ "$status" != 1 ]] || ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
        ! jq -e '[.iterations,.failures,.bench_segments,.bench_failures] == [1,1,0,0]' evidence/output/summary.json >/dev/null; then
        printf 'ERROR: The fixture did not reach a published protection failure.\n' >&2
        exit 2
    fi
    trap - ERR
    incomplete=""
    for name in host-guardian-breach.txt protection-breach.txt early-exit.txt summary.txt summary.json .soak-state; do
        path="/case/evidence/output/$name"
        final="$(sha256sum "$path" | cut -c1-64)"
        synced_line="$(awk -v prefix="$path.tmp." -v digest="$final" \
            '$1 == "file" && $3 == digest && index($4, prefix) == 1 { line = NR } END { print line + 0 }' evidence/sync-calls.txt)"
        directory_line="$(awk -v after="$synced_line" \
            '$1 == "dir" && $4 == "/case/evidence/output" && NR > after { print NR; exit }' evidence/sync-calls.txt)"
        in_place="$(awk -v target="$path" '$1 == "file" && $4 == target' evidence/sync-calls.txt)"
        if [[ "$synced_line" == 0 || -z "$directory_line" || -n "$in_place" ]]; then
            incomplete="$incomplete $name"
        fi
        printf '%s synced_line=%s directory_line=%s in_place=%s\n' "$name" "$synced_line" "${directory_line:-none}" "${in_place:+yes}" >>evidence/record-durability.txt
    done
    if compgen -G '/case/evidence/output/*.tmp.*' >/dev/null || compgen -G '/case/evidence/output/.*.tmp.*' >/dev/null; then
        incomplete="$incomplete leftover-temporary-file"
    fi
    if [[ -n "$incomplete" ]]; then
        printf 'FAIL: The driver published a record without a synced temporary file, an atomic rename, and a directory sync:%s\n' "$incomplete" >&2
        exit 1
    fi
    printf 'PASS: Every minimal record was synced under a temporary name, renamed into place, and followed by a directory sync, and no partial record was left behind.\n'
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

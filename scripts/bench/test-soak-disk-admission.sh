#!/usr/bin/env bash
set -euo pipefail

SOURCE_FILES=(
    scripts/run-merge-recovery-soak.sh
    scripts/bench/write-soak-summary.sh
    scripts/bench/collect-soak-metrics.sh
    scripts/bench/soak-metrics.json
)

if [[ "${1:-}" == --inside ]]; then
    trap 'printf "ERROR: The container fixture failed before its behavioral verdict.\n" >&2; exit 2' ERR
    [[ -f /.dockerenv && "$(id -u)" == 65534 && ! -S /var/run/docker.sock ]] || exit 2
    cd /case
    for tool in bash jq timeout find awk sed tar ps perl; do
        command -v "$tool" >/dev/null || exit 2
    done
    mkdir -p evidence bin harness
    (cd repo && sha256sum "${SOURCE_FILES[@]}") >evidence/source-sha256.txt
    dpkg-query -W bash coreutils findutils mawk jq procps 2>/dev/null >evidence/packages.txt || true
    cat >bin/df <<'SH'
#!/usr/bin/env bash
printf 'Filesystem 1M-blocks Used Available Capacity Mounted on\n'
printf '/dev/fixture 47000 40000 7000 85%% /\n'
SH
    cat >bin/docker <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>/case/evidence/docker-commands.txt
exit 0
SH
    cat >bin/poetry <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >/case/evidence/workload-started.txt
printf 'finalize\n' >/case/evidence/output/signal
printf 'The boundary workload fixture completed.\n'
SH
    chmod +x bin/*
    status=0
    PATH="/case/bin:$PATH" \
        SOAK_DURATION_SECONDS=30 \
        SYSTEM_INTEGRATION_DIR=/case/harness \
        SOAK_OUTPUT_DIR=/case/evidence/output \
        SOAK_TARGET_REF=disk-admission-fixture \
        SOAK_TARGET_SHA="${SOAK_DISK_TEST_SOURCE_SHA:-unknown}" \
        SOAK_RSS_CEILING_MB=0 \
        SOAK_HOST_FREE_FLOOR_MB=0 \
        SOAK_DISK_FREE_FLOOR_MB=4096 \
        SOAK_DISK_HYGIENE_BAND_MB=4096 \
        SOAK_TMP_ROOT=/tmp \
        SOAK_RUNNER_ROOT=/case/runner \
        SOAK_RUN_BENCHMARKS=false \
        SOAK_MERGE_EXIT_MIN_SECONDS=0 \
        SOAK_GUARDIAN_POLL_SECONDS=0.05 \
        SOAK_MONITOR_SNAPSHOT_SECONDS=0.1 \
        timeout --signal=TERM --kill-after=2 20 \
        bash repo/scripts/run-merge-recovery-soak.sh >evidence/driver.log 2>&1 || status=$?
    printf '%s\n' "$status" >evidence/driver-exit.txt
    if [[ "$status" != 0 && "$status" != 1 ]] ||
        [[ ! -s evidence/output/summary.json ]] ||
        ! grep -Fxq 'disk hygiene: 7000MB free -> 7000MB free' evidence/driver.log ||
        ! jq -e 'has("degraded") | not' evidence/output/summary.json >/dev/null; then
        printf 'ERROR: The fixture did not complete the required driver path.\n' >&2
        exit 2
    fi
    iterations="$(find evidence/output -maxdepth 1 -type d -name 'iteration-*' | wc -l | tr -d ' ')"
    if [[ "$iterations" != 0 || -e evidence/workload-started.txt ]]; then
        printf 'FAIL: Disk hygiene left 7000 MiB below 8192 MiB, but the driver admitted %s iteration(s).\n' "$iterations" >&2
        exit 1
    fi
    if [[ "$status" != 1 ]] ||
        ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
        ! grep -Fxq 'host_protection_breach: disk floor: free 7000MB still inside hygiene band (floor 4096MB + band 4096MB) after hygiene' evidence/output/early-exit.txt ||
        ! jq -e '.iterations == 0 and .failures == 1' evidence/output/summary.json >/dev/null; then
        printf 'FAIL: Refused admission lacks the required failure result and evidence.\n' >&2
        exit 1
    fi
    printf 'PASS: Disk hygiene left 7000 MiB below 8192 MiB. No iteration started, and the driver recorded refusal.\n'
    exit 0
fi

if (($# > 2)); then
    printf 'Usage: %s [source-directory] [evidence-directory]\n' "$0" >&2
    exit 2
fi
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOURCE="$(cd "${1:-$ROOT}" && pwd)"
OUTPUT="${2:-$(mktemp -d)}"
mkdir -p "$OUTPUT"
OUTPUT="$(cd "$OUTPUT" && pwd)"
if [[ -n "$(find "$OUTPUT" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
    printf 'ERROR: The evidence directory must be empty.\n' >&2
    exit 2
fi
for file in "${SOURCE_FILES[@]}"; do
    [[ -f "$SOURCE/$file" && ! -L "$SOURCE/$file" ]] || exit 2
done
command -v docker >/dev/null
command -v timeout >/dev/null
command -v jq >/dev/null
CONTAINER=""
cleanup() {
    if [[ "$CONTAINER" =~ ^[0-9a-f]{64}$ ]]; then
        docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT
IMAGE="${SOAK_DISK_TEST_IMAGE:-}"
if [[ -z "$IMAGE" ]]; then
    timeout --signal=TERM --kill-after=5 180 docker build \
        --iidfile "$OUTPUT/image-id.txt" - <"$ROOT/scripts/bench/soak-disk-test.Dockerfile" \
        >"$OUTPUT/image-build.log" 2>&1
    IMAGE="$(<"$OUTPUT/image-id.txt")"
fi
[[ "$IMAGE" =~ ^sha256:[0-9a-f]{64}$ ]] || exit 2
docker image inspect "$IMAGE" >"$OUTPUT/image-inspect.json"
jq -e '.[0].Config.Volumes == null or (.[0].Config.Volumes | length) == 0' \
    "$OUTPUT/image-inspect.json" >/dev/null
CONTAINER="$(docker create --pull=never --network none --cap-drop ALL \
    --security-opt no-new-privileges --pids-limit 128 --memory 256m --cpus 1 \
    --user 65534:65534 --workdir /case \
    --env "SOAK_DISK_TEST_SOURCE_SHA=${SOAK_DISK_TEST_SOURCE_SHA:-unknown}" \
    --entrypoint bash "$IMAGE" /case/test.sh --inside)"
[[ "$CONTAINER" =~ ^[0-9a-f]{64}$ ]] || exit 2
docker inspect "$CONTAINER" >"$OUTPUT/container-inspect.json"
jq -e '.[0] | (.Mounts | length) == 0 and .HostConfig.NetworkMode == "none"
    and .HostConfig.Privileged == false and .HostConfig.PidMode == ""
    and .Config.User == "65534:65534" and .HostConfig.CapDrop == ["ALL"]
    and (.HostConfig.SecurityOpt | index("no-new-privileges") != null)' \
    "$OUTPUT/container-inspect.json" >/dev/null
tar -C "$SOURCE" --mode='a+rX' -cf - "${SOURCE_FILES[@]}" |
    docker cp - "$CONTAINER:/case/repo/"
docker cp "${BASH_SOURCE[0]}" "$CONTAINER:/case/test.sh"
status=0
timeout --signal=TERM --kill-after=5 40 docker start -a "$CONTAINER" \
    >"$OUTPUT/result.txt" 2>&1 || status=$?
docker inspect "$CONTAINER" >"$OUTPUT/container-finished.json"
if ! jq -e '.[0].State.Running == false and .[0].State.OOMKilled == false' \
    "$OUTPUT/container-finished.json" >/dev/null; then
    printf 'ERROR: The fixture exceeded its container bounds.\n' >&2
    exit 2
fi
container_status="$(jq -r '.[0].State.ExitCode' "$OUTPUT/container-finished.json")"
if [[ "$status" != "$container_status" ]]; then
    printf 'ERROR: Docker did not return the container verdict.\n' >&2
    exit 2
fi
docker cp "$CONTAINER:/case/evidence" "$OUTPUT/evidence"
printf '%s\n' "$status" >"$OUTPUT/test-exit.txt"
cat "$OUTPUT/result.txt"
printf 'Evidence: %s\n' "$OUTPUT"
exit "$status"

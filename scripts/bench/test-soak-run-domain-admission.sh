#!/usr/bin/env bash
set -euo pipefail

MODE="${SOAK_RUN_DOMAIN_MODE:-iteration}"
[[ "$MODE" == benchmark || "$MODE" == iteration ]] || exit 2
if [[ "${1:-}" == --inside ]]; then
    [[ -f /.dockerenv && "$(id -u)" == 0 && ! -S /var/run/docker.sock ]] || exit 2
    cgroup="$(cut -d: -f3- /proc/self/cgroup)"
    [[ "$(wc -l </proc/self/cgroup)" == 1 && "$cgroup" == /* ]] || exit 2
    mkdir -m 0755 /run/soak-run-domain
    printf '%s\n' "$cgroup" >/run/soak-run-domain/fixture-cgroup.txt
    printf '{"unit":"fixture-run-domain.service","cgroup":"%s","uid":65534}\n' "$cgroup" >/run/soak-run-domain/matching.json
    printf '{"unit":"fixture-other.service","cgroup":"/system.slice/fixture-other.scope","uid":65534}\n' >/run/soak-run-domain/mismatched.json
    chmod 0644 /run/soak-run-domain/*
    exec setpriv --reuid 65534 --regid 65534 --clear-groups -- bash /case/test.sh --inside-worker
fi
if [[ "${1:-}" == --inside-worker ]]; then
    [[ -f /.dockerenv && "$(id -u)" == 65534 && ! -S /var/run/docker.sock ]] || exit 2
    cd /case
    mkdir -p evidence bin harness tmp runner node/docker
    trap 'printf "ERROR: The run-domain admission fixture failed before its behavioral verdict.\n" >&2; exit 2' ERR
    DRIVER=""
    stop_driver() {
        [[ -z "$DRIVER" ]] || kill -TERM -- "-$DRIVER" 2>/dev/null || true
        [[ -z "$DRIVER" ]] || wait "$DRIVER" 2>/dev/null || true
        DRIVER=""
    }
    stat -c '%u %a %n' /run/soak-run-domain /run/soak-run-domain/* >evidence/records.txt
    cp /run/soak-run-domain/fixture-cgroup.txt evidence/fixture-cgroup.txt
    printf '{"services":{"writer":{"image":"fixture-unused"}}}\n' >node/docker/shard.yml
    cat >bin/df <<'SH'
#!/usr/bin/env bash
printf 'Filesystem 1024-blocks Used Available Capacity Mounted on\n'
printf 'fixture 32768 16384 16384 50%% /case\n'
SH
    cat >bin/docker <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == compose && " $* " == *' config '* ]]; then
    printf '{"services":{"writer":{"image":"fixture-unused"}}}\n'
elif [[ "${1:-}" == compose && " $* " == *' up '* ]]; then
    printf 'A benchmark was admitted.\n' >>"$SOAK_CASE_DIR/admitted.txt"
fi
exit 0
SH
    cat >bin/poetry <<'SH'
#!/usr/bin/env bash
trap 'exit 143' TERM INT
printf 'An iteration was admitted.\n' >>"$SOAK_CASE_DIR/admitted.txt"
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
        local case="$1" record="$2"
        exec env PATH="/case/bin:$PATH" SYSTEM_INTEGRATION_DIR=/case/harness SOAK_OUTPUT_DIR="/case/evidence/$case/output" \
            SOAK_CASE_DIR="/case/evidence/$case" SOAK_CONTAINMENT=required SOAK_RUN_DOMAIN_RECORD="$record" \
            SOAK_TMP_ROOT=/case/tmp SOAK_RUNNER_ROOT=/case/runner SOAK_DURATION_SECONDS=1200 \
            SOAK_RUN_BENCHMARKS="$benchmarks" SOAK_BENCH_DURATION=1 SOAK_NODE_REPO_DIR=/case/node DEPLOYER_KEY=fixture-unused \
            SOAK_RSS_CEILING_MB=0 SOAK_HOST_FREE_FLOOR_MB=0 SOAK_DISK_FREE_FLOOR_MB=4096 \
            SOAK_DISK_HYGIENE_BAND_MB=4096 SOAK_DISK_STOP_SECONDS=2 SOAK_GUARDIAN_POLL_SECONDS=0.1 \
            setsid bash repo/scripts/run-merge-recovery-soak.sh
    }
    refused() {
        local case="$1" pass="$2"
        cp "evidence/$case/output/summary.json" "evidence/$case/$pass-summary.json"
        if [[ -e "evidence/$case/admitted.txt" ]] || ! jq -e '.iterations == 0 and .bench_segments == 0' "evidence/$case/$pass-summary.json" >/dev/null; then
            printf 'FAIL: The driver admitted %s work without a verified run domain (%s, %s).\n' "$MODE" "$case" "$pass" >&2
            exit 1
        fi
        if ! grep -q 'run domain' "evidence/$case/output/host-guardian-breach.txt" 2>/dev/null || ! jq -e '.failures == 1 and .bench_failures == 0' "evidence/$case/$pass-summary.json" >/dev/null; then
            printf 'FAIL: Run-domain refusal lost or duplicated its retained failure (%s, %s).\n' "$case" "$pass" >&2
            exit 1
        fi
    }
    for case in absent mismatched; do
        mkdir -p "evidence/$case"
        record="/run/soak-run-domain/$case.json"
        for pass in initial restart-1 restart-2; do
            status=0
            (driver "$case" "$record") >"evidence/$case/$pass.log" 2>&1 &
            DRIVER=$!
            for _ in $(seq 1 100); do
                kill -0 "$DRIVER" 2>/dev/null || break
                sleep 0.1
            done
            if kill -0 "$DRIVER" 2>/dev/null; then
                stop_driver
                if [[ -e "evidence/$case/admitted.txt" ]]; then
                    printf 'FAIL: The driver admitted %s work without a verified run domain (%s, %s).\n' "$MODE" "$case" "$pass" >&2
                    exit 1
                fi
                printf 'ERROR: The driver neither admitted nor refused work within the fixture budget (%s, %s).\n' "$case" "$pass" >&2
                exit 2
            fi
            wait "$DRIVER" || status=$?
            DRIVER=""
            printf '%s\n' "$status" >"evidence/$case/$pass-exit.txt"
            if [[ "$status" == 0 ]]; then
                printf 'FAIL: The driver returned success without a verified run domain (%s, %s).\n' "$case" "$pass" >&2
                exit 1
            fi
            refused "$case" "$pass"
            [[ "$case" == absent ]] || break
        done
    done
    mkdir -p evidence/matching
    (driver matching /run/soak-run-domain/matching.json) >evidence/matching/initial.log 2>&1 &
    DRIVER=$!
    for _ in $(seq 1 150); do
        [[ ! -s evidence/matching/admitted.txt ]] || break
        kill -0 "$DRIVER" 2>/dev/null || break
        sleep 0.1
    done
    cp evidence/matching/output/.soak-state evidence/matching/state-after-admission.txt 2>/dev/null || true
    stop_driver
    if [[ "$(wc -l <evidence/matching/admitted.txt 2>/dev/null || echo 0)" != 1 ]] || [[ -s evidence/matching/output/host-guardian-breach.txt ]]; then
        printf 'FAIL: A matching trusted record did not admit %s work.\n' "$MODE" >&2
        exit 1
    fi
    ps -o stat= -p "$UNRELATED" >evidence/unrelated-after.txt
    cp evidence/unrelated.writes evidence/unrelated-before.writes
    sleep 0.5
    cp evidence/unrelated.writes evidence/unrelated-after.writes
    [[ "$(<evidence/unrelated-after.txt)" != Z* ]] && ! cmp -s evidence/unrelated-before.writes evidence/unrelated-after.writes
    trap - ERR
    printf 'PASS: Required containment refuses %s admission without a matching trusted run-domain record, retains one failure across two refused restarts, and admits with a matching record.\n' "$MODE"
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
CID="$(docker create --pull=never --network none --cap-drop ALL --cap-add SETUID --cap-add SETGID --security-opt no-new-privileges --pids-limit 128 --memory 256m --cpus 1 --user 0:0 --workdir /case --env "SOAK_RUN_DOMAIN_MODE=$MODE" --entrypoint bash "$IMAGE" /case/test.sh --inside)"
[[ "$CID" =~ ^[a-f0-9]{64}$ ]]
docker inspect "$CID" >"$EVIDENCE/container-before.json"
jq -e '.[0] | (.Mounts | length) == 0 and .HostConfig.NetworkMode == "none" and .HostConfig.Privileged == false and .HostConfig.PidMode == "" and .Config.User == "0:0" and .HostConfig.CapDrop == ["ALL"] and (.HostConfig.CapAdd | sort) == ["CAP_SETGID", "CAP_SETUID"] and .HostConfig.Memory == 268435456 and .HostConfig.PidsLimit == 128 and (.HostConfig.SecurityOpt | index("no-new-privileges") != null)' "$EVIDENCE/container-before.json" >/dev/null
tar -C "$SOURCE" --mode='a+rX' -cf - scripts/run-merge-recovery-soak.sh scripts/bench/run-bench-segment.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json | docker cp - "$CID:/case/repo/"
docker cp "${BASH_SOURCE[0]}" "$CID:/case/test.sh"
status=0
timeout --signal=TERM --kill-after=5 60 docker start -a "$CID" >"$EVIDENCE/result.txt" 2>&1 || status=$?
printf '%s\n' "$status" >"$EVIDENCE/fixture-exit.txt"
docker inspect "$CID" >"$EVIDENCE/container-after.json"
jq -e '.[0].State | .Running == false and .OOMKilled == false' "$EVIDENCE/container-after.json" >/dev/null || exit 2
[[ "$(jq -r '.[0].State.ExitCode' "$EVIDENCE/container-after.json")" == "$status" ]] || exit 2
docker cp "$CID:/case/evidence" "$EVIDENCE/evidence"
exit "$status"

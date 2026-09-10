#!/usr/bin/env bash
# Runs the real soak driver inside a disposable container (no host mounts, no
# network, no Docker socket, unprivileged user) against fixture df/docker/poetry
# commands, one scenario per container. Each scenario is a behavioral check of
# scripts/run-merge-recovery-soak.sh's disk protection:
#
#   band                  hygiene cannot lift free space out of the band -> refuse
#   missing-boundary      df returns nothing at the boundary probe        -> refuse
#   missing-after-hygiene df returns nothing after hygiene                -> refuse
#   malformed-boundary    df prints "16384junk"                           -> refuse
#   missing-active        df fails while an iteration runs               -> stop it
#   stalled-active        df prints a field, then stalls past its deadline -> stop it
#   record-before-stop    the breach record exists before docker kill
#   guardian-death        the watcher notices a dead guardian             -> stop it
#   diagnostic-deadline   du stalls on 32 roots; attribution stays bounded
#   restart-uncounted     a retained breach marker blocks the next segment (failures 0 -> 1)
#   restart-counted       same, with a prior failure count that stays at 2
#   stop-timeout          pkill/docker kill stall; the stop is bounded and the failure still publishes
#   guardian-death-boundary the guardian dies during the boundary probe -> refuse
#   restart-benchmark     a retained breach marker blocks the opening benchmark (no state file)
#   benchmark-band        a 7000 MiB sample below floor plus band blocks the opening benchmark
#   benchmark-active-disk the disk falls to 1024 MiB during the benchmark; the guardian records and stops it
#   benchmark-equal       8192 MiB, exactly floor plus band              -> benchmark admitted
#   benchmark-sufficient  16384 MiB                                       -> benchmark admitted
#   benchmark-missing     df returns nothing at the benchmark probe       -> refuse
#   benchmark-disabled    disk protection off, 7000 MiB                   -> benchmark admitted
#   benchmark-cancel-death the guardian dies during the benchmark; the benchmark is cancelled and the failure published
#   benchmark-cancel-breach the guardian records a breach during the benchmark; same cancellation
#   benchmark-guardian-boundary the guardian dies during the opening benchmark's disk probe -> refuse
#   benchmark-guardian-interleaved same, at an interleaved benchmark after one iteration -> refuse
#   benchmark-cancel-stall the guardian is SIGSTOPped during the benchmark; no progress -> cancel it
#   guardian-stall        the guardian is SIGSTOPped during an iteration; no progress -> stop it
#   guardian-progress-boundary driver and guardian paused across the iteration probe; stale progress -> refuse
#   benchmark-progress-boundary same, across the opening benchmark probe -> refuse
#   hygiene-timeout       docker system df ignores TERM inside the band; hygiene is bounded -> refuse
#   disk-floor-range      SOAK_DISK_FREE_FLOOR_MB above the 64-bit maximum   -> configuration rejected, exit 2
#   disk-band-range       SOAK_DISK_HYGIENE_BAND_MB above the 64-bit maximum -> configuration rejected, exit 2
#   disk-sum-range        floor plus band above the 64-bit maximum           -> configuration rejected, exit 2
#   disk-max-floor        floor exactly at the 64-bit maximum, band 0        -> accepted, then refused on the sample
#   disk-max-band         band exactly at the 64-bit maximum, floor 0        -> accepted, then refused on the sample
#   cleanup-active-session an unowned two-hour-old session with a live writer survives hygiene
#   cleanup-error-list    docker ps fails during hygiene; the failure is kept -> refuse
#   cleanup-error-remove  docker inspect fails during hygiene                -> refuse
#   cleanup-error-network docker network ls fails                             -> refuse
#   cleanup-error-image   docker image ls fails                               -> refuse
#   cleanup-error-builder docker system df fails                              -> refuse
#   cleanup-partial       hygiene reclaims to 8000 MiB, still below the band  -> refuse
#   cleanup-sufficient    hygiene reclaims to 16384 MiB                       -> one iteration
#
# Usage: test-soak-disk-admission.sh [--scenario NAME] [source-directory] [evidence-directory]
#   With no --scenario (and no SOAK_DISK_TEST_SCENARIO) every scenario runs
#   against one image build, each into <evidence-directory>/<scenario>.
#   Exit 0 = all pass, 1 = a behavioral failure, 2 = the fixture itself broke.
set -euo pipefail

SOURCE_FILES=(
    scripts/run-merge-recovery-soak.sh
    scripts/bench/write-soak-summary.sh
    scripts/bench/collect-soak-metrics.sh
    scripts/bench/soak-metrics.json
    scripts/bench/run-bench-segment.sh
)
SCENARIOS=(band missing-boundary missing-after-hygiene malformed-boundary missing-active
    stalled-active record-before-stop guardian-death diagnostic-deadline restart-uncounted
    restart-counted stop-timeout guardian-death-boundary restart-benchmark benchmark-band
    benchmark-active-disk benchmark-equal benchmark-sufficient benchmark-missing benchmark-disabled
    benchmark-cancel-death benchmark-cancel-breach benchmark-guardian-boundary benchmark-guardian-interleaved
    benchmark-cancel-stall guardian-stall guardian-progress-boundary benchmark-progress-boundary
    hygiene-timeout disk-floor-range disk-band-range disk-sum-range disk-max-floor disk-max-band
    cleanup-active-session cleanup-error-list cleanup-error-remove cleanup-error-network
    cleanup-error-image cleanup-error-builder cleanup-partial cleanup-sufficient)

SCENARIO="${SOAK_DISK_TEST_SCENARIO:-}"
if [[ "${1:-}" == --scenario ]]; then
    SCENARIO="${2:?--scenario needs a name}"
    shift 2
fi
if [[ -n "$SCENARIO" && "$SCENARIO" != all ]] && ! printf '%s\n' "${SCENARIOS[@]}" | grep -Fxq "$SCENARIO"; then
    printf 'ERROR: Unknown disk fixture scenario %s.\n' "$SCENARIO" >&2
    exit 2
fi

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
available=7000
case "${SOAK_DISK_TEST_SCENARIO:-band}" in
    cleanup-error-* | cleanup-partial | cleanup-sufficient)
        if [[ -e /case/evidence/hygiene-completed ]]; then
            available=16384
            [[ "$SOAK_DISK_TEST_SCENARIO" != cleanup-partial ]] || available=8000
        fi
        ;;
    stalled-active)
        if [[ -f /case/evidence/workload-started.txt ]]; then
            printf 'stalled-active\n' >>/case/evidence/probe-samples.txt
            stall_after=true
        fi
        available=16384
        ;;
    guardian-progress-boundary | benchmark-progress-boundary)
        available=16384
        guardian_pid="$(awk '/^orchestrator host guardian watching/ {print $NF; exit}' /case/evidence/driver.log)"
        from_guardian=false
        ancestor="$PPID"
        for _ in $(seq 1 16); do
            if [[ "$ancestor" == "$guardian_pid" ]]; then from_guardian=true; break; fi
            [[ "$ancestor" -gt 1 ]] || break
            ancestor="$(awk '/^PPid:/ {print $2}' "/proc/$ancestor/status")" || exit 2
        done
        if [[ "$guardian_pid" =~ ^[1-9][0-9]*$ && "$from_guardian" == false && ! -e /case/evidence/admission-suspended.txt ]]; then
            driver_pid="$(awk '/^PPid:/ {print $2}' "/proc/$guardian_pid/status")" || exit 2
            [[ "$driver_pid" =~ ^[1-9][0-9]*$ ]] || exit 2
            kill -STOP "$guardian_pid" || exit 2
            printf '%s %s\n' "$driver_pid" "$guardian_pid" >/case/evidence/admission-suspended.txt
            kill -STOP "$driver_pid" || exit 2
        fi
        ;;
    benchmark-guardian-boundary | benchmark-guardian-interleaved)
        available=16384
        guardian_pid="$(awk '/^orchestrator host guardian watching/ {print $NF; exit}' /case/evidence/driver.log)"
        if [[ "$guardian_pid" =~ ^[1-9][0-9]*$ && ! -s /case/evidence/benchmark-boundary-killed.txt &&
            ( "$SOAK_DISK_TEST_SCENARIO" == benchmark-guardian-boundary || -s /case/evidence/workload-started.txt ) ]]; then
            ancestor="$PPID"
            from_guardian=false
            for _ in $(seq 1 16); do
                if [[ "$ancestor" == "$guardian_pid" ]]; then from_guardian=true; break; fi
                [[ "$ancestor" -gt 1 ]] || break
                ancestor="$(awk '/^PPid:/ {print $2}' "/proc/$ancestor/status")" || exit 2
            done
            if [[ "$from_guardian" == false ]]; then
                kill -KILL "$guardian_pid" || exit 2
                sleep 0.05
                guardian_state="$(ps -o stat= -p "$guardian_pid" || true)"
                [[ -z "$guardian_state" || "$guardian_state" == Z* ]] || exit 2
                printf '%s\n' "$guardian_pid" >/case/evidence/benchmark-boundary-killed.txt
            fi
        fi
        ;;
    guardian-death-boundary)
        available=16384
        if ! mkdir /case/evidence/startup-probe-seen 2>/dev/null &&
            [[ ! -s /case/evidence/guardian-killed-at-boundary.txt ]]; then
            guardian_pid="$(awk '/^orchestrator host guardian watching/ {print $NF; exit}' /case/evidence/driver.log)"
            [[ "$guardian_pid" =~ ^[1-9][0-9]*$ ]] || exit 2
            kill -KILL "$guardian_pid" || exit 2
            sleep 0.05
            guardian_state="$(ps -o stat= -p "$guardian_pid" || true)"
            [[ -z "$guardian_state" || "$guardian_state" == Z* ]] || exit 2
            printf '%s\n' "$guardian_pid" >/case/evidence/guardian-killed-at-boundary.txt
            printf 'boundary-guardian-killed=%s\n' "$guardian_pid" >>/case/evidence/probe-samples.txt
        fi
        ;;
    benchmark-equal)
        available=8192
        ;;
    guardian-death | restart-uncounted | restart-counted | restart-benchmark | benchmark-sufficient | benchmark-cancel-death | benchmark-cancel-stall | guardian-stall)
        available=16384
        ;;
    benchmark-active-disk | benchmark-cancel-breach)
        available=16384
        if [[ -e /case/evidence/benchmark-started.txt && ! -e /case/evidence/benchmark-returned.txt ]]; then
            available=1024
        fi
        ;;
    record-before-stop | stop-timeout)
        available=16384
        if [[ -f /case/evidence/workload-started.txt ]]; then
            available=1024
        fi
        ;;
    missing-active)
        if [[ -f /case/evidence/workload-started.txt ]]; then
            printf 'missing-active\n' >>/case/evidence/probe-samples.txt
            exit 1
        fi
        available=16384
        ;;
    missing-boundary | benchmark-missing)
        if ! mkdir /case/evidence/startup-probe-seen 2>/dev/null; then
            printf 'missing\n' >>/case/evidence/probe-samples.txt
            exit 1
        fi
        available=16384
        ;;
    malformed-boundary)
        if ! mkdir /case/evidence/startup-probe-seen 2>/dev/null; then
            printf 'malformed=16384junk\n' >>/case/evidence/probe-samples.txt
            printf 'Filesystem 1M-blocks Used Available Capacity Mounted on\n'
            printf '/dev/fixture 47000 30616 16384junk 65%% /\n'
            exit 0
        fi
        available=16384
        ;;
    missing-after-hygiene)
        if [[ -f /case/evidence/hygiene-completed ]]; then
            printf 'missing\n' >>/case/evidence/probe-samples.txt
            exit 1
        fi
        ;;
esac
printf 'valid=%s\n' "$available" >>/case/evidence/probe-samples.txt
printf 'Filesystem 1M-blocks Used Available Capacity Mounted on\n'
printf '/dev/fixture 47000 %s %s 85%% /\n' "$((47000 - available))" "$available"
if [[ "${stall_after:-false}" == true ]]; then
    sleep 4
fi
SH
    cat >bin/docker <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>/case/evidence/docker-commands.txt
owned_id=0000000000000000000000000000000000000000000000000000000000000001
if [[ "${1:-}" == ps && "$*" == *'--filter label=io.f1r3fly.soak.owner='* ]]; then
    printf '%s\n' "$owned_id"
    exit 0
fi
if [[ "${1:-}" == inspect && "${2:-}" == --format && "${4:-}" == "$owned_id" ]]; then
    printf '%s\n' "${SOAK_WRITER_OWNER:?}"
    exit 0
fi
if [[ "${1:-}" == kill && "${2:-}" == "$owned_id" ]]; then set -- kill disk-fixture; fi
if [[ "${1:-}" == rm && "${3:-}" == "$owned_id" ]]; then set -- rm -f disk-fixture; fi
if [[ "$*" == 'compose -f /case/node/docker/shard.yml -p soak-bench config --format json' ]]; then
    printf '{"services":{"fixture":{}}}\n'
    exit 0
fi
if [[ "${1:-}" == compose && "${6:-}" == -f && "${7:-}" == /case/evidence/output/.docker-owner.*/labels.*.json ]]; then
    jq -e --arg owner "${SOAK_WRITER_OWNER:?}" '.services.fixture.labels["io.f1r3fly.soak.owner"] == $owner' "$7" >/dev/null || exit 2
    set -- "${@:1:5}" "${@:8}"
fi
if [[ "${SOAK_DISK_TEST_SCENARIO:-band}" == cleanup-error-* ]]; then
    action=""
    case "$*" in
        'ps -aq --filter status=exited --filter name=rnode.') action=list; printf 'cleanup-fixture\n' ;;
        'inspect cleanup-fixture') action=remove ;;
        'network ls -q') action=network ;;
        'image ls -q') action=image ;;
        'system df') action=builder; touch /case/evidence/hygiene-completed ;;
    esac
    if [[ "$action" == "${SOAK_DISK_TEST_SCENARIO#cleanup-error-}" ]]; then
        printf '%s\n' "$action" >/case/evidence/cleanup-failed-command.txt
        exit 42
    fi
fi
if [[ ( "${SOAK_DISK_TEST_SCENARIO:-band}" == restart-benchmark ||
    ( "${SOAK_DISK_TEST_SCENARIO:-band}" == benchmark-* &&
    "${SOAK_DISK_TEST_SCENARIO:-band}" != benchmark-active-disk &&
    "${SOAK_DISK_TEST_SCENARIO:-band}" != benchmark-cancel-* ) ) &&
    "$*" == 'compose -f /case/node/docker/shard.yml -p soak-bench up -d' ]]; then
    printf '%s\n' "$*" >/case/evidence/benchmark-started.txt
    exit 1
fi
if [[ "${SOAK_DISK_TEST_SCENARIO:-band}" == benchmark-active-disk ]]; then
    case "$*" in
        'compose -f /case/node/docker/shard.yml -p soak-bench up -d')
            printf '%s\n' "$*" >/case/evidence/benchmark-started.txt
            printf 'available=1024\n' >/case/evidence/benchmark-disk-fault.txt
            observe_benchmark_return() {
                if [[ -s /case/evidence/stop-during-benchmark.txt ]]; then
                    printf 'recorded\n' >/case/evidence/benchmark-observation.txt
                else
                    printf 'missing\n' >/case/evidence/benchmark-observation.txt
                fi
                touch /case/evidence/benchmark-returned.txt
            }
            trap 'observe_benchmark_return; exit 1' TERM
            sleep 8
            observe_benchmark_return
            exit 1
            ;;
        'ps -q --filter name=rnode.') printf 'disk-fixture\n' ;;
        'kill disk-fixture')
            if [[ ! -e /case/evidence/benchmark-returned.txt &&
                -s /case/evidence/output/host-guardian-breach.txt ]]; then
                cp /case/evidence/output/host-guardian-breach.txt /case/evidence/stop-during-benchmark.txt
            fi
            ;;
    esac
fi
if [[ "${SOAK_DISK_TEST_SCENARIO:-band}" == benchmark-cancel-* &&
    "$*" == 'compose -f /case/node/docker/shard.yml -p soak-bench up -d' ]]; then
    trap '' TERM
    printf '%s\n' "$*" >/case/evidence/benchmark-started.txt
    printf '%s\n' "$$" >/case/evidence/benchmark-client-pid.txt
    if [[ "$SOAK_DISK_TEST_SCENARIO" == benchmark-cancel-death ]]; then
        guardian_pid="$(awk '/^orchestrator host guardian watching/ {print $NF; exit}' /case/evidence/driver.log)"
        [[ "$guardian_pid" =~ ^[1-9][0-9]*$ ]] || exit 2
        kill -0 "$guardian_pid" || exit 2
        kill -KILL "$guardian_pid" || exit 2
        printf '%s\n' "$guardian_pid" >/case/evidence/benchmark-guardian-killed.txt
    fi
    if [[ "$SOAK_DISK_TEST_SCENARIO" == benchmark-cancel-stall ]]; then
        guardian_pid="$(awk '/^orchestrator host guardian watching/ {print $NF; exit}' /case/evidence/driver.log)"
        [[ "$guardian_pid" =~ ^[1-9][0-9]*$ ]] || exit 2
        kill -STOP "$guardian_pid" || exit 2
        for _ in $(seq 1 20); do
            guardian_state="$(ps -o stat= -p "$guardian_pid" || true)"
            [[ "$guardian_state" != T* ]] || break
            sleep 0.05
        done
        [[ "$guardian_state" == T* ]] || exit 2
        printf '%s\n' "$guardian_pid" >/case/evidence/guardian-stopped.txt
    fi
    printf '%s\n' "$SOAK_DISK_TEST_SCENARIO" >/case/evidence/benchmark-fault-ready.txt
    while [[ ! -e /case/evidence/release-benchmark ]]; do sleep 0.05; done
    touch /case/evidence/benchmark-returned.txt
    exit 1
fi
if [[ "$*" == 'system df' ]]; then
    if [[ "${SOAK_DISK_TEST_SCENARIO:-band}" == hygiene-timeout ]]; then
        trap '' TERM
        printf '%s\n' "$$" >/case/evidence/hygiene-client-pid.txt
        touch /case/evidence/hygiene-fault-ready
        while [[ ! -e /case/evidence/release-hygiene ]]; do sleep 0.05; done
    fi
    touch /case/evidence/hygiene-completed
fi
if [[ "${SOAK_DISK_TEST_SCENARIO:-band}" == stop-timeout ]]; then
    case "$*" in
        'ps -q --filter name=rnode.' | 'ps -aq --filter name=rnode.') printf 'disk-fixture\n' ;;
        'kill disk-fixture' | 'rm -f disk-fixture')
            printf '%s\n' "$$" >>/case/evidence/stop-pids.txt
            touch /case/evidence/stop-started
            trap '' TERM
            while [[ ! -e /case/evidence/release-stop ]]; do sleep 0.05; done
            ;;
    esac
fi
if [[ "${SOAK_DISK_TEST_SCENARIO:-band}" == record-before-stop ]]; then
    if [[ "$*" == "ps -q --filter name=rnode." ]]; then
        printf 'disk-fixture\n'
    elif [[ "$*" == 'kill disk-fixture' ]]; then
        if [[ -s /case/evidence/output/host-guardian-breach.txt ]]; then
            cp /case/evidence/output/host-guardian-breach.txt /case/evidence/record-before-stop.txt
        else
            printf 'missing\n' >/case/evidence/record-before-stop.txt
        fi
        sleep 1
    fi
fi
exit 0
SH
    cat >bin/poetry <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >/case/evidence/workload-started.txt
case "${SOAK_DISK_TEST_SCENARIO:-band}" in
    guardian-death)
        guardian_pid="$(awk '/^orchestrator host guardian watching/ {print $NF; exit}' /case/evidence/driver.log)"
        [[ "$guardian_pid" =~ ^[1-9][0-9]*$ ]] || exit 2
        kill -0 "$guardian_pid" || exit 2
        kill -KILL "$guardian_pid" || exit 2
        printf '%s\n' "$guardian_pid" >/case/evidence/guardian-killed.txt
        sleep 12
        ;;
    guardian-stall)
        guardian_pid="$(awk '/^orchestrator host guardian watching/ {print $NF; exit}' /case/evidence/driver.log)"
        [[ "$guardian_pid" =~ ^[1-9][0-9]*$ ]] || exit 2
        kill -STOP "$guardian_pid" || exit 2
        printf '%s\n' "$guardian_pid" >/case/evidence/guardian-stopped.txt
        printf '%s\n' "$$" >/case/evidence/iteration-client-pid.txt
        touch /case/evidence/iteration-fault-ready
        while [[ ! -e /case/evidence/release-iteration ]]; do sleep 0.05; done
        ;;
    missing-active | record-before-stop | stalled-active | stop-timeout) sleep 12 ;;
esac
if [[ "${SOAK_DISK_TEST_SCENARIO:-band}" != benchmark-guardian-interleaved ]]; then
    printf 'finalize\n' >/case/evidence/output/signal
fi
printf 'The boundary workload fixture completed.\n'
SH
    if [[ "$SCENARIO" == restart-uncounted || "$SCENARIO" == restart-counted ]]; then
        mkdir -p evidence/output
        prior_failures=0
        [[ "$SCENARIO" != restart-counted ]] || prior_failures=2
        printf 'STARTED_AT=%s\nITERATIONS=1\nFAILURES=%s\nBENCH_SEGMENTS=0\nBENCH_FAILURES=0\nSEGMENT=1\n' \
            "$(date +%s)" "$prior_failures" >evidence/output/.soak-state
        printf 'A prior guardian detected a disk breach. Termination remains unconfirmed.\n' >evidence/output/host-guardian-breach.txt
        cp evidence/output/.soak-state evidence/restart-input-state.txt
        cp evidence/output/host-guardian-breach.txt evidence/restart-input-breach.txt
    fi
    duration=30
    disk_floor=4096
    disk_band=4096
    case "$SCENARIO" in
    disk-floor-range) disk_floor=9223372036854775808 ;;
    disk-band-range) disk_band=9223372036854775808 ;;
    disk-sum-range) disk_band=9223372036854771712 ;;
    disk-max-floor)
        disk_floor=9223372036854775807
        disk_band=0
        ;;
    disk-max-band)
        disk_floor=0
        disk_band=9223372036854775807
        ;;
    esac
    [[ "$SCENARIO" != benchmark-disabled ]] || disk_floor=0
    printf 'floor=%s\nband=%s\n' "$disk_floor" "$disk_band" >evidence/disk-settings.txt
    run_benchmarks=false
    if [[ "$SCENARIO" == restart-benchmark || "$SCENARIO" == benchmark-* ]]; then
        duration=700
        run_benchmarks=true
        mkdir -p evidence/output node/docker
        printf 'services: {}\n' >node/docker/shard.yml
        if [[ "$SCENARIO" == restart-benchmark ]]; then
            printf 'A prior guardian detected a disk breach. Termination remains unconfirmed.\n' >evidence/output/host-guardian-breach.txt
            cp evidence/output/host-guardian-breach.txt evidence/restart-input-breach.txt
        fi
    fi
    bench_every=4
    if [[ "$SCENARIO" == benchmark-guardian-interleaved ]]; then
        bench_every=1
        printf 'STARTED_AT=%s\nITERATIONS=0\nFAILURES=0\nBENCH_SEGMENTS=0\nBENCH_FAILURES=0\nSEGMENT=1\n' \
            "$(date +%s)" >evidence/output/.soak-state
    fi
    observer=""
    if [[ "$SCENARIO" == hygiene-timeout ]]; then
        (
            for _ in $(seq 1 200); do
                [[ ! -e evidence/hygiene-fault-ready ]] || break
                sleep 0.05
            done
            [[ -e evidence/hygiene-fault-ready ]] || exit 2
            sleep 5
            ps -eo pid,ppid,pgid,stat,args >evidence/hygiene-observed-processes.txt
            outcome=met
            if [[ -s evidence/output/summary.json ]]; then
                cp evidence/output/summary.json evidence/hygiene-observed-summary.json
            else
                outcome=exceeded
            fi
            pid="$(<evidence/hygiene-client-pid.txt)"
            client_state="$(ps -o stat= -p "$pid" || true)"
            [[ -z "$client_state" || "$client_state" == Z* ]] || outcome=exceeded
            printf '%s\n' "$outcome" >evidence/hygiene-cancellation.txt
            touch evidence/release-hygiene
        ) &
        observer=$!
    fi
    if [[ "$SCENARIO" == guardian-progress-boundary || "$SCENARIO" == benchmark-progress-boundary ]]; then
        (
            for _ in $(seq 1 200); do
                [[ ! -s evidence/admission-suspended.txt ]] || break
                sleep 0.05
            done
            [[ -s evidence/admission-suspended.txt ]] || exit 2
            read -r driver_pid guardian_pid <evidence/admission-suspended.txt
            sleep 9
            ps -o stat= -p "$driver_pid" >evidence/admission-driver-state.txt || exit 2
            ps -o stat= -p "$guardian_pid" >evidence/admission-guardian-state.txt || exit 2
            cp evidence/output/.host-guardian-progress evidence/admission-progress.txt || exit 2
            read -r uptime _ </proc/uptime
            printf '%s\n' "${uptime%%.*}" >evidence/admission-resumed-at.txt
            kill -CONT "$driver_pid" || exit 2
            sleep 3
            kill -CONT "$guardian_pid" || exit 2
        ) &
        observer=$!
    fi
    if [[ "$SCENARIO" == guardian-stall ]]; then
        (
            for _ in $(seq 1 200); do
                [[ ! -e evidence/iteration-fault-ready ]] || break
                sleep 0.05
            done
            [[ -e evidence/iteration-fault-ready ]] || exit 2
            sleep 12
            ps -eo pid,ppid,pgid,stat,args >evidence/iteration-observed-processes.txt
            outcome=met
            if [[ -s evidence/output/summary.json ]]; then
                cp evidence/output/summary.json evidence/iteration-observed-summary.json
            else
                outcome=exceeded
            fi
            pid="$(<evidence/iteration-client-pid.txt)"
            client_state="$(ps -o stat= -p "$pid" || true)"
            [[ -z "$client_state" || "$client_state" == Z* ]] || outcome=exceeded
            printf '%s\n' "$outcome" >evidence/iteration-cancellation.txt
            guardian_pid="$(<evidence/guardian-stopped.txt)"
            ps -o stat= -p "$guardian_pid" >evidence/guardian-observed-state.txt || exit 2
            kill -CONT "$guardian_pid" || exit 2
            touch evidence/release-iteration
        ) &
        observer=$!
    fi
    if [[ "$SCENARIO" == benchmark-cancel-* ]]; then
        (
            for _ in $(seq 1 200); do
                [[ ! -s evidence/benchmark-fault-ready.txt ]] || break
                sleep 0.05
            done
            [[ -s evidence/benchmark-fault-ready.txt ]] || exit 2
            if [[ "$SCENARIO" == benchmark-cancel-stall ]]; then sleep 12; else sleep 8; fi
            ps -eo pid,ppid,pgid,stat,args >evidence/benchmark-observed-processes.txt
            outcome=met
            if [[ -s evidence/output/summary.json ]]; then
                cp evidence/output/summary.json evidence/benchmark-observed-summary.json
            else
                outcome=exceeded
            fi
            pid="$(<evidence/benchmark-client-pid.txt)"
            client_state="$(ps -o stat= -p "$pid" || true)"
            [[ -z "$client_state" || "$client_state" == Z* ]] || outcome=exceeded
            printf '%s\n' "$outcome" >evidence/benchmark-cancellation.txt
            if [[ "$SCENARIO" == benchmark-cancel-stall ]]; then
                guardian_pid="$(<evidence/guardian-stopped.txt)"
                ps -o stat= -p "$guardian_pid" >evidence/guardian-observed-state.txt || exit 2
                kill -CONT "$guardian_pid" || exit 2
            fi
            touch evidence/release-benchmark
        ) &
        observer=$!
    fi
    if [[ "$SCENARIO" == stop-timeout ]]; then
        (
            for _ in $(seq 1 200); do
                [[ ! -e evidence/stop-started ]] || break
                sleep 0.05
            done
            [[ -e evidence/stop-started ]] || exit 2
            sleep 5
            ps -eo pid,ppid,stat,args >evidence/stop-processes.txt
            date -u '+%Y-%m-%dT%H:%M:%SZ' >evidence/stop-observed-at.txt
            outcome=met
            if [[ -s evidence/output/summary.json ]]; then
                cp evidence/output/summary.json evidence/stop-observed-summary.json
            else
                outcome=exceeded
            fi
            while IFS= read -r pid; do
                if kill -0 "$pid" 2>/dev/null; then outcome=exceeded; fi
            done <evidence/stop-pids.txt
            printf '%s\n' "$outcome" >evidence/stop-deadline.txt
            touch evidence/release-stop
        ) &
        observer=$!
    fi
    if [[ "$SCENARIO" == diagnostic-deadline ]]; then
        for n in $(seq 1 32); do mkdir -p "/tmp/test-diagnostic-$n"; done
        cat >bin/du <<'SH'
#!/usr/bin/env bash
touch /case/evidence/diagnostic-started
while [[ ! -e /case/evidence/release-diagnostic ]]; do sleep 0.05; done
exec /usr/bin/du "$@"
SH
        (
            for _ in $(seq 1 200); do
                [[ ! -e evidence/diagnostic-started ]] || break
                sleep 0.05
            done
            [[ -e evidence/diagnostic-started ]] || exit 2
            sleep 3
            if [[ -e evidence/output/disk-floor-breach.txt ]]; then
                printf 'met\n' >evidence/diagnostic-deadline.txt
            else
                printf 'exceeded\n' >evidence/diagnostic-deadline.txt
            fi
            touch evidence/release-diagnostic
        ) &
        observer=$!
    fi
    ownership_writer=""
    if [[ "$SCENARIO" == cleanup-active-session ]]; then
        mkdir /tmp/test-unowned-active
        printf 'active session data\n' >/tmp/test-unowned-active/data
        (
            exec 3>>/tmp/test-unowned-active/data
            touch /case/evidence/ownership-writer-ready
            while [[ ! -e /case/evidence/release-ownership-writer ]]; do
                printf 'writer active\n' >&3
                sleep 0.05
            done
        ) &
        ownership_writer=$!
        for _ in $(seq 1 100); do
            [[ ! -e evidence/ownership-writer-ready ]] || break
            sleep 0.05
        done
        [[ -e evidence/ownership-writer-ready ]] || exit 2
        kill -0 "$ownership_writer" || exit 2
        touch -d '2 hours ago' /tmp/test-unowned-active
        stat -c '%n %i %Y' /tmp/test-unowned-active /tmp/test-unowned-active/data >evidence/ownership-before.txt
        find /tmp -maxdepth 1 -name test-unowned-active -mmin +60 >evidence/ownership-aged-root.txt
        grep -Fxq /tmp/test-unowned-active evidence/ownership-aged-root.txt || exit 2
        printf '%s\n' "$ownership_writer" >evidence/ownership-writer-pid.txt
    fi
    chmod +x bin/*
    status=0
    PATH="/case/bin:$PATH" \
        SOAK_DURATION_SECONDS="$duration" \
        SYSTEM_INTEGRATION_DIR=/case/harness \
        SOAK_OUTPUT_DIR=/case/evidence/output \
        SOAK_TARGET_REF=disk-admission-fixture \
        SOAK_TARGET_SHA="${SOAK_DISK_TEST_SOURCE_SHA:-unknown}" \
        SOAK_RSS_CEILING_MB=0 \
        SOAK_HOST_FREE_FLOOR_MB=0 \
        SOAK_DISK_FREE_FLOOR_MB="$disk_floor" \
        SOAK_DISK_HYGIENE_BAND_MB="$disk_band" \
        SOAK_DISK_DIAGNOSTIC_SECONDS=1 \
        SOAK_DISK_STOP_SECONDS=1 \
        SOAK_DISK_HYGIENE_SECONDS=1 \
        SOAK_TMP_ROOT=/tmp \
        SOAK_RUNNER_ROOT=/case/runner \
        SOAK_RUN_BENCHMARKS="$run_benchmarks" \
        SOAK_BENCH_DURATION=1 \
        SOAK_BENCH_EVERY="$bench_every" \
        SOAK_NODE_REPO_DIR=/case/node \
        DEPLOYER_KEY=fixture-not-used \
        SOAK_MERGE_EXIT_MIN_SECONDS=0 \
        SOAK_GUARDIAN_MAX_SILENCE_SECONDS=8 \
        SOAK_GUARDIAN_POLL_SECONDS=0.05 \
        SOAK_MONITOR_SNAPSHOT_SECONDS=0.1 \
        timeout --signal=TERM --kill-after=2 20 \
        bash repo/scripts/run-merge-recovery-soak.sh >evidence/driver.log 2>&1 || status=$?
    printf '%s\n' "$status" >evidence/driver-exit.txt
    [[ -z "$observer" ]] || wait "$observer"
    if [[ "$SCENARIO" == disk-*-range ]]; then
        if [[ "$status" != 2 ]]; then
            if [[ ! -s evidence/workload-started.txt ]]; then
                printf 'ERROR: The range fixture neither rejected configuration nor reached workload admission.\n' >&2
                exit 2
            fi
            printf 'FAIL: An out-of-range disk setting permitted workload admission (%s).\n' "$SCENARIO" >&2
            exit 1
        fi
        if [[ -e evidence/workload-started.txt || -e evidence/benchmark-started.txt ]] ||
            ! grep -Eq '^SOAK_DISK_.*must' evidence/driver.log; then
            printf 'ERROR: The range fixture did not produce the expected configuration refusal.\n' >&2
            exit 2
        fi
        printf 'PASS: Invalid disk settings were rejected before workload admission (%s).\n' "$SCENARIO"
        exit 0
    fi
    if [[ "$status" != 0 && "$status" != 1 ]] ||
        [[ ! -s evidence/output/summary.json ]] ||
        ! jq -e 'has("degraded") | not' evidence/output/summary.json >/dev/null; then
        printf 'ERROR: The fixture did not complete the required driver path.\n' >&2
        exit 2
    fi
    iterations="$(find evidence/output -maxdepth 1 -type d -name 'iteration-*' | wc -l | tr -d ' ')"
    if [[ "$SCENARIO" == cleanup-partial || "$SCENARIO" == cleanup-sufficient ]]; then
        expected_iterations=0
        expected_failures=1
        expected_sample=8000
        if [[ "$SCENARIO" == cleanup-sufficient ]]; then
            expected_iterations=1
            expected_failures=0
            expected_sample=16384
        fi
        if ! grep -Fxq 'valid=7000' evidence/probe-samples.txt ||
            ! grep -Fxq "valid=$expected_sample" evidence/probe-samples.txt ||
            [[ ! -e evidence/hygiene-completed ]]; then
            printf 'ERROR: The cleanup fixture did not exercise its reclamation outcome.\n' >&2
            exit 2
        fi
        if [[ "$status" != "$expected_failures" || "$iterations" != "$expected_iterations" ]] ||
            ! jq -e --argjson iterations "$expected_iterations" --argjson failures "$expected_failures" \
                '.iterations == $iterations and .failures == $failures and .bench_segments == 0 and .bench_failures == 0' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: Successful cleanup changed the admission result for its disk sample (%s).\n' "$SCENARIO" >&2
            exit 1
        fi
        printf 'PASS: Successful cleanup preserved the admission result for its disk sample (%s).\n' "$SCENARIO"
        exit 0
    fi
    if [[ "$SCENARIO" == cleanup-error-* ]]; then
        if ! grep -Fxq "${SCENARIO#cleanup-error-}" evidence/cleanup-failed-command.txt ||
            ! grep -Fxq 'valid=7000' evidence/probe-samples.txt ||
            ! grep -Fxq 'valid=16384' evidence/probe-samples.txt; then
            printf 'ERROR: The cleanup fixture did not expose the command error and later sufficient sample.\n' >&2
            exit 2
        fi
        if [[ "$status" != 1 || "$iterations" != 0 || -e evidence/workload-started.txt ]] ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! jq -e '.iterations == 0 and .failures == 1 and .bench_segments == 0 and .bench_failures == 0' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: A failed cleanup command permitted work or lost its failure result (%s).\n' "$SCENARIO" >&2
            exit 1
        fi
        printf 'PASS: A failed cleanup command prevented admission despite a sufficient later disk sample (%s).\n' "$SCENARIO"
        exit 0
    fi
    if [[ "$SCENARIO" == cleanup-active-session ]]; then
        kill -0 "$ownership_writer" || exit 2
        ps -o pid=,stat= -p "$ownership_writer" >evidence/ownership-writer-state.txt
        readlink "/proc/$ownership_writer/fd/3" >evidence/ownership-open-file.txt
        touch evidence/release-ownership-writer
        wait "$ownership_writer"
        if ! grep -Fxq 'valid=7000' evidence/probe-samples.txt || [[ ! -e evidence/hygiene-completed ]]; then
            printf 'ERROR: The ownership fixture did not exercise disk hygiene.\n' >&2
            exit 2
        fi
        if [[ ! -d /tmp/test-unowned-active || ! -s /tmp/test-unowned-active/data ]]; then
            printf 'FAIL: Disk hygiene deleted an unowned session directory while its writer remained active.\n' >&2
            exit 1
        fi
        stat -c '%n %i %Y' /tmp/test-unowned-active /tmp/test-unowned-active/data >evidence/ownership-after.txt
        cp /tmp/test-unowned-active/data evidence/ownership-preserved-data.txt
        if [[ "$status" != 1 || "$iterations" != 0 || -e evidence/workload-started.txt ]] ||
            ! grep -Fxq 'active session data' evidence/ownership-preserved-data.txt ||
            ! jq -e '.iterations == 0 and .failures == 1 and .bench_segments == 0 and .bench_failures == 0' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: Preserved session data did not retain disk refusal and its failure result.\n' >&2
            exit 1
        fi
        printf 'PASS: Disk hygiene preserved the unowned active session and refused new work below the admission threshold.\n'
        exit 0
    fi
    if [[ "$SCENARIO" == disk-max-floor || "$SCENARIO" == disk-max-band ]]; then
        expected_iterations=0
        expected_failures=1
        if [[ "$SCENARIO" == disk-max-band ]]; then
            expected_iterations=1
            expected_failures=0
        fi
        if [[ "$status" != "$expected_failures" || "$iterations" != "$expected_iterations" ]] ||
            ! jq -e --argjson count "$expected_iterations" --argjson failures "$expected_failures" \
                '.iterations == $count and .failures == $failures and .bench_segments == 0' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: A valid maximum disk setting changed admission behavior (%s).\n' "$SCENARIO" >&2
            exit 1
        fi
        printf 'PASS: A valid maximum disk setting preserved admission behavior (%s).\n' "$SCENARIO"
        exit 0
    fi
    if [[ "$SCENARIO" == hygiene-timeout ]]; then
        if [[ ! -s evidence/hygiene-client-pid.txt ]] ||
            ! grep -Fxq 'system df' evidence/docker-commands.txt ||
            ! grep -Fxq 'valid=7000' evidence/probe-samples.txt; then
            printf 'ERROR: The fixture did not stall cleanup inside the hygiene band.\n' >&2
            exit 2
        fi
        if ! grep -Fxq met evidence/hygiene-cancellation.txt; then
            printf 'FAIL: Stalled disk hygiene prevented client cancellation and failure publication before fixture release.\n' >&2
            exit 1
        fi
        if [[ "$status" != 1 || "$iterations" != 0 || -e evidence/workload-started.txt ]] ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! jq -e '.iterations == 0 and .failures == 1 and .bench_segments == 0 and .bench_failures == 0' evidence/hygiene-observed-summary.json >/dev/null; then
            printf 'FAIL: Disk hygiene timeout lost its protection failure or admitted work.\n' >&2
            exit 1
        fi
        printf 'PASS: The driver canceled the stalled hygiene client and published failure before fixture release.\n'
        exit 0
    fi
    if [[ "$SCENARIO" == restart-uncounted || "$SCENARIO" == restart-counted ]]; then
        expected_failures=1
        [[ "$SCENARIO" != restart-counted ]] || expected_failures=2
        if [[ -e evidence/workload-started.txt || "$status" != 1 || "$iterations" != 0 ]] ||
            ! cmp -s evidence/restart-input-breach.txt evidence/output/host-guardian-breach.txt ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! jq -e --argjson expected "$expected_failures" '.iterations == 1 and .failures == $expected' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: Restart lost the prior guardian breach or admitted new work (%s).\n' "$SCENARIO" >&2
            exit 1
        fi
        printf 'PASS: Restart preserved the guardian breach and failure count without new work (%s).\n' "$SCENARIO"
        exit 0
    fi
    if [[ "$SCENARIO" == restart-benchmark ]]; then
        if ! grep -Fxq 'valid=16384' evidence/probe-samples.txt ||
            ! cmp -s evidence/restart-input-breach.txt evidence/output/host-guardian-breach.txt; then
            printf 'ERROR: The fixture lacks the retained breach or valid startup sample.\n' >&2
            exit 2
        fi
        if ! jq -e '.bench_segments == 0' evidence/output/summary.json >/dev/null &&
            [[ ! -s evidence/benchmark-started.txt ]]; then
            printf 'ERROR: The benchmark fixture did not reach the Docker boundary.\n' >&2
            exit 2
        fi
        if [[ -s evidence/benchmark-started.txt ]]; then
            printf 'FAIL: A retained guardian breach allowed the opening benchmark to start.\n' >&2
            exit 1
        fi
        if [[ "$status" != 1 || "$iterations" != 0 || -e evidence/workload-started.txt ]] ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! jq -e '.iterations == 0 and .failures == 1 and .bench_segments == 0 and .bench_failures == 0' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: Benchmark refusal lost the retained protection failure.\n' >&2
            exit 1
        fi
        printf 'PASS: The retained breach prevented benchmark and iteration admission and preserved the failure.\n'
        exit 0
    fi
    if [[ "$SCENARIO" == benchmark-guardian-boundary || "$SCENARIO" == benchmark-guardian-interleaved ]]; then
        expected_iterations=0
        [[ "$SCENARIO" != benchmark-guardian-interleaved ]] || expected_iterations=1
        if [[ ! -s evidence/benchmark-boundary-killed.txt ]] ||
            ! grep -Fxq 'valid=16384' evidence/probe-samples.txt ||
            [[ "$iterations" != "$expected_iterations" ]]; then
            printf 'ERROR: The fixture did not exercise its benchmark admission boundary.\n' >&2
            exit 2
        fi
        if [[ -e evidence/benchmark-started.txt ]] ||
            ! jq -e '.bench_segments == 0' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: Guardian death during the benchmark probe allowed benchmark admission (%s).\n' "$SCENARIO" >&2
            exit 1
        fi
        if [[ "$status" != 1 ]] ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! jq -e --argjson count "$expected_iterations" '.iterations == $count and .failures == 1 and .bench_segments == 0 and .bench_failures == 0' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: Benchmark guardian refusal lost its protection failure.\n' >&2
            exit 1
        fi
        printf 'PASS: Guardian death during the probe prevented benchmark admission and preserved failure (%s).\n' "$SCENARIO"
        exit 0
    fi
    if [[ "$SCENARIO" == guardian-progress-boundary || "$SCENARIO" == benchmark-progress-boundary ]]; then
        if ! grep -Eq '^T' evidence/admission-driver-state.txt ||
            ! grep -Eq '^T' evidence/admission-guardian-state.txt ||
            ! grep -Fxq 'valid=16384' evidence/probe-samples.txt; then
            printf 'ERROR: The fixture did not suspend the driver and guardian at admission.\n' >&2
            exit 2
        fi
        last="$(<evidence/admission-progress.txt)"
        now="$(<evidence/admission-resumed-at.txt)"
        if [[ ! "$last" =~ ^[0-9]+$ || ! "$now" =~ ^[0-9]+$ ]] || ((now - last <= 8)); then
            printf 'ERROR: The fixture did not expire guardian progress before admission.\n' >&2
            exit 2
        fi
        if ! jq -e '.iterations == 0 and .bench_segments == 0' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: Stale guardian progress permitted new work at admission (%s).\n' "$SCENARIO" >&2
            exit 1
        fi
        if [[ "$status" != 1 || "$iterations" != 0 || -e evidence/workload-started.txt || -e evidence/benchmark-started.txt ]] ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! jq -e '.failures == 1 and .bench_failures == 0' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: Stale guardian admission refusal lost its protection failure.\n' >&2
            exit 1
        fi
        printf 'PASS: Stale guardian progress prevented admission and preserved failure (%s).\n' "$SCENARIO"
        exit 0
    fi
    if [[ "$SCENARIO" == guardian-stall ]]; then
        if [[ ! -s evidence/guardian-stopped.txt || ! -s evidence/iteration-client-pid.txt ]] ||
            ! grep -Fxq 'valid=16384' evidence/probe-samples.txt ||
            ! grep -Eq '^T' evidence/guardian-observed-state.txt; then
            printf 'ERROR: The fixture did not suspend a live guardian during the active iteration.\n' >&2
            exit 2
        fi
        if ! grep -Fxq met evidence/iteration-cancellation.txt; then
            printf 'FAIL: A live guardian without progress did not stop its active iteration before fixture release.\n' >&2
            exit 1
        fi
        if [[ "$status" != 1 || "$iterations" != 1 ]] ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! jq -e '.iterations == 1 and .failures == 1 and .bench_segments == 0 and .bench_failures == 0' evidence/iteration-observed-summary.json >/dev/null; then
            printf 'FAIL: Guardian progress refusal lost its protection failure or admitted another iteration.\n' >&2
            exit 1
        fi
        printf 'PASS: The missing guardian progress stopped the active iteration and preserved failure before fixture release.\n'
        exit 0
    fi
    if [[ "$SCENARIO" == benchmark-cancel-* ]]; then
        if [[ ! -s evidence/benchmark-client-pid.txt ]] ||
            ! grep -Fxq "$SCENARIO" evidence/benchmark-fault-ready.txt ||
            ! grep -Fxq 'valid=16384' evidence/probe-samples.txt ||
            [[ "$SCENARIO" == benchmark-cancel-death && ! -s evidence/benchmark-guardian-killed.txt ]]; then
            printf 'ERROR: The fixture did not exercise the stalled benchmark guardian fault.\n' >&2
            exit 2
        fi
        if [[ "$SCENARIO" == benchmark-cancel-breach ]] &&
            ! grep -Fxq 'valid=1024' evidence/probe-samples.txt; then
            printf 'ERROR: The guardian did not sample the benchmark disk fault.\n' >&2
            exit 2
        fi
        if [[ "$SCENARIO" == benchmark-cancel-stall ]] &&
            { [[ ! -s evidence/guardian-stopped.txt ]] || ! grep -Eq '^T' evidence/guardian-observed-state.txt; }; then
            printf 'ERROR: The fixture did not retain a live suspended guardian until observation.\n' >&2
            exit 2
        fi
        if ! grep -Fxq met evidence/benchmark-cancellation.txt; then
            printf 'FAIL: A stalled benchmark prevented failure publication and client cancellation before fixture release (%s).\n' "$SCENARIO" >&2
            exit 1
        fi
        if [[ "$status" != 1 || "$iterations" != 0 || -e evidence/workload-started.txt ]] ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! jq -e '.iterations == 0 and .failures == 1 and .bench_segments == 1 and .bench_failures == 1' evidence/benchmark-observed-summary.json >/dev/null; then
            printf 'FAIL: Benchmark cancellation lost the protection failure or admitted later work.\n' >&2
            exit 1
        fi
        printf 'PASS: The guardian fault canceled the stalled benchmark client and preserved failure before fixture release (%s).\n' "$SCENARIO"
        exit 0
    fi
    if [[ "$SCENARIO" == benchmark-missing ]]; then
        if ! grep -Fxq 'valid=16384' evidence/probe-samples.txt ||
            ! grep -Fxq 'missing' evidence/probe-samples.txt; then
            printf 'ERROR: The fixture did not exercise an unavailable benchmark sample.\n' >&2
            exit 2
        fi
        if [[ "$status" != 1 || "$iterations" != 0 || -e evidence/workload-started.txt || -e evidence/benchmark-started.txt ]] ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! grep -Fq 'benchmark disk sample unavailable MiB' evidence/output/early-exit.txt ||
            ! jq -e '.iterations == 0 and .failures == 1 and .bench_segments == 0 and .bench_failures == 0' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: An unavailable benchmark sample did not refuse all later work and record one failure.\n' >&2
            exit 1
        fi
        printf 'PASS: An unavailable opening benchmark sample refused work and recorded one protection failure.\n'
        exit 0
    fi
    if [[ "$SCENARIO" == benchmark-equal || "$SCENARIO" == benchmark-sufficient || "$SCENARIO" == benchmark-disabled ]]; then
        expected_sample=8192
        [[ "$SCENARIO" != benchmark-sufficient ]] || expected_sample=16384
        if [[ "$SCENARIO" != benchmark-disabled ]]; then
            if ! grep -Fxq "valid=$expected_sample" evidence/probe-samples.txt; then
                printf 'ERROR: The fixture lacks its expected benchmark sample.\n' >&2
                exit 2
            fi
        elif ! grep -Fq 'disk free floor=0MB' evidence/driver.log ||
            grep -q '^orchestrator host guardian watching' evidence/driver.log; then
            printf 'ERROR: The fixture did not disable both guardian protection floors.\n' >&2
            exit 2
        fi
        if [[ "$status" != 0 || "$iterations" != 1 || ! -s evidence/benchmark-started.txt || ! -s evidence/workload-started.txt ||
            -e evidence/output/host-guardian-breach.txt ]] ||
            ! jq -e '.iterations == 1 and .failures == 0 and .bench_segments == 1 and .bench_failures == 1' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: An allowed opening benchmark admission changed (%s).\n' "$SCENARIO" >&2
            exit 1
        fi
        printf 'PASS: Opening benchmark admission preserved the allowed path (%s).\n' "$SCENARIO"
        exit 0
    fi
    if [[ "$SCENARIO" == benchmark-band ]]; then
        if ! grep -Fxq 'valid=7000' evidence/probe-samples.txt ||
            [[ -e evidence/output/host-guardian-breach.txt ]]; then
            printf 'ERROR: The fixture lacks the low disk sample or has an unexpected guardian breach.\n' >&2
            exit 2
        fi
        if ! jq -e '.bench_segments == 0' evidence/output/summary.json >/dev/null &&
            [[ ! -s evidence/benchmark-started.txt ]]; then
            printf 'ERROR: The benchmark fixture did not reach the Docker boundary.\n' >&2
            exit 2
        fi
        if [[ -s evidence/benchmark-started.txt ]]; then
            printf 'FAIL: The opening benchmark started with 7000 MiB below the 8192 MiB admission threshold.\n' >&2
            exit 1
        fi
        if [[ "$status" != 1 || "$iterations" != 0 || -e evidence/workload-started.txt ]] ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            [[ ! -s evidence/output/protection-breach.txt || ! -s evidence/output/early-exit.txt ]] ||
            ! jq -e '.iterations == 0 and .failures == 1 and .bench_segments == 0 and .bench_failures == 0' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: Benchmark disk refusal lost the protection failure or admitted later work.\n' >&2
            exit 1
        fi
        printf 'PASS: The 7000 MiB sample prevented benchmark and iteration admission and recorded one protection failure.\n'
        exit 0
    fi
    if [[ "$SCENARIO" == benchmark-active-disk ]]; then
        if [[ ! -s evidence/benchmark-started.txt || ! -e evidence/benchmark-returned.txt ]] ||
            ! grep -Fxq 'available=1024' evidence/benchmark-disk-fault.txt ||
            ! grep -Fxq 'valid=16384' evidence/probe-samples.txt; then
            printf 'ERROR: The fixture did not exercise the active benchmark disk fault.\n' >&2
            exit 2
        fi
        if ! grep -Fxq 'recorded' evidence/benchmark-observation.txt; then
            printf 'FAIL: The opening benchmark returned without a guardian record and stop request for its disk fault.\n' >&2
            exit 1
        fi
        if [[ "$status" != 1 || "$iterations" != 0 || -e evidence/workload-started.txt ]] ||
            ! grep -Fxq 'valid=1024' evidence/probe-samples.txt ||
            ! grep -Fq '1024 MiB below floor 4096 MiB' evidence/stop-during-benchmark.txt ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! jq -e '.iterations == 0 and .failures == 1 and .bench_segments == 1 and .bench_failures == 1' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: Active benchmark protection lost the failure or admitted later work.\n' >&2
            exit 1
        fi
        printf 'PASS: The guardian recorded the benchmark disk breach and requested a stop before benchmark return.\n'
        exit 0
    fi
    if [[ "$SCENARIO" == stop-timeout ]]; then
        if [[ ! -s evidence/stop-deadline.txt || ! -s evidence/stop-pids.txt ]] ||
            ! grep -Fxq 'valid=1024' evidence/probe-samples.txt; then
            printf 'ERROR: The fixture did not exercise stalled disk stop commands.\n' >&2
            exit 2
        fi
        if ! grep -Fxq met evidence/stop-deadline.txt; then
            printf 'FAIL: Stalled stop commands prevented failure publication within the fixture deadline.\n' >&2
            exit 1
        fi
        if [[ "$status" != 1 || "$iterations" != 1 ]] ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! jq -e '.iterations == 1 and .failures == 1' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: Bounded stop commands lost the protection failure.\n' >&2
            exit 1
        fi
        printf 'PASS: The driver bounded stalled stop commands and published the protection failure. Writer termination remains unconfirmed.\n'
        exit 0
    fi
    if [[ "$SCENARIO" == diagnostic-deadline ]]; then
        if [[ ! -s evidence/diagnostic-deadline.txt ]] ||
            ! grep -Fxq 'disk hygiene: 7000MB free -> 7000MB free' evidence/driver.log; then
            printf 'ERROR: The fixture did not exercise stalled attribution after hygiene.\n' >&2
            exit 2
        fi
        if ! grep -Fxq 'met' evidence/diagnostic-deadline.txt; then
            printf 'FAIL: Disk attribution exceeded the aggregate fixture deadline.\n' >&2
            exit 1
        fi
        if [[ "$status" != 1 || "$iterations" != 0 ]] ||
            ! jq -e '.iterations == 0 and .failures == 1' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: Bounded attribution lost the admission refusal.\n' >&2
            exit 1
        fi
        printf 'PASS: Stalled attribution stopped within the aggregate fixture deadline across 32 session roots.\n'
        exit 0
    fi
    if [[ "$SCENARIO" == guardian-death-boundary ]]; then
        if [[ ! -s evidence/guardian-killed-at-boundary.txt ]] ||
            ! grep -q '^boundary-guardian-killed=' evidence/probe-samples.txt; then
            printf 'ERROR: The fixture did not kill the guardian during the boundary probe.\n' >&2
            exit 2
        fi
        if [[ "$iterations" != 0 || -e evidence/workload-started.txt ]]; then
            printf 'FAIL: The driver admitted an iteration after guardian death during the boundary probe.\n' >&2
            exit 1
        fi
        if [[ "$status" != 1 || ! -s evidence/output/host-guardian-breach.txt ]] ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! jq -e '.iterations == 0 and .failures == 1' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: Guardian death before admission lacks a recorded protection failure.\n' >&2
            exit 1
        fi
        printf 'PASS: Guardian death during the boundary probe prevented admission and produced a protection failure.\n'
        exit 0
    fi
    if [[ "$SCENARIO" == guardian-death ]]; then
        if [[ ! -s evidence/guardian-killed.txt || ! -s evidence/workload-started.txt ]]; then
            printf 'ERROR: The fixture did not kill the active guardian.\n' >&2
            exit 2
        fi
        if [[ "$status" != 1 || "$iterations" != 1 || ! -s evidence/output/host-guardian-breach.txt ]] ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! jq -e '.iterations == 1 and .failures >= 1' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: The driver completed without a failure after its guardian died.\n' >&2
            exit 1
        fi
        printf 'PASS: The driver recorded guardian death and stopped the active iteration.\n'
        exit 0
    fi
    if [[ "$SCENARIO" == record-before-stop ]]; then
        if [[ ! -s evidence/workload-started.txt || ! -s evidence/record-before-stop.txt ]] ||
            ! grep -Fxq 'valid=1024' evidence/probe-samples.txt; then
            printf 'ERROR: The fixture did not reach the disk stop command.\n' >&2
            exit 2
        fi
        if grep -Fxq 'missing' evidence/record-before-stop.txt; then
            printf 'FAIL: The Docker stop command started before the disk breach record existed.\n' >&2
            exit 1
        fi
        if ! grep -Fq 'termination is unconfirmed' evidence/record-before-stop.txt ||
            [[ "$status" != 1 || "$iterations" != 1 ]] ||
            ! jq -e '.iterations == 1 and .failures >= 1' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: The breach record or failure result is incomplete.\n' >&2
            exit 1
        fi
        printf 'PASS: The disk breach record exists before Docker starts the stop command. Termination remains unconfirmed.\n'
        exit 0
    fi
    if [[ "$SCENARIO" == missing-active || "$SCENARIO" == stalled-active ]]; then
        if [[ ! -s evidence/workload-started.txt ]] ||
            ! grep -Fxq 'valid=16384' evidence/probe-samples.txt ||
            ! grep -Fxq "$SCENARIO" evidence/probe-samples.txt; then
            printf 'ERROR: The fixture did not exercise the requested active probe fault.\n' >&2
            exit 2
        fi
        if [[ "$status" != 1 || "$iterations" != 1 ]] ||
            [[ ! -s evidence/output/host-guardian-breach.txt ]] ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! jq -e '.iterations == 1 and .failures >= 1' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: The driver did not stop the active iteration after the disk probe fault (%s).\n' "$SCENARIO" >&2
            exit 1
        fi
        printf 'PASS: The driver stopped the active iteration after the disk probe fault (%s).\n' "$SCENARIO"
        exit 0
    fi
    if [[ "$SCENARIO" != band ]]; then
        sample_kind=missing
        sample_record=missing
        if [[ "$SCENARIO" == malformed-boundary ]]; then
            sample_kind=malformed
            sample_record='malformed=16384junk'
        fi
        if ! grep -Eq '^valid=(7000|16384)$' evidence/probe-samples.txt ||
            ! grep -Fxq "$sample_record" evidence/probe-samples.txt ||
            [[ "$SCENARIO" == missing-after-hygiene && ! -f evidence/hygiene-completed ]]; then
            printf 'ERROR: The fixture did not exercise a %s post-start disk sample.\n' "$sample_kind" >&2
            exit 2
        fi
        if [[ "$iterations" != 0 || -e evidence/workload-started.txt ]]; then
            printf 'FAIL: A post-start disk sample was %s (%s), but the driver admitted %s iteration(s).\n' "$sample_kind" "$SCENARIO" "$iterations" >&2
            exit 1
        fi
        if [[ "$status" != 1 ]] ||
            ! grep -Fxq 'early_exit_reason=host_protection_breach' evidence/output/summary.txt ||
            ! grep -Fxq 'host_protection_breach: disk probe unavailable before admission' evidence/output/early-exit.txt ||
            ! grep -Fxq 'The disk probe is unavailable before admission. The driver refused work.' evidence/output/protection-breach.txt ||
            ! jq -e '.iterations == 0 and .failures == 1' evidence/output/summary.json >/dev/null; then
            printf 'FAIL: Invalid-sample refusal lacks the required failure result and evidence.\n' >&2
            exit 1
        fi
        printf 'PASS: A post-start disk sample was %s (%s). No iteration started, and the driver recorded refusal.\n' "$sample_kind" "$SCENARIO"
        exit 0
    fi
    if ! grep -Fxq 'disk hygiene: 7000MB free -> 7000MB free' evidence/driver.log; then
        printf 'ERROR: The fixture did not exercise zero-reclamation hygiene.\n' >&2
        exit 2
    fi
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
    printf 'Usage: %s [--scenario NAME] [source-directory] [evidence-directory]\n' "$0" >&2
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
# The unprivileged container user must be able to read the copied sources.
# GNU tar sets the modes on the way in; BSD tar (macOS) has no --mode, so the
# sources' own modes are used there.
TAR=tar
command -v gtar >/dev/null && TAR=gtar
TAR_MODE=()
"$TAR" --version 2>/dev/null | grep -q GNU && TAR_MODE=(--mode='a+rX')
CONTAINER=""
cleanup() {
    if [[ "$CONTAINER" =~ ^[0-9a-f]{64}$ ]]; then
        docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT

IMAGE="${SOAK_DISK_TEST_IMAGE:-}"
if [[ -z "$IMAGE" ]]; then
    if ! timeout --signal=TERM --kill-after=5 180 docker build \
        --iidfile "$OUTPUT/image-id.txt" - <"$ROOT/scripts/bench/soak-disk-test.Dockerfile" \
        >"$OUTPUT/image-build.log" 2>&1; then
        printf 'ERROR: The fixture image did not build; see %s\n' "$OUTPUT/image-build.log" >&2
        exit 2
    fi
    IMAGE="$(<"$OUTPUT/image-id.txt")"
fi
[[ "$IMAGE" =~ ^sha256:[0-9a-f]{64}$ ]] || exit 2
docker image inspect "$IMAGE" >"$OUTPUT/image-inspect.json"
jq -e '.[0].Config.Volumes == null or (.[0].Config.Volumes | length) == 0' \
    "$OUTPUT/image-inspect.json" >/dev/null

# Runs one scenario in a fresh container; evidence lands in $2. Returns the
# container's verdict: 0 pass, 1 behavioral failure, 2 fixture error.
run_scenario() {
    local scenario="$1" out="$2" status=0 container_status
    mkdir -p "$out"
    CONTAINER="$(docker create --pull=never --network none --cap-drop ALL \
        --security-opt no-new-privileges --pids-limit 128 --memory 256m --cpus 1 \
        --user 65534:65534 --workdir /case \
        --env "SOAK_DISK_TEST_SOURCE_SHA=${SOAK_DISK_TEST_SOURCE_SHA:-unknown}" \
        --env "SOAK_DISK_TEST_SCENARIO=$scenario" \
        --entrypoint bash "$IMAGE" /case/test.sh --inside)"
    [[ "$CONTAINER" =~ ^[0-9a-f]{64}$ ]] || return 2
    docker inspect "$CONTAINER" >"$out/container-inspect.json"
    jq -e '.[0] | (.Mounts | length) == 0 and .HostConfig.NetworkMode == "none"
        and .HostConfig.Privileged == false and .HostConfig.PidMode == ""
        and .Config.User == "65534:65534" and .HostConfig.CapDrop == ["ALL"]
        and (.HostConfig.SecurityOpt | index("no-new-privileges") != null)' \
        "$out/container-inspect.json" >/dev/null || return 2
    "$TAR" -C "$SOURCE" "${TAR_MODE[@]}" -cf - "${SOURCE_FILES[@]}" |
        docker cp - "$CONTAINER:/case/repo/"
    docker cp "${BASH_SOURCE[0]}" "$CONTAINER:/case/test.sh"
    timeout --signal=TERM --kill-after=5 40 docker start -a "$CONTAINER" \
        >"$out/result.txt" 2>&1 || status=$?
    docker inspect "$CONTAINER" >"$out/container-finished.json"
    if ! jq -e '.[0].State.Running == false and .[0].State.OOMKilled == false' \
        "$out/container-finished.json" >/dev/null; then
        printf 'ERROR: The fixture exceeded its container bounds (%s).\n' "$scenario" >&2
        return 2
    fi
    container_status="$(jq -r '.[0].State.ExitCode' "$out/container-finished.json")"
    if [[ "$status" != "$container_status" ]]; then
        printf 'ERROR: Docker did not return the container verdict (%s).\n' "$scenario" >&2
        return 2
    fi
    docker cp "$CONTAINER:/case/evidence" "$out/evidence"
    docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
    CONTAINER=""
    printf '%s\n' "$status" >"$out/test-exit.txt"
    cat "$out/result.txt"
    return "$status"
}

if [[ -n "$SCENARIO" && "$SCENARIO" != all ]]; then
    status=0
    run_scenario "$SCENARIO" "$OUTPUT" || status=$?
    printf 'Evidence: %s\n' "$OUTPUT"
    exit "$status"
fi

result=0
for scenario in "${SCENARIOS[@]}"; do
    status=0
    run_scenario "$scenario" "$OUTPUT/$scenario" || status=$?
    case "$status" in
    0) ;;
    1) result=1 ;;
    *)
        printf 'ERROR: Scenario %s failed without a behavioral verdict (exit %s).\n' "$scenario" "$status" >&2
        printf 'Evidence: %s\n' "$OUTPUT"
        exit 2
        ;;
    esac
done
printf 'Evidence: %s\n' "$OUTPUT"
if ((result == 0)); then
    printf 'PASS: All %s isolated disk scenarios passed.\n' "${#SCENARIOS[@]}"
fi
exit "$result"

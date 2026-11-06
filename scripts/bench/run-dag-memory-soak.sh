#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
apply=0
duration="${DAG_SOAK_SECONDS:-86400}"
interval="${DAG_SOAK_INTERVAL:-60}"
load_seconds="${DAG_SOAK_LOAD_SECONDS:-300}"
catchup_timeout="${DAG_SOAK_CATCHUP_TIMEOUT:-7200}"
fall_behind_timeout="${DAG_SOAK_FALL_BEHIND_TIMEOUT:-auto}"
live_memory_mib="${DAG_SOAK_LIVE_MEMORY_MIB:-2048}"
minimum_load_samples="${DAG_SOAK_MIN_LOAD_SAMPLES:-100}"
window="${DAG_SOAK_WINDOW:-1024}"
project="dag-memory-$(date -u +%Y%m%d%H%M%S)-$$"
out="${DAG_SOAK_OUTPUT:-$root/target/dag-memory-soaks/$project}"
image="${DAG_SOAK_IMAGE:-}"
usage() {
    cat <<'USAGE'
Usage: scripts/bench/run-dag-memory-soak.sh [--apply] [--out-dir NEW_DIRECTORY]

Default is a read-only plan. --apply starts one continuous local shard from the
existing docker/shard.yml: three validators, bootstrap and readonly follower.
The same containers and volumes survive the idle, deploy-load and catch-up phases.
At exit containers stop; volumes and evidence remain for inspection.

Required for --apply:
  DAG_SOAK_IMAGE       Locally available sha256:... or repository@sha256:... image
  DEPLOYER_KEY         Funded development deployer key, passed to existing load tool
Optional:
  DAG_SOAK_SECONDS=86400        Idle duration; shorter runs are smoke evidence only
  DAG_SOAK_INTERVAL=60          Sampling cadence in seconds
  DAG_SOAK_LOAD_SECONDS=300     Subsequent deploy load duration
  DAG_SOAK_LOAD_RATE=2          Deploys per second
  DAG_SOAK_CATCHUP_TIMEOUT=7200 Follower recovery timeout after unpause
  DAG_SOAK_FALL_BEHIND_TIMEOUT=auto  Pause budget: at least 7200s, otherwise
                              twice the observed time to advance W+33 heights
  DAG_SOAK_OUTPUT=...           New evidence directory; default under target/
  DAG_SOAK_ENV_FILE=...         Compose env file; defaults to docker/.env.example
  DAG_SOAK_MIN_MEMORY_MIB=8192   Required host MemAvailable before starting;
                              overriding requires measured node-capacity evidence
  DAG_SOAK_MIN_DISK_GIB=20      Required available space on Docker data filesystem
  DAG_SOAK_LIVE_MEMORY_MIB=2048 Stop this shard if host MemAvailable falls below
                              this reserve; this is not a per-node memory cap
  DAG_SOAK_MIN_LOAD_SAMPLES=100 Require at least this many submitted and finalized
                              deploys, zero submit errors, and all submissions finalized

Requires Linux Docker with cgroup memory.stat, Compose v2.24+, curl, jq and awk.
Host ports 14400..14455 must be available. No existing deployment is removed.
Evidence is not an acceptance certificate: allocator, migration, latency and
plateau analysis remain separate gates. No 24-hour claim is made for shorter runs.
USAGE
}
while (($#)); do
    case "$1" in
        --apply) apply=1; shift ;;
        --out-dir) out="$2"; shift 2 ;;
        --help|-h) usage; exit 0 ;;
        *) printf 'Unknown argument: %s\n' "$1" >&2; exit 2 ;;
    esac
done
for value in "$duration" "$interval" "$load_seconds" "$catchup_timeout" "$window" "$live_memory_mib" "$minimum_load_samples"; do
    [[ "$value" =~ ^[1-9][0-9]*$ ]] || { printf 'Durations/window must be positive integers.\n' >&2; exit 2; }
done
[[ "$fall_behind_timeout" == auto || "$fall_behind_timeout" =~ ^[1-9][0-9]*$ ]] || { printf 'Invalid fall-behind timeout.\n' >&2; exit 2; }
[[ "$window" == 1024 ]] || { printf 'This runner uses the existing shard configuration with W=1024.\n' >&2; exit 2; }
printf 'Plan: pinned image %s, idle %ss at %ss cadence, load %ss, catch-up beyond %s heights; evidence %s\n' "${image:-REQUIRED}" "$duration" "$interval" "$load_seconds" "$window" "$out"
printf 'Budgets: fall-behind=%s seconds (auto uses measured cadence), recovery=%s seconds; live host memory reserve=%s MiB, minimum finalized load=%s.\n' "$fall_behind_timeout" "$catchup_timeout" "$live_memory_mib" "$minimum_load_samples"
((apply)) || exit 0
[[ "$image" =~ (^sha256:[a-f0-9]{64}$|@sha256:[a-f0-9]{64}$) ]] || { printf 'DAG_SOAK_IMAGE must be an immutable image digest.\n' >&2; exit 2; }
: "${DEPLOYER_KEY:?Set the funded development DEPLOYER_KEY}"
for tool in docker curl jq awk timeout; do command -v "$tool" >/dev/null; done
docker image inspect "$image" >/dev/null
available_kib="$(awk '$1=="MemAvailable:" {print $2}' /proc/meminfo)"
docker_root="$(docker info --format '{{.DockerRootDir}}')"
disk_available_kib="$(df -Pk "$docker_root" | awk 'NR==2 {print $4}')"
out_ancestor="$out"
while [[ ! -e "$out_ancestor" ]]; do out_ancestor="$(dirname "$out_ancestor")"; done
evidence_available_kib="$(df -Pk "$out_ancestor" | awk 'NR==2 {print $4}')"
minimum_memory="${DAG_SOAK_MIN_MEMORY_MIB:-8192}"
minimum_disk="${DAG_SOAK_MIN_DISK_GIB:-20}"
[[ "$minimum_memory" =~ ^[1-9][0-9]*$ && "$minimum_disk" =~ ^[1-9][0-9]*$ ]]
printf 'Capacity: MemAvailable=%s KiB, Docker filesystem available=%s KiB, evidence filesystem available=%s KiB; existing containers remain untouched.\n' "$available_kib" "$disk_available_kib" "$evidence_available_kib"
((available_kib >= minimum_memory * 1024 && disk_available_kib >= minimum_disk * 1024 * 1024 && evidence_available_kib >= 10 * 1024 * 1024)) || { printf 'Insufficient host headroom for the configured capacity floor (evidence requires 10 GiB).\n' >&2; exit 2; }
[[ ! -e "$out" ]] || { printf 'Output directory already exists: %s\n' "$out" >&2; exit 2; }
mkdir -p "$out/raw"
out="$(cd "$out" && pwd)"
export F1R3FLY_RUST_IMAGE="$image"
export BOOTSTRAP_HOST="$project-bootstrap" VALIDATOR1_HOST="$project-validator1"
export VALIDATOR2_HOST="$project-validator2" VALIDATOR3_HOST="$project-validator3" READONLY_HOST="$project-readonly"
export COMPOSE_PROFILES=""
names=("$BOOTSTRAP_HOST" "$VALIDATOR1_HOST" "$VALIDATOR2_HOST" "$VALIDATOR3_HOST" "$READONLY_HOST")
ports=(14405 14415 14425 14435 14455)
services=(boot validator1 validator2 validator3 readonly)
for name in "${names[@]}"; do
    if docker container inspect "$name" >/dev/null 2>&1; then
        printf 'Refusing to reuse existing container: %s\n' "$name" >&2
        exit 2
    fi
done
cat >"$out/override.yml" <<YAML
networks:
  f1r3fly:
    name: $project
services:
  boot:
    restart: "no"
  validator1:
    restart: "no"
  validator2:
    restart: "no"
  validator3:
    restart: "no"
  readonly:
    restart: "no"
YAML
compose=(docker compose --project-name "$project" --env-file "${DAG_SOAK_ENV_FILE:-$root/docker/.env.example}" -f "$root/docker/shard.yml" -f "$root/docker/ci-ports.shard.yml" -f "$out/override.yml")
"${compose[@]}" config --format json | jq '{services: (.services | with_entries(.value |= {image,container_name,ports})),networks,volumes}' >"$out/topology.json"
git -C "$root" rev-parse HEAD >"$out/revision.txt"
git -C "$root" diff --stat >"$out/worktree-diff-stat.txt"
docker image inspect --format '{{.Id}}' "$image" >"$out/image-id.txt"
docker version --format '{{json .}}' >"$out/docker-version.json"
docker ps --format '{{json .}}' >"$out/preexisting-containers.jsonl"
jq -n --argjson memory_kib "$available_kib" --argjson disk_kib "$disk_available_kib" --argjson evidence_kib "$evidence_available_kib" '{available_memory_kib:$memory_kib,available_docker_disk_kib:$disk_kib,available_evidence_disk_kib:$evidence_kib}' >"$out/capacity.json"
host_oom_start="$(awk '$1=="oom_kill" {print $2}' /proc/vmstat)"
[[ "$host_oom_start" =~ ^[0-9]+$ ]]
printf 'timestamp,phase,available_memory_kib,swap_free_kib,docker_available_kib,evidence_available_kib,host_oom_kills\n' >"$out/host-capacity.csv"
jq -n --arg project "$project" --arg image "$image" --argjson seconds "$duration" --argjson interval "$interval" --argjson startup_memory "$minimum_memory" --argjson live_memory "$live_memory_mib" --argjson disk "$minimum_disk" --argjson load_samples "$minimum_load_samples" '{project:$project,image:$image,idle_seconds:$seconds,interval_seconds:$interval,validators:3,window:1024,allocator_live_heap_available:false,startup_memory_floor_mib:$startup_memory,live_memory_floor_mib:$live_memory,docker_disk_floor_gib:$disk,evidence_disk_floor_gib:10,minimum_finalized_load_samples:$load_samples}' >"$out/run.json"
started=0
paused=0
load_pid=""
cleanup() {
    local status=$? name running
    trap - EXIT
    if [[ -n "$load_pid" ]] && kill -0 "$load_pid" 2>/dev/null; then
        kill "$load_pid" 2>/dev/null || true
        wait "$load_pid" 2>/dev/null || true
    fi
    if ((started)); then
        if ((paused)); then docker unpause "$READONLY_HOST" >/dev/null || true; fi
        if ! timeout 90 "${compose[@]}" stop --timeout 30 "${services[@]}" >"$out/stop.txt" 2>&1; then status=1; fi
        for name in "${names[@]}"; do
            if running="$(docker inspect --format '{{.State.Running}}' "$name" 2>/dev/null)"; then
                printf '%s running=%s\n' "$name" "$running" >>"$out/stop-verification.txt"
                [[ "$running" == false ]] || status=1
            else
                printf '%s state unavailable; verify cleanup manually\n' "$name" >>"$out/stop-verification.txt"
                status=1
            fi
        done
        timeout 60 "${compose[@]}" logs --no-color >"$out/node-logs.txt" 2>&1 || true
    fi
    printf '%s\n' "$status" >"$out/exit-code.txt"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
started=1
"${compose[@]}" up -d --pull never --no-deps "${services[@]}" >"$out/start.txt" 2>&1
metric() {
    awk -v name="$2" '$1 ~ ("^" name "(\\{|$)") {sum += $2; found=1} END {if (!found) exit 1; print sum}' "$1"
}
host_sample_index=0
host_guard() {
    local phase="$1" timestamp resource current_memory current_swap current_disk current_evidence host_oom
    timestamp="$(date -u +%s)"
    cat /proc/meminfo >"$out/raw/host-$host_sample_index.meminfo"
    for resource in memory io cpu; do
        if [[ -r "/proc/pressure/$resource" ]]; then cat "/proc/pressure/$resource" >"$out/raw/host-$host_sample_index.$resource.pressure"; fi
    done
    current_memory="$(awk '$1=="MemAvailable:" {print $2}' "$out/raw/host-$host_sample_index.meminfo")"
    current_swap="$(awk '$1=="SwapFree:" {print $2}' "$out/raw/host-$host_sample_index.meminfo")"
    current_disk="$(df -Pk "$docker_root" | awk 'NR==2 {print $4}')"
    current_evidence="$(df -Pk "$out" | awk 'NR==2 {print $4}')"
    host_oom="$(awk '$1=="oom_kill" {print $2}' /proc/vmstat)"
    printf '%s,%s,%s,%s,%s,%s,%s\n' "$timestamp" "$phase" "$current_memory" "$current_swap" "$current_disk" "$current_evidence" "$host_oom" >>"$out/host-capacity.csv"
    host_sample_index=$((host_sample_index + 1))
    if ((current_memory < live_memory_mib * 1024 || current_disk < minimum_disk * 1024 * 1024 || current_evidence < 10 * 1024 * 1024 || host_oom > host_oom_start)); then
        printf 'Live host safeguard failed; stopping only this run: memory=%s KiB, Docker disk=%s KiB, evidence disk=%s KiB, host OOM kills=%s (initial %s).\n' "$current_memory" "$current_disk" "$current_evidence" "$host_oom" "$host_oom_start" >&2
        return 1
    fi
}
ready_deadline=$((SECONDS + 600))
while true; do
    host_guard startup
    ready=1
    for port in "${ports[@]}"; do
        if ! curl -fsS --max-time 5 "http://127.0.0.1:$port/metrics" >"$out/ready.prom" 2>/dev/null || ! metric "$out/ready.prom" dag_lfb_height >/dev/null; then ready=0; fi
    done
    ((ready)) && break
    ((SECONDS < ready_deadline)) || { printf 'Nodes did not publish required DAG metrics.\n' >&2; exit 1; }
    sleep 5
done
printf 'timestamp,phase,node,held,resident,edges,lfb,cutoff,anonymous_bytes,disk_kib,restarts,max_height,finality_lag\n' >"$out/samples.csv"
sample_index=0
sample() {
    local phase="$1" timestamp name raw memory held resident edges lfb cutoff anon disk restarts running height lag i events oom
    timestamp="$(date -u +%s)"
    host_guard "$phase"
    for i in "${!names[@]}"; do
        ((paused && i == 4)) && continue
        name="${names[$i]}"
        raw="$out/raw/$sample_index-$i.prom"
        curl -fsS --max-time 10 "http://127.0.0.1:${ports[$i]}/metrics" >"$raw"
        held="$(metric "$raw" dag_held_blocks)"
        resident="$(metric "$raw" dag_resident_blocks)"
        edges="$(metric "$raw" dag_resident_edges)"
        lfb="$(metric "$raw" dag_lfb_height)"
        cutoff="$(metric "$raw" dag_resident_cutoff)"
        height="$(metric "$raw" dag_max_height)"
        lag="$(awk -v height="$height" -v lfb="$lfb" 'BEGIN {print height-lfb}')"
        awk -v lfb="$lfb" -v cutoff="$cutoff" -v w="$window" 'BEGIN {expected=lfb>w?lfb-w:0; exit !(cutoff==expected)}'
        memory="$out/raw/$sample_index-$i.memory.stat"
        docker exec "$name" sh -c 'if test -f /sys/fs/cgroup/memory.stat; then cat /sys/fs/cgroup/memory.stat; else cat /sys/fs/cgroup/memory/memory.stat; fi' >"$memory"
        events="$out/raw/$sample_index-$i.memory.events"
        docker exec "$name" sh -c 'if test -f /sys/fs/cgroup/memory.events; then cat /sys/fs/cgroup/memory.events; else cat /sys/fs/cgroup/memory/memory.oom_control; fi' >"$events"
        oom="$(awk '$1=="oom_kill" || $1=="under_oom" {value+=$2} END {print value+0}' "$events")"
        ((oom == 0)) || { printf 'Container OOM event for %s; stopping this shard.\n' "$name" >&2; return 1; }
        anon="$(awk '$1=="anon" {v=$2;found=1} $1=="total_rss" {v=$2;found=1} END {if(!found)exit 1;print v}' "$memory")"
        disk="$(docker exec "$name" du -sk /var/lib/rnode | awk '{print $1}')"
        restarts="$(docker inspect --format '{{.RestartCount}}' "$name")"
        running="$(docker inspect --format '{{.State.Running}}' "$name")"
        [[ "$restarts" == 0 && "$running" == true ]]
        printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' "$timestamp" "$phase" "$name" "$held" "$resident" "$edges" "$lfb" "$cutoff" "$anon" "$disk" "$restarts" "$height" "$lag" >>"$out/samples.csv"
    done
    docker stats --no-stream --format '{{json .}}' "${names[@]}" >>"$out/docker-stats.jsonl"
    sample_index=$((sample_index + 1))
}
lfb_of() {
    awk -F, -v node="$1" '$3==node {value=$7} END {if(value=="")exit 1;print value}' "$out/samples.csv"
}
sample idle
idle_starts=()
for name in "$VALIDATOR1_HOST" "$VALIDATOR2_HOST" "$VALIDATOR3_HOST"; do idle_starts+=("$(lfb_of "$name")"); done
idle_start="${idle_starts[0]}"
idle_started="$(date -u +%s)"
deadline=$((SECONDS + duration))
next_sample=$((SECONDS + interval))
while ((SECONDS < deadline)); do
    tick="$next_sample"
    ((deadline < tick)) && tick="$deadline"
    delay=$((tick - SECONDS))
    ((delay <= 0)) || sleep "$delay"
    sample idle
    next_sample=$((next_sample + interval))
    ((next_sample > SECONDS || SECONDS >= deadline)) || { printf 'Sampling exceeded the configured cadence; evidence is incomplete.\n' >&2; exit 1; }
done
idle_ended="$(date -u +%s)"
idle_end="$(lfb_of "$VALIDATOR1_HOST")"
printf 'node,lfb_start,lfb_end,elapsed_seconds,finalized_heights_per_second\n' >"$out/idle-cadence.csv"
slowest_rate=""
for i in 1 2 3; do
    name="${names[$i]}"
    raw="$out/raw/$((sample_index - 1))-$i.prom"
    for histogram in dag_eviction_time_count dag_history_query_time_count; do
        count="$(metric "$raw" "$histogram")"
        awk -v count="$count" 'BEGIN {exit !(count>0)}' || { printf 'Missing positive %s observations for %s.\n' "$histogram" "$name" >&2; exit 1; }
    done
    end="$(lfb_of "$name")"
    begin="${idle_starts[$((i - 1))]}"
    awk -v start="$begin" -v end="$end" -v w="$window" 'BEGIN {exit !(end-start>w)}' || { printf 'Validator %s did not advance finality beyond one hot window.\n' "$name" >&2; exit 1; }
    rate="$(awk -v start="$begin" -v end="$end" -v elapsed="$((idle_ended - idle_started))" 'BEGIN {printf "%.9f",(end-start)/elapsed}')"
    printf '%s,%s,%s,%s,%s\n' "$name" "$begin" "$end" "$((idle_ended - idle_started))" "$rate" >>"$out/idle-cadence.csv"
    slowest_rate="$(awk -v old="$slowest_rate" -v current="$rate" 'BEGIN {print old=="" || current<old ? current : old}')"
done
estimated_pause="$(awk -v rate="$slowest_rate" -v w="$window" 'BEGIN {seconds=int(2*(w+33)/rate)+1; print seconds>7200?seconds:7200}')"
[[ "$fall_behind_timeout" != auto ]] || fall_behind_timeout="$estimated_pause"
jq -n --argjson rate "$slowest_rate" --argjson estimate "$estimated_pause" --argjson pause "$fall_behind_timeout" --argjson recovery "$catchup_timeout" '{slowest_idle_heights_per_second:$rate,estimated_pause_budget_seconds:$estimate,fall_behind_budget_seconds:$pause,recovery_budget_seconds:$recovery}' >"$out/catchup-budgets.json"
printf 'Observed slowest validator cadence=%s finalized heights/s; pause budget=%ss, recovery budget=%ss.\n' "$slowest_rate" "$fall_behind_timeout" "$catchup_timeout"
DEPLOYER_KEY="$DEPLOYER_KEY" bash "$root/scripts/bench/latency-benchmark.sh" --apply --duration "$load_seconds" --rate "${DAG_SOAK_LOAD_RATE:-2}" --container "$VALIDATOR1_HOST" --http-port 14413 --out-dir "$out/load" >"$out/load-driver.log" 2>&1 &
load_pid=$!
while kill -0 "$load_pid" 2>/dev/null; do sample load; sleep "$interval"; done
wait "$load_pid"
load_pid=""
jq -e --argjson minimum "$minimum_load_samples" '.submitted >= $minimum and .finalized >= $minimum and .submit_errors == 0 and .finalized == .submitted and .latency.samples == .finalized' "$out/load/metrics.json" >/dev/null || { printf 'Load gate failed: require meaningful submission/finalization samples, no submit errors, and every submission finalized.\n' >&2; exit 1; }
sample before_pause
paused_at="$(lfb_of "$READONLY_HOST")"
docker pause "$READONLY_HOST" >"$out/pause.txt"
paused=1
deadline=$((SECONDS + fall_behind_timeout))
while true; do
    sample follower_paused
    current="$(lfb_of "$VALIDATOR1_HOST")"
    if awk -v current="$current" -v at="$paused_at" -v w="$window" 'BEGIN {exit !(current>at+w+32)}'; then break; fi
    ((SECONDS < deadline)) || { printf 'Finality did not advance sufficiently during follower pause.\n' >&2; exit 1; }
    sleep "$interval"
done
curl -fsS --max-time 15 'http://127.0.0.1:14413/api/last-finalized-block?view=summary' >"$out/catchup-target.json"
target_hash="$(jq -er '.blockInfo.blockHash | select(type=="string" and test("^[a-fA-F0-9]{64}$"))' "$out/catchup-target.json")"
target_height="$(jq -er '.blockInfo.blockNumber | select(type=="number")' "$out/catchup-target.json")"
awk -v target="$target_height" -v paused="$paused_at" -v w="$window" 'BEGIN {exit !(target>paused+w+32)}'
docker unpause "$READONLY_HOST" >"$out/unpause.txt"
paused=0
deadline=$((SECONDS + catchup_timeout))
while true; do
    sample catchup
    current="$(lfb_of "$VALIDATOR1_HOST")"
    follower="$(lfb_of "$READONLY_HOST")"
    agreement=1
    for port in 14413 14423 14433 14453; do
        block_file="$out/raw/$sample_index-$port-target.json"
        final_file="$out/raw/$sample_index-$port-finalized.json"
        if ! curl -fsS --max-time 15 "http://127.0.0.1:$port/api/block/$target_hash?view=summary" >"$block_file" ||
            ! jq -e --arg hash "$target_hash" --argjson height "$target_height" '.blockInfo.blockHash==$hash and .blockInfo.blockNumber==$height' "$block_file" >/dev/null ||
            ! curl -fsS --max-time 15 "http://127.0.0.1:$port/api/is-finalized/$target_hash" >"$final_file" ||
            ! jq -e '. == true' "$final_file" >/dev/null; then agreement=0; fi
    done
    if ((agreement)) && awk -v current="$current" -v follower="$follower" 'BEGIN {exit !(follower>=current-8)}'; then break; fi
    ((SECONDS < deadline)) || { printf 'Follower failed to catch up.\n' >&2; exit 1; }
    sleep "$interval"
done
jq -n --argjson start "$idle_started" --argjson end "$idle_ended" --argjson finality_start "$idle_start" --argjson finality_end "$idle_end" --arg hash "$target_hash" --argjson height "$target_height" --slurpfile load "$out/load/metrics.json" --slurpfile budgets "$out/catchup-budgets.json" '{execution_complete:true,idle_elapsed_seconds:($end-$start),continuous_24h_idle:($end-$start>=86400),idle_lfb_start:$finality_start,idle_lfb_end:$finality_end,all_validators_advanced_beyond_window:true,load_and_catchup_complete:true,load:$load[0],catchup_budgets:$budgets[0],catchup_target:{hash:$hash,height:$height,finalized_and_visible_on_all_validators_and_follower:true},acceptance:"requires allocator, latency and plateau analysis"}' >"$out/result.json"
printf 'Continuous run evidence written to %s; allocator and plateau acceptance are separate.\n' "$out"

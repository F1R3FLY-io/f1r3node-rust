#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
apply=0
duration="${DAG_SOAK_SECONDS:-86400}"
interval="${DAG_SOAK_INTERVAL:-60}"
load_seconds="${DAG_SOAK_LOAD_SECONDS:-300}"
catchup_timeout="${DAG_SOAK_CATCHUP_TIMEOUT:-7200}"
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
  DAG_SOAK_CATCHUP_TIMEOUT=7200 Timeout for both falling behind and catching up
  DAG_SOAK_OUTPUT=...           New evidence directory; default under target/
  DAG_SOAK_ENV_FILE=...         Compose env file; defaults to docker/.env.example
  DAG_SOAK_MIN_MEMORY_MIB=8192   Required host MemAvailable before starting
  DAG_SOAK_MIN_DISK_GIB=20      Required available space on Docker data filesystem

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
for value in "$duration" "$interval" "$load_seconds" "$catchup_timeout" "$window"; do
    [[ "$value" =~ ^[1-9][0-9]*$ ]] || { printf 'Durations/window must be positive integers.\n' >&2; exit 2; }
done
[[ "$window" == 1024 ]] || { printf 'This runner uses the existing shard configuration with W=1024.\n' >&2; exit 2; }
printf 'Plan: pinned image %s, idle %ss at %ss cadence, load %ss, catch-up beyond %s heights; evidence %s\n' "${image:-REQUIRED}" "$duration" "$interval" "$load_seconds" "$window" "$out"
((apply)) || exit 0
[[ "$image" =~ (^sha256:[a-f0-9]{64}$|@sha256:[a-f0-9]{64}$) ]] || { printf 'DAG_SOAK_IMAGE must be an immutable image digest.\n' >&2; exit 2; }
: "${DEPLOYER_KEY:?Set the funded development DEPLOYER_KEY}"
for tool in docker curl jq awk; do command -v "$tool" >/dev/null; done
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
jq -n --arg project "$project" --arg image "$image" --argjson seconds "$duration" --argjson interval "$interval" '{project:$project,image:$image,idle_seconds:$seconds,interval_seconds:$interval,validators:3,window:1024,allocator_live_heap_available:false}' >"$out/run.json"
started=0
paused=0
load_pid=""
cleanup() {
    local status=$?
    trap - EXIT
    if [[ -n "$load_pid" ]] && kill -0 "$load_pid" 2>/dev/null; then
        kill "$load_pid" 2>/dev/null || true
        wait "$load_pid" 2>/dev/null || true
    fi
    if ((started)); then
        if ((paused)); then docker unpause "$READONLY_HOST" >/dev/null || true; fi
        "${compose[@]}" logs --no-color >"$out/node-logs.txt" 2>&1 || true
        "${compose[@]}" stop "${services[@]}" >"$out/stop.txt" 2>&1 || true
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
ready_deadline=$((SECONDS + 600))
while true; do
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
    local phase="$1" timestamp name raw memory held resident edges lfb cutoff anon disk restarts running height lag i
    timestamp="$(date -u +%s)"
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
idle_start="$(lfb_of "$VALIDATOR1_HOST")"
idle_started="$(date -u +%s)"
deadline=$((SECONDS + duration))
while ((SECONDS < deadline)); do
    remaining=$((deadline - SECONDS))
    delay="$interval"
    ((remaining < delay)) && delay="$remaining"
    sleep "$delay"
    sample idle
done
idle_ended="$(date -u +%s)"
idle_end="$(lfb_of "$VALIDATOR1_HOST")"
awk '$1 ~ /^dag_eviction_time_(sum|count)(\{|$)/ {found=1} END {exit !found}' "$out/raw/$((sample_index - 1))-1.prom" || { printf 'Missing eviction latency measurements.\n' >&2; exit 1; }
awk '$1 ~ /^dag_history_query_time_(sum|count)(\{|$)/ {found=1} END {exit !found}' "$out/raw/$((sample_index - 1))-1.prom" || { printf 'Missing historical query latency measurements.\n' >&2; exit 1; }
awk -v start="$idle_start" -v end="$idle_end" -v w="$window" 'BEGIN {exit !(end-start>w)}' || { printf 'Idle phase did not advance finality beyond one hot window; evidence insufficient.\n' >&2; exit 1; }
DEPLOYER_KEY="$DEPLOYER_KEY" bash "$root/scripts/bench/latency-benchmark.sh" --apply --duration "$load_seconds" --rate "${DAG_SOAK_LOAD_RATE:-2}" --container "$VALIDATOR1_HOST" --http-port 14413 --out-dir "$out/load" >"$out/load-driver.log" 2>&1 &
load_pid=$!
while kill -0 "$load_pid" 2>/dev/null; do sample load; sleep "$interval"; done
wait "$load_pid"
load_pid=""
sample before_pause
paused_at="$(lfb_of "$READONLY_HOST")"
docker pause "$READONLY_HOST" >"$out/pause.txt"
paused=1
deadline=$((SECONDS + catchup_timeout))
while true; do
    sample follower_paused
    current="$(lfb_of "$VALIDATOR1_HOST")"
    if awk -v current="$current" -v at="$paused_at" -v w="$window" 'BEGIN {exit !(current>at+w+32)}'; then break; fi
    ((SECONDS < deadline)) || { printf 'Finality did not advance sufficiently during follower pause.\n' >&2; exit 1; }
    sleep "$interval"
done
docker unpause "$READONLY_HOST" >"$out/unpause.txt"
paused=0
deadline=$((SECONDS + catchup_timeout))
while true; do
    sample catchup
    current="$(lfb_of "$VALIDATOR1_HOST")"
    follower="$(lfb_of "$READONLY_HOST")"
    if awk -v current="$current" -v follower="$follower" 'BEGIN {exit !(follower>=current-8)}'; then break; fi
    ((SECONDS < deadline)) || { printf 'Follower failed to catch up.\n' >&2; exit 1; }
    sleep "$interval"
done
jq -n --argjson start "$idle_started" --argjson end "$idle_ended" --argjson finality_start "$idle_start" --argjson finality_end "$idle_end" '{execution_complete:true,idle_elapsed_seconds:($end-$start),continuous_24h_idle:($end-$start>=86400),idle_lfb_start:$finality_start,idle_lfb_end:$finality_end,load_and_catchup_complete:true,acceptance:"requires allocator, latency and plateau analysis"}' >"$out/result.json"
printf 'Continuous run evidence written to %s; allocator and plateau acceptance are separate.\n' "$out"

#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
DEST="${1:-$ROOT/target/erd-tools/observed}"
mkdir -p "$DEST"
DEST="$(cd "$DEST" && pwd)"
export CARGO_TARGET_DIR="$ROOT/target/erd-tools/build"
cargo build --locked --manifest-path "$ROOT/docs/erd/Cargo.toml" --bin erd-store-stats
scratch="$(mktemp -d "$ROOT/target/erd-tools/collect.XXXXXX")"
active_container=""
remote_binary=""
cleanup() {
    if [[ -n "$active_container" && -n "$remote_binary" ]]; then
        docker exec "$active_container" rm -f -- "$remote_binary" >/dev/null
    fi
    rm -rf -- "$scratch"
}
trap cleanup EXIT
start="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
nodes=(rnode.bootstrap rnode.validator1 rnode.validator2 rnode.validator3 rnode.readonly)
queues=(rnode.deploy-queue1 rnode.deploy-queue2 rnode.deploy-queue3)
environments=(blockstorage dagstorage deploystorage rspace/history rspace/cold eval/history eval/cold reporting transaction rspace/casper/v2/channels)
names=(
    blocks
    blocks-approved,block-metadata,latest-messages,dag-membership-v1,dag-height-v1,dag-children-v1,dag-sequence-v1,dag-equivocations-v1,genesis-hash,dag-history-state-v1,dag-gc-retry-v1,floor-index,frontier-index,mergeable-channel-cache,deploy-lifecycle-events,deploy-lifecycle-terminal,carrier-index,carrier-index-seeds,carrier-index-meta
    deploy_storage,rejected_deploy_buffer,deploy_storage_seeds,rejected_deploy_buffer_seeds,deploy_storage_quarantine,rejected_deploy_buffer_quarantine
    rspace-history,rspace-roots
    rspace-cold
    eval-history,eval-roots
    eval-cold
    reporting-cache
    transaction
    db
)
for container in "${nodes[@]}" "${queues[@]}"; do
    active_container="$container"
    remote_binary="$(docker exec "$container" mktemp /tmp/erd-store-stats.XXXXXX)"
    docker cp "$CARGO_TARGET_DIR/debug/erd-store-stats" "$container:$remote_binary"
    docker exec "$container" chmod 755 "$remote_binary"
    docker inspect --format '{{json .State}}' "$container" | jq --arg container "$container" '{container:$container,running:.Running,started:.StartedAt,health:(.Health.Status // "not configured"),oom_killed:.OOMKilled}' >> "$scratch/containers.jsonl"
    docker inspect --format '{{.RestartCount}}' "$container" | jq --arg container "$container" '{container:$container,restarts:.}' >> "$scratch/restarts.jsonl"
    docker inspect --format '{{json .Image}}' "$container" | jq --arg container "$container" '{container:$container,image_id:.}' >> "$scratch/images.jsonl"
    if [[ "$container" == rnode.deploy-queue* ]]; then
        docker exec "$container" "$remote_binary" /var/lib/deploy-queue records,payloads,meta | jq --arg container "$container" '. + {container:$container}' >> "$scratch/stores.jsonl"
    else
        docker exec "$container" curl --fail --silent --show-error --max-time 10 http://127.0.0.1:40403/api/status | jq --arg container "$container" '{container:$container,isReady,peers,latestBlockNumber,lastFinalizedBlockNumber}' >> "$scratch/status.jsonl"
        docker exec "$container" curl --fail --silent --show-error --max-time 10 http://127.0.0.1:40403/metrics | jq -Rs --arg container "$container" '{container:$container,metrics:[split("\n")[] | select(test("^(dag_(resident|held|lfb|max)_|block_index_cache_size|parents_post_state_cache_size|replay_cache_(size|bytes))"))]}' >> "$scratch/metrics.jsonl"
        for i in "${!environments[@]}"; do
            docker exec "$container" "$remote_binary" "/var/lib/rnode/${environments[$i]}" "${names[$i]}" | jq --arg container "$container" '. + {container:$container}' >> "$scratch/stores.jsonl"
        done
    fi
    docker exec "$container" rm -f -- "$remote_binary"
    remote_binary=""
    active_container=""
done
docker stats --no-stream --format '{{json .}}' "${nodes[@]}" "${queues[@]}" | jq '{container:.Name,cpu:.CPUPerc,memory:.MemUsage,memory_percent:.MemPerc,pids:.PIDs}' > "$scratch/resources.jsonl"
jq -n --arg start "$start" --arg finish "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg revision "$(git -C "$ROOT" rev-parse HEAD)" \
    --slurpfile stores "$scratch/stores.jsonl" --slurpfile metrics "$scratch/metrics.jsonl" \
    --slurpfile status "$scratch/status.jsonl" --slurpfile containers "$scratch/containers.jsonl" \
    --slurpfile resources "$scratch/resources.jsonl" --slurpfile restarts "$scratch/restarts.jsonl" \
    --slurpfile images "$scratch/images.jsonl" \
    '{schema_version:1,started_utc:$start,finished_utc:$finish,documentation_base_revision:$revision,stores:$stores,metrics:$metrics,status:$status,containers:$containers,resources:$resources,restarts:$restarts,images:$images}' > "$DEST/observed.json"
jq -r '
    "# Runtime Cardinality and Size\n",
    "Captured from \(.started_utc) to \(.finished_utc). Base revision: `\(.documentation_base_revision)`.\n",
    "[Raw observations](observed.json) | [Storage formulas](sizing.md) | [Memory limits](../memory/sizing.md)\n",
    "This is a read-only sample of five nodes and three queues, not a load test or heap capture. No node was restarted. Each environment has its own read transaction. Counts across environments and replicas are not one atomic shard snapshot.\n",
    "The probe uses normal LMDB reader locking. It can update reader slots in lock.mdb, but opens data.mdb read-only. A temporary executable is copied into each container and removed after use.\n",
    "Exact table counts and live page bytes come from LMDB statistics. Record-size samples visit at most 256 first keys per table within a two-second budget per environment. They are not random samples. Sample totals cover the whole table only when complete is true in the JSON.\n",
    "## Process Resources\n",
    "Docker memory accounting is not a sum of entity heap sizes. CPU is a short sample, not a sustained-load result.\n",
    "| Container | Memory / limit | CPU | PIDs |", "| --- | --- | --- | --- |",
    (.resources[] | "| \(.container) | \(.memory) | \(.cpu) | \(.pids) |"),
    "\n## Node Status\n",
    "latestBlockNumber is the next block number, not the current tip.\n",
    "| Node | Ready | Peers | Next block | Finalized block |", "| --- | --- | --- | --- | --- |",
    (.status[] | "| \(.container) | \(.isReady) | \(.peers) | \(.latestBlockNumber) | \(.lastFinalizedBlockNumber) |"),
    "\n## Resident Metrics\n",
    (.metrics[] | "### \(.container)\n\n```text\n\(.metrics | join("\n"))\n```\n"),
    "## Environment Files\n",
    "All byte columns are bytes, not KiB. File allocation can differ from file length. Map size is address space, not RAM use. Absent environments have no measured table count.\n",
    "| Node | Environment | File length | Allocated bytes | Map bytes | Read transaction |", "| --- | --- | --- | --- | --- | --- |",
    (.stores[] | "| \(.container) | \(.path) | \(.file_length_bytes // .status) | \(.file_allocated_bytes // "n/a") | \(.map_size_bytes // "n/a") | \(.transaction_id // "n/a") |"),
    "\n## Database Records\n",
    "Page bytes include live branch, leaf, and overflow pages. They exclude environment metadata and free pages. Min, mean, and max below describe sampled encoded key plus value bytes, including adapter prefixes. A missing sample is n/a, not zero.\n",
    "| Node / environment | Database | Exact rows | Live page bytes | Sample rows | Min / mean / max record bytes |", "| --- | --- | --- | --- | --- | --- |",
    (.stores[] | . as $s | .tables[] | "| \($s.container) / \($s.path | sub("^/var/lib/"; "")) | \(.name) | \(.records // .status) | \(.page_bytes // "n/a") | \(.sample.records // "n/a") | \(.sample.record_min_bytes // "n/a") / \(.sample.record_mean_bytes // "n/a") / \(.sample.record_max_bytes // "n/a") |"),
    "\n## Reproduce\n",
    "Run `bash docs/erd/collect-sizes.sh` from this checkout. It writes generated observations under target/erd-tools/observed by default. It requires the named local containers, Docker, curl in each node image, jq, Rust, and Cargo.\n",
    "Review the output before replacing this recorded sample. The script does not submit deploys, start containers, stop containers, or dump database keys and values."
' "$DEST/observed.json" > "$DEST/observed.md"
printf 'Generated %s/observed.json and observed.md\n' "$DEST"

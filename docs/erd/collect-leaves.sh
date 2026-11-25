#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
DEST="${1:-$ROOT/target/erd-tools/leaves}"
mkdir -p "$DEST"
DEST="$(cd "$DEST" && pwd)"
export CARGO_TARGET_DIR="$ROOT/target/erd-tools/build"
cargo build --locked --manifest-path "$ROOT/docs/erd/Cargo.toml" --bin erd-leaf-stats
scratch="$(mktemp -d "$ROOT/target/erd-tools/leaves.XXXXXX")"
active=""
cleanup() {
    if [[ -n "$active" ]]; then docker rm -f "$active" >/dev/null 2>&1 || true; fi
    rm -rf -- "$scratch"
}
trap cleanup EXIT
start="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
docker exec rnode.bootstrap curl -fsS --max-time 15 http://127.0.0.1:40403/api/last-finalized-block | jq -e '.blockInfo | select(.isFinalized == true) | {blockHash,blockNumber,postStateHash,isFinalized}' > "$scratch/root.json"
root="$(jq -r .postStateHash "$scratch/root.json")"
block="$(jq -r .blockHash "$scratch/root.json")"
for node in rnode.bootstrap rnode.validator1 rnode.validator2 rnode.validator3 rnode.readonly; do
    docker exec "$node" curl -fsS --max-time 15 "http://127.0.0.1:40403/api/block/$block" | jq -e --arg root "$root" '.blockInfo | select(.isFinalized == true and .postStateHash == $root) | {blockHash,blockNumber,postStateHash,isFinalized}' > "$scratch/verified.json"
    volume="$(docker inspect --format '{{range .Mounts}}{{if eq .Destination "/var/lib/rnode"}}{{.Name}}{{end}}{{end}}' "$node")"
    image="$(docker inspect --format '{{.Image}}' "$node")"
    [[ -n "$volume" ]]
    docker inspect --format '{{json .State}}' "$node" | jq --arg node "$node" '{node:$node,running:.Running,started:.StartedAt,oom_killed:.OOMKilled,health:.Health.Status}' >> "$scratch/before.jsonl"
    active="erd-leaf-probe-$$"
    printf 'Measuring %s at finalized block %s\n' "$node" "$(jq -r .blockNumber "$scratch/root.json")" >&2
    timeout 150 docker run --rm --name "$active" --network none --pid "container:$node" --memory 512m --memory-swap 512m --cpus 1 --pids-limit 64 \
        --read-only --cap-drop ALL --security-opt no-new-privileges --user 0:0 \
        --mount "type=volume,source=$volume,target=/data" \
        --mount "type=bind,source=$CARGO_TARGET_DIR/debug/erd-leaf-stats,target=/probe,readonly" \
        --entrypoint /probe "$image" /data "$root" > "$scratch/probe.json"
    active=""
    jq --arg node "$node" --arg image "$image" '. + {node:$node,image_id:$image}' "$scratch/probe.json" >> "$scratch/probes.jsonl"
    docker inspect --format '{{json .State}}' "$node" | jq --arg node "$node" '{node:$node,running:.Running,started:.StartedAt,oom_killed:.OOMKilled,health:.Health.Status}' >> "$scratch/after.jsonl"
    docker exec "$node" curl -fsS --max-time 15 http://127.0.0.1:40403/api/status | jq --arg node "$node" '{node:$node,isReady,peers,latestBlockNumber,lastFinalizedBlockNumber}' >> "$scratch/status.jsonl"
done
jq -n --arg start "$start" --arg finish "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg revision "$(git -C "$ROOT" rev-parse HEAD)" \
    --slurpfile finalized "$scratch/root.json" --slurpfile nodes "$scratch/probes.jsonl" \
    --slurpfile before "$scratch/before.jsonl" --slurpfile after "$scratch/after.jsonl" --slurpfile status "$scratch/status.jsonl" \
    '{schema_version:1,started_utc:$start,finished_utc:$finish,documentation_base_revision:$revision,finalized_block:$finalized[0],nodes:$nodes,before:$before,after:$after,status_after:$status}' > "$DEST/leaves-observed.json"
printf 'Generated %s/leaves-observed.json\n' "$DEST"
"$CARGO_TARGET_DIR/debug/erd-leaf-stats" --report "$DEST/leaves-observed.json" "$DEST/leaf-measurements.md"

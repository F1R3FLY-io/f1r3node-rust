#!/usr/bin/env bash
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
output="${1:-$root/target/compose-log-policy}"
mkdir -p -- "$output"

check() {
    local name="$1" expected="$2"
    shift 2
    local args=() source
    for source in "$@"; do
        args+=(-f "$root/$source")
    done
    docker compose --project-name node-log-policy "${args[@]}" config \
        --no-interpolate --no-env-resolution --no-path-resolution \
        --format json >"$output/$name.private.json"
    jq -e --argjson expected "$expected" '
        [.services[] | select(.image | contains("f1r3fly-rust"))] as $nodes
        | ($nodes | length) == $expected
        and all($nodes[];
            .logging.driver == "json-file"
            and .logging.options["max-size"] == "100m"
            and .logging.options["max-file"] == "3"
            and .command[0] == "--log-sink=stdout"
            and .command[1] == "run"
            and ([.command[] | select(startswith("--log-sink"))] | length) == 1
        )
    ' "$output/$name.private.json" >/dev/null
    printf '%s: %s node services passed\n' "$name" "$expected"
}

check shard 5 docker/shard.yml
check standalone 1 docker/standalone.yml
check observer 1 docker/observer.yml
check validator4 1 docker/validator4.yml
check shard-vps1 1 docker/shard.vps1.yml
check shard-vps2 3 docker/shard.vps2.yml
check ci-shard 5 docker/shard.yml docker/ci-ports.shard.yml
check ci-standalone 1 docker/standalone.yml docker/ci-ports.standalone.yml

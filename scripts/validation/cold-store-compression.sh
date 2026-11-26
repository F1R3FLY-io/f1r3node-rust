#!/usr/bin/env bash
set -euo pipefail
umask 077
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
BASE="${COLD_COMPRESSION_BASE_IMAGE:?Set COLD_COMPRESSION_BASE_IMAGE to a local node runtime image}"
RUN="$(mktemp -d "$ROOT/target/cold-store-validation/shard.XXXXXX")"
NAME="cold-compression-$(basename "$RUN" | tr '[:upper:].' '[:lower:]-')"
COMPOSE="$RUN/compose.json"
CLIENT="$ROOT/target/release/examples/cold_store_validation"
INVENTORY="$ROOT/target/release/examples/cold_store_inventory"
test -x "$CLIENT"
test -x "$INVENTORY"
cp "$CLIENT" "$RUN/client"
cp "$INVENTORY" "$RUN/inventory"
CLIENT="$RUN/client"
INVENTORY="$RUN/inventory"
file target/release/node | grep -Eq 'ELF.*(executable|shared object)'
(( $(df --output=avail -k "$ROOT" | tail -1) > 4194304 )) || { printf 'At least 4 GiB free disk is required.\n' >&2; exit 2; }
mkdir -p "$RUN/image/runtime-libs" "$RUN/config" "$RUN/state"
"$CLIENT" prepare "$RUN/keys"
cp target/release/node "$RUN/image/node"
cp node/docker-entrypoint-rust.sh "$RUN/image/docker-entrypoint.sh"
while IFS= read -r library; do
    [[ "$library" == /* && -f "$library" ]] || exit 2
    cp -L "$library" "$RUN/image/runtime-libs/"
done < <(ldd target/release/node | awk '$1 == "libssl.so.1.1" || $1 == "libcrypto.so.1.1" {print $3}')
docker build --build-arg "BASE=$BASE" -f scripts/validation/cold-store-compression.Dockerfile -t "$NAME:validation" "$RUN/image" >"$RUN/image-build.log" 2>&1
sha256sum target/release/node "$CLIENT" "$INVENTORY" >"$RUN/binaries.sha256"
sha256sum Cargo.lock rspace++/src/rspace/history/cold_store_codec.rs rspace++/src/rspace/history/history_repository.rs rspace++/src/rspace/rspace.rs rspace++/examples/cold_store_inventory.rs node/src/rust/runtime/setup.rs node/examples/cold_store_validation.rs node/docker-entrypoint-rust.sh scripts/validation/cold-store-compression.sh scripts/validation/cold-store-compression.Dockerfile >"$RUN/sources.sha256"
configure() {
    local role="$1" compressed="$2"
    {
        printf 'include "%s"\n' /run/config/base.conf
        printf 'storage.cold-store-compression = %s\n' "$compressed"
        printf 'protocol-server.network-id = "%s"\n' "$NAME"
        printf 'protocol-client.network-id = "%s"\n' "$NAME"
        if [[ "$role" != observer ]]; then
            printf 'casper.validator-private-key = "%s"\n' "$(<"$RUN/keys/$role.key")"
        fi
    } >"$RUN/config/$role.conf"
}
for role in boot validator1 validator2 validator3 observer; do
    mkdir -p "$RUN/state/$role"
    compressed=true
    [[ "$role" == boot || "$role" == validator1 ]] && compressed=false
    configure "$role" "$compressed"
done
jq -n --arg root "$ROOT" --arg run "$RUN" --arg name "$NAME" --arg image "$NAME:validation" --arg user "$(id -u):$(id -g)" --slurpfile keys "$RUN/keys/public.json" '
  def bind($source; $target): {type:"bind",source:$source,target:$target,read_only:true};
  def service($role): {
    image:$image,container_name:($name+"-"+$role),user:$user,restart:"no",cpus:1.0,mem_limit:"4g",memswap_limit:"4g",pids_limit:512,
    environment:{OPENAI_ENABLED:"false",OLLAMA_ENABLED:"false",MALLOC_ARENA_MAX:"2",RUST_LOG:"info,tonic=error,h2=error,hyper=error"},
    command:(["--log-sink=stdout","run","--host="+$role,"--bootstrap=rnode://1e780e5dfbe0a3d9470a2b414f502d59402e09c2@boot?protocol=40400&discovery=40404","--no-upnp","--allow-private-addresses"] +
      (if $role=="boot" then ["--ceremony-master-mode","--required-signatures=2","--heartbeat-disabled"]
       elif $role=="observer" then ["--heartbeat-disabled"]
       else ["--genesis-validator","--validator-public-key="+$keys[0][$role]] end)),
    volumes:([{type:"bind",source:($run+"/state/"+$role),target:"/var/lib/rnode"},
      bind($run+"/config/"+$role+".conf";"/var/lib/rnode/f1r3fly.conf"),
      bind($root+"/docker/conf/default.conf";"/run/config/base.conf"),
      bind($run+"/keys/wallets.txt";"/var/lib/rnode/genesis/wallets.txt"),bind($run+"/keys/bonds.txt";"/var/lib/rnode/genesis/bonds.txt")] +
      (if $role=="boot" then [bind($root+"/docker/certs/bootstrap/node.certificate.pem";"/var/lib/rnode/node.certificate.pem"),bind($root+"/docker/certs/bootstrap/node.key.pem";"/var/lib/rnode/node.key.pem")] else [] end)),
    healthcheck:{test:["CMD","curl","-fsS","http://127.0.0.1:40403/api/ready"],interval:"10s",timeout:"5s",retries:6,start_period:"180s"},
    logging:{driver:"json-file",options:{"max-size":"10m","max-file":"2"}},
    labels:{"f1r3node.validation":"cold-store-compression"}
  };
  {name:$name,services:{boot:service("boot"),validator1:service("validator1"),validator2:service("validator2"),validator3:service("validator3"),observer:service("observer")},networks:{default:{internal:true}}}
' >"$COMPOSE"
compose() { docker compose -p "$NAME" -f "$COMPOSE" "$@"; }
monitor_pid=""
cleanup() {
    local result=$?
    trap - EXIT INT TERM
    set +e
    if [[ -n "$monitor_pid" ]]; then kill "$monitor_pid" 2>/dev/null; wait "$monitor_pid" 2>/dev/null; fi
    compose ps --all --format json >"$RUN/containers.json"
    for role in boot validator1 validator2 validator3 observer; do
        docker logs "$NAME-$role" 2>&1 | sed -E '/[Pp]rivate.?[Kk]ey|[Pp]assword|[Ss]ecret/c\[sensitive log line omitted]' >"$RUN/$role.log"
    done
    compose down --timeout 30 >"$RUN/cleanup.log" 2>&1
    printf 'Validation exit=%s; retained evidence and disposable state: %s\n' "$result" "$RUN"
    exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
wait_ready() {
    local deadline=$((SECONDS+600))
    for role in "$@"; do
        while ! docker exec "$NAME-$role" curl -fsS http://127.0.0.1:40403/api/ready >/dev/null 2>&1; do
            (( SECONDS < deadline )) || { printf 'Readiness deadline: %s\n' "$role" >&2; return 1; }
            [[ "$(docker inspect -f '{{.State.Running}}' "$NAME-$role")" == true ]] || return 1
            sleep 3
        done
    done
}
capture() {
    local label="$1"
    local roles=(boot validator1 validator2 validator3)
    if [[ "$label" == final ]]; then roles+=(observer); fi
    for role in "${roles[@]}"; do
        docker exec "$NAME-$role" curl -fsS http://127.0.0.1:40403/api/status >"$RUN/$label-$role-status.json"
        docker exec "$NAME-$role" curl -fsS http://127.0.0.1:40403/metrics >"$RUN/$label-$role-metrics.txt"
        docker inspect -f '{{json .State}}' "$NAME-$role" >"$RUN/$label-$role-container-state.json"
        docker inspect -f '{{.RestartCount}}' "$NAME-$role" >"$RUN/$label-$role-restarts.json"
    done
}
compose up -d boot validator1 validator2 validator3 >"$RUN/start.log" 2>&1
(
    while true; do
        docker stats --no-stream --format '{{json .}}' "$NAME-boot" "$NAME-validator1" "$NAME-validator2" "$NAME-validator3" >>"$RUN/resources.jsonl" 2>/dev/null
        sleep 15
    done
) &
monitor_pid=$!
wait_ready boot validator1 validator2 validator3
capture genesis
ENDPOINTS="docker://$NAME-validator1,docker://$NAME-validator2,docker://$NAME-validator3"
"$CLIENT" run "$RUN/keys" "$ENDPOINTS" >"$RUN/workload.log" 2>&1
capture workload
compose up -d observer >"$RUN/joiner-start.log" 2>&1
wait_ready observer
ENDPOINTS+=",docker://$NAME-observer"
"$CLIENT" verify "$RUN/keys" "$ENDPOINTS" >"$RUN/joiner-verification.json"
compose stop --timeout 30 validator2 >"$RUN/restart.log" 2>&1
configure validator2 false
compose start validator2 >>"$RUN/restart.log" 2>&1
wait_ready validator2
"$CLIENT" verify "$RUN/keys" "$ENDPOINTS" >"$RUN/restart-verification.json"
capture recovery
sleep 60
"$CLIENT" verify "$RUN/keys" "$ENDPOINTS" >"$RUN/final-verification.json"
capture final
for role in validator1 validator2 validator3; do
    jq -e --slurpfile before "$RUN/recovery-$role-status.json" '.lastFinalizedBlockNumber > $before[0].lastFinalizedBlockNumber' "$RUN/final-$role-status.json" >/dev/null
done
for role in boot validator1 validator2 validator3 observer; do
    jq -e '.Running and (.OOMKilled|not) and .ExitCode==0' "$RUN/final-$role-container-state.json" >/dev/null
    jq -e '.==0' "$RUN/final-$role-restarts.json" >/dev/null
done
docker stats --no-stream --format '{{json .}}' "$NAME-observer" >>"$RUN/resources.jsonl"
compose stop --timeout 30 >"$RUN/stop.log" 2>&1
for role in validator1 validator2 validator3 observer; do
    "$INVENTORY" "$RUN/state/$role/rspace/cold" >"$RUN/$role-cold-inventory.json"
done
jq -e '.compressed_rows==0 and .rows>0' "$RUN/validator1-cold-inventory.json" >/dev/null
for role in validator2 validator3 observer; do jq -e '.compressed_rows>0' "$RUN/$role-cold-inventory.json" >/dev/null; done
jq -n --arg run "$RUN" --arg image "$(docker image inspect -f '{{.Id}}' "$NAME:validation")" '{schema:"cold-store-live-validation/v1",run:$run,image:$image,genesis:true,counter_finalized:100,locks_finalized:100,common_finalized_state:true,restart_write_disabled:true,late_joiner:true,continued_finalization:true,oom:false,automatic_restarts:0,physical_inventory_verified:true}' >"$RUN/report.json"

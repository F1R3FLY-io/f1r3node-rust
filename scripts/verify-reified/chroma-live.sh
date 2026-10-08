#!/usr/bin/env bash
# The live leg of the ChromaDB search backend of the VectorDB spaces (P6 step 7c, decision 61).
#
# The script starts a disposable server of the image that docker/shard.yml pins, and it runs
# rholang/tests/reified_vectordb_chroma_live.rs against it with the chromadb feature and
# RRS_LIVE_CHROMA=1 (decision 59: an unreachable server fails the leg, which never skips):
#   * CHROMA_ENDPOINT is the bridge address of the container, so the run needs no published
#     port (some hosts cannot publish ports). On a host whose bridge network is not reachable,
#     set CHROMA_ENDPOINT to a running server of the pinned image and set RRS_CHROMA_EXTERNAL=1.
#   * CHROMA_DATABASE is new for each run, so no run sees the records of an earlier run.
#   * FASTEMBED_CACHE_DIR and TMPDIR are under target/, never in the source tree or /tmp.
#   * The cargo run is bounded by a systemd user scope, unless RRS_SANDBOX=1.
# The container is stopped on every exit path.
#
# Usage: bash scripts/verify-reified/chroma-live.sh [extra cargo test arguments]
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
IMAGE=$(grep -o 'chromadb/chroma@sha256:[0-9a-f]*' "$ROOT/docker/shard.yml" | head -1)
[[ -n "$IMAGE" ]] || { echo "chroma-live: docker/shard.yml pins no ChromaDB image" >&2; exit 1; }

if [[ "${RRS_CHROMA_EXTERNAL:-}" != "1" ]]; then
  NAME="rrs-chroma-live-$$"
  trap 'docker stop "$NAME" >/dev/null 2>&1 || true' EXIT
  docker run -d --rm --name "$NAME" "$IMAGE" >/dev/null
  ADDRESS=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$NAME")
  export CHROMA_ENDPOINT="http://$ADDRESS:8000"
fi
: "${CHROMA_ENDPOINT:?chroma-live: CHROMA_ENDPOINT is not set}"
for _ in $(seq 1 60); do
  curl -sf -m 1 "$CHROMA_ENDPOINT/api/v2/heartbeat" >/dev/null && break
  sleep 1
done
curl -sf -m 2 "$CHROMA_ENDPOINT/api/v2/heartbeat" >/dev/null \
  || { echo "chroma-live: the server at $CHROMA_ENDPOINT does not answer" >&2; exit 1; }

export RRS_LIVE_CHROMA=1
export CHROMA_DATABASE="rrs_live_$$_$(date +%s)"
export FASTEMBED_CACHE_DIR="$ROOT/target/fastembed-cache"
export TMPDIR="$ROOT/target/tmp" CARGO_INCREMENTAL=0
mkdir -p "$FASTEMBED_CACHE_DIR" "$TMPDIR"
cd "$ROOT"
COMMAND=(cargo test -p rholang --features chromadb --test reified_vectordb_chroma_live "$@")
if [[ "${RRS_SANDBOX:-}" != "1" ]]; then
  COMMAND=(systemd-run --user --scope --quiet -p MemoryMax=24G -p CPUQuota=800% \
    -p RuntimeMaxSec=3600 -- "${COMMAND[@]}")
fi
"${COMMAND[@]}"

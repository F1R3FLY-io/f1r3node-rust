#!/usr/bin/env bash
# A bench segment whose shard fails to start must put the compose error on
# stderr. The soak driver copies only the segment.log tail into the job log,
# and compose-up.log stays on the runner until the run ends. Soak 38184406100
# lost the cause of two such failures this way.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/repo/docker" "$TMP/bin" "$TMP/out"
: >"$TMP/repo/docker/shard.yml"
: >"$TMP/repo/docker/ci-ports.shard.yml"
printf 'BOOTSTRAP_PRIVATE_KEY=fixture\n' >"$TMP/repo/docker/.env"

cat >"$TMP/bin/docker" <<'SH'
#!/usr/bin/env bash
case " $* " in
	*" up "*)
		echo ' Container grafana  Creating'
		echo 'Error response from daemon: Conflict. The container name "/grafana" is already in use by container "fixture".'
		exit 1
		;;
esac
exit 0
SH
chmod +x "$TMP/bin/docker"

status=0
PATH="$TMP/bin:$PATH" NODE_REPO_DIR="$TMP/repo" OUT_DIR="$TMP/out" \
	"$ROOT/scripts/bench/run-bench-segment.sh" >"$TMP/stdout" 2>"$TMP/stderr" || status=$?

[ "$status" -eq 1 ] || { echo "expected exit 1, got $status" >&2; cat "$TMP/stderr" >&2; exit 1; }
grep -Fq 'shard compose up failed' "$TMP/stderr" || { echo "missing failure line" >&2; cat "$TMP/stderr" >&2; exit 1; }
grep -Fq 'The container name "/grafana" is already in use' "$TMP/stderr" || {
	echo "compose error is not on stderr" >&2
	cat "$TMP/stderr" >&2
	exit 1
}


# The bench shard must publish its host ports below the kernel ephemeral
# range. Soak 38184406100 lost bench segments 11 and 13 to "failed to bind
# host port 0.0.0.0:40452/tcp: address already in use": 40400-40455 is inside
# 32768-60999, so an outbound connection can hold one of those ports.
mkdir -p "$TMP/bin2" "$TMP/out2"
cat >"$TMP/bin2/docker" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$CALLS"
exit 0
SH
cat >"$TMP/bin2/curl" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$PROBES"
exit 7
SH
chmod +x "$TMP/bin2/docker" "$TMP/bin2/curl"
status=0
CALLS="$TMP/calls" PROBES="$TMP/probes" READY_TIMEOUT=1 \
	PATH="$TMP/bin2:$PATH" NODE_REPO_DIR="$TMP/repo" OUT_DIR="$TMP/out2" \
	"$ROOT/scripts/bench/run-bench-segment.sh" >"$TMP/stdout2" 2>"$TMP/stderr2" || status=$?
[ "$status" -eq 1 ] || { echo "expected exit 1 (shard not ready), got $status" >&2; cat "$TMP/stderr2" >&2; exit 1; }
grep -Fq "compose -f $TMP/repo/docker/shard.yml -f $TMP/repo/docker/ci-ports.shard.yml -p soak-bench up -d" "$TMP/calls" || {
	echo "compose up does not use the ci-ports override" >&2
	cat "$TMP/calls" >&2
	exit 1
}
grep -Fq "http://localhost:14413/api/status" "$TMP/probes" || {
	echo "readiness probe does not use the remapped validator1 HTTP port 14413" >&2
	cat "$TMP/probes" >&2
	exit 1
}

printf 'bench segment failure diagnostics tests passed\n'

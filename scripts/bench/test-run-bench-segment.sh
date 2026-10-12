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

printf 'bench segment failure diagnostics tests passed\n'

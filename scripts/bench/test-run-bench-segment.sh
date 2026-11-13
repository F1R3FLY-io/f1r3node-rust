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

# Merged compose configuration that the fake docker prints for `config`.
# validator1 HTTP (container 40403) is published on 14999, not on the 14413 of
# the real override, so a passing probe check proves the script reads the port.
cat >"$TMP/config-ok.json" <<'JSON'
{"services": {
  "boot": {"ports": [{"target": 40403, "published": "14403"}]},
  "validator1": {"ports": [{"target": 40401, "published": "14411"}, {"target": 40403, "published": "14999"}]},
  "grafana": {"ports": [{"target": 3000, "published": "3000"}]}
}}
JSON
cat >"$TMP/config-ephemeral.json" <<'JSON'
{"services": {
  "validator1": {"ports": [{"target": 40403, "published": "14413"}]},
  "readonly": {"ports": [{"target": 40402, "published": "40452"}]}
}}
JSON

cat >"$TMP/bin/docker" <<'SH'
#!/usr/bin/env bash
case " $* " in
	*" config "*)
		cat "$CONFIG_JSON"
		exit 0
		;;
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
CONFIG_JSON="$TMP/config-ok.json" PATH="$TMP/bin:$PATH" NODE_REPO_DIR="$TMP/repo" OUT_DIR="$TMP/out" \
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
# The fake docker and curl record their arguments in the files that CALLS and
# PROBES name. The variables reach them through the environment of the run.
mkdir -p "$TMP/bin2" "$TMP/out2"
cat >"$TMP/bin2/docker" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$CALLS"
case " $* " in
	*" config "*) cat "$CONFIG_JSON" ;;
esac
exit 0
SH
cat >"$TMP/bin2/curl" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$PROBES"
exit 7
SH
chmod +x "$TMP/bin2/docker" "$TMP/bin2/curl"
status=0
CONFIG_JSON="$TMP/config-ok.json" CALLS="$TMP/calls" PROBES="$TMP/probes" READY_TIMEOUT=1 \
	PATH="$TMP/bin2:$PATH" NODE_REPO_DIR="$TMP/repo" OUT_DIR="$TMP/out2" \
	"$ROOT/scripts/bench/run-bench-segment.sh" >"$TMP/stdout2" 2>"$TMP/stderr2" || status=$?
[ "$status" -eq 1 ] || { echo "expected exit 1 (shard not ready), got $status" >&2; cat "$TMP/stderr2" >&2; exit 1; }
grep -Fq "compose -f $TMP/repo/docker/shard.yml -f $TMP/repo/docker/ci-ports.shard.yml -p soak-bench up -d" "$TMP/calls" || {
	echo "compose up does not use the ci-ports override" >&2
	cat "$TMP/calls" >&2
	exit 1
}
grep -Fq "http://localhost:14999/api/status" "$TMP/probes" || {
	echo "readiness probe does not use the validator1 HTTP port from the merged configuration" >&2
	cat "$TMP/probes" >&2
	exit 1
}

# A merged configuration that still publishes a port in the ephemeral range
# must stop the segment before compose up.
mkdir -p "$TMP/out3"
: >"$TMP/calls3"
status=0
CONFIG_JSON="$TMP/config-ephemeral.json" CALLS="$TMP/calls3" PROBES="$TMP/probes3" READY_TIMEOUT=1 \
	PATH="$TMP/bin2:$PATH" NODE_REPO_DIR="$TMP/repo" OUT_DIR="$TMP/out3" \
	"$ROOT/scripts/bench/run-bench-segment.sh" >"$TMP/stdout3" 2>"$TMP/stderr3" || status=$?
[ "$status" -eq 2 ] || { echo "expected exit 2 for an ephemeral host port, got $status" >&2; cat "$TMP/stderr3" >&2; exit 1; }
grep -Fq '40452' "$TMP/stderr3" || { echo "refusal does not name port 40452" >&2; cat "$TMP/stderr3" >&2; exit 1; }
if grep -Fq ' up ' "$TMP/calls3"; then
	echo "compose up ran although a host port is in the ephemeral range" >&2
	exit 1
fi

# A merged configuration without a validator1 HTTP port (the soak fixtures use
# a one-service shard) falls back to the default remapped port 14413 and says
# so on stderr. The readiness probe then shows whether the shard answers.
cat >"$TMP/config-no-validator1.json" <<'JSON'
{"services": {"writer": {}}}
JSON
mkdir -p "$TMP/out4"
status=0
CONFIG_JSON="$TMP/config-no-validator1.json" CALLS="$TMP/calls4" PROBES="$TMP/probes4" READY_TIMEOUT=1 \
	PATH="$TMP/bin2:$PATH" NODE_REPO_DIR="$TMP/repo" OUT_DIR="$TMP/out4" \
	"$ROOT/scripts/bench/run-bench-segment.sh" >"$TMP/stdout4" 2>"$TMP/stderr4" || status=$?
[ "$status" -eq 1 ] || { echo "expected exit 1 (shard not ready) without a validator1 port, got $status" >&2; cat "$TMP/stderr4" >&2; exit 1; }
grep -Fq "http://localhost:14413/api/status" "$TMP/probes4" || {
	echo "probe does not fall back to 14413 without a validator1 port" >&2
	cat "$TMP/probes4" >&2
	exit 1
}
grep -Fq 'no host port for validator1 HTTP' "$TMP/stderr4" || { echo "fallback is not reported on stderr" >&2; cat "$TMP/stderr4" >&2; exit 1; }

# docker/.env is not tracked (#745). A checkout with only the development
# template must start the shard from it and take the deployer key from it. A
# missing deployer key exits 2 before compose, so reaching compose up shows
# that the key came from the template.
mkdir -p "$TMP/repo5/docker" "$TMP/out5"
: >"$TMP/repo5/docker/shard.yml"
: >"$TMP/repo5/docker/ci-ports.shard.yml"
printf 'BOOTSTRAP_PRIVATE_KEY=fixture-example\n' >"$TMP/repo5/docker/.env.example"
status=0
env -u DEPLOYER_KEY CONFIG_JSON="$TMP/config-ok.json" CALLS="$TMP/calls5" PROBES="$TMP/probes5" READY_TIMEOUT=1 \
	PATH="$TMP/bin2:$PATH" NODE_REPO_DIR="$TMP/repo5" OUT_DIR="$TMP/out5" \
	"$ROOT/scripts/bench/run-bench-segment.sh" >"$TMP/stdout5" 2>"$TMP/stderr5" || status=$?
[ "$status" -eq 1 ] || { echo "expected exit 1 (shard not ready) with only docker/.env.example, got $status" >&2; cat "$TMP/stderr5" >&2; exit 1; }
grep -Fq "compose --env-file $TMP/repo5/docker/.env.example -f $TMP/repo5/docker/shard.yml -f $TMP/repo5/docker/ci-ports.shard.yml -p soak-bench up -d" "$TMP/calls5" || {
	echo "compose up does not read the shard keys from docker/.env.example" >&2
	cat "$TMP/calls5" >&2
	exit 1
}

# Without docker/.env, docker/.env.example and DEPLOYER_KEY, the segment
# stops before compose and names the env file it read.
mkdir -p "$TMP/repo6/docker" "$TMP/out6"
: >"$TMP/repo6/docker/shard.yml"
: >"$TMP/repo6/docker/ci-ports.shard.yml"
: >"$TMP/calls6"
status=0
env -u DEPLOYER_KEY CONFIG_JSON="$TMP/config-ok.json" CALLS="$TMP/calls6" PROBES="$TMP/probes6" READY_TIMEOUT=1 \
	PATH="$TMP/bin2:$PATH" NODE_REPO_DIR="$TMP/repo6" OUT_DIR="$TMP/out6" \
	"$ROOT/scripts/bench/run-bench-segment.sh" >"$TMP/stdout6" 2>"$TMP/stderr6" || status=$?
[ "$status" -eq 2 ] || { echo "expected exit 2 without a deployer key, got $status" >&2; cat "$TMP/stderr6" >&2; exit 1; }
grep -Fq "no BOOTSTRAP_PRIVATE_KEY in $TMP/repo6/docker/.env" "$TMP/stderr6" || { echo "missing deployer key is not reported" >&2; cat "$TMP/stderr6" >&2; exit 1; }
[ ! -s "$TMP/calls6" ] || { echo "compose ran without a deployer key" >&2; cat "$TMP/calls6" >&2; exit 1; }

# The real compose files must merge to host ports below the ephemeral floor.
# This needs docker compose. CI runs this test where Docker exists, so a
# missing docker compose fails in CI and only skips locally. The shard keys
# come from docker/.env.example, because docker/.env is not tracked (#745).
if docker compose version >/dev/null 2>&1; then
	docker compose --env-file "$ROOT/docker/.env.example" -f "$ROOT/docker/shard.yml" -f "$ROOT/docker/ci-ports.shard.yml" -p soak-bench \
		config --format json >"$TMP/real-config.json"
	jq -e '
	  ([.services[].ports[]? | .published | tonumber] | length) > 0
	  and ([.services[].ports[]? | .published | tonumber | select(. >= 32768)] | length) == 0
	  and ([.services.validator1.ports[]? | select(.target == 40403)] | length) == 1
	' "$TMP/real-config.json" >/dev/null || {
		echo "the real merged compose configuration publishes a host port at or above 32768, or no validator1 HTTP port" >&2
		jq -c '[.services | to_entries[] | {(.key): [.value.ports[]? | .published]}]' "$TMP/real-config.json" >&2
		exit 1
	}
elif [ -n "${CI:-}" ]; then
	echo "docker compose is not available; the merged port check cannot run in CI" >&2
	exit 1
else
	echo "skipped the real merged port check: docker compose is not available" >&2
fi

printf 'bench segment failure diagnostics tests passed\n'

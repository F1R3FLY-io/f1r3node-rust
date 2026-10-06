#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin" "$TMP/out" "$TMP/state"

cat >"$TMP/bin/docker" <<'SH'
#!/usr/bin/env bash
if [ "$1" = cp ]; then
	exit 0
fi
if [ "$1" = exec ]; then
	shift 2
	if printf '%s\n' "$*" | grep -q 'show-blocks'; then
		if [ ! -e "$MOCK_STATE_DIR/preflight-attempted" ]; then
			touch "$MOCK_STATE_DIR/preflight-attempted"
			exit 1
		fi
		touch "$MOCK_STATE_DIR/casper-ready"
		printf 'count: 1\n'
		exit 0
	fi
	if printf '%s\n' "$*" | grep -q ' deploy '; then
		[ -e "$MOCK_STATE_DIR/casper-ready" ] || exit 1
		printf 'DeployId is: deadbeef\n'
		exit 0
	fi
fi
if [ "$1" = logs ]; then
	exit 0
fi
exit 0
SH
cat >"$TMP/bin/curl" <<'SH'
#!/usr/bin/env bash
case "$*" in
	*'/api/status'*) printf '{"peers":1,"nodes":1}\n' ;;
	*'/api/deploy-finalization-status/'*)
		if [ ! -e "$MOCK_STATE_DIR/finalization-polled" ]; then
			touch "$MOCK_STATE_DIR/finalization-polled"
			printf '{"state":"Pending","latest_block_hash":null}\n'
		else
			printf '{"state":"Finalized","latest_block_hash":"abc123"}\n'
		fi
		;;
	*) exit 1 ;;
esac
SH
chmod +x "$TMP/bin/docker" "$TMP/bin/curl"

PATH="$TMP/bin:$PATH" \
	MOCK_STATE_DIR="$TMP/state" \
	DEPLOYER_KEY=test \
	DURATION=1 \
	DEPLOYS_PER_SEC=1 \
	POLL_INTERVAL=0.1 \
	PREFLIGHT_TIMEOUT=2 \
	OUT_DIR="$TMP/out" \
	bash "$ROOT/scripts/bench/latency-benchmark.sh" \
	--apply --duration 1 --rate 1 --out-dir "$TMP/out" >/dev/null 2>&1

jq -e '
  .submitted == 1
  and .submit_errors == 0
  and .finalized == 1
  and .finalization_rate == 1
  and .latency.samples == 1
' "$TMP/out/metrics.json" >/dev/null

printf 'latency benchmark readiness and finalization tests passed\n'

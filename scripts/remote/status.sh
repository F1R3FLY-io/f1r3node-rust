#!/usr/bin/env bash
# Quick health check of the distributed shard across both VPSes.
# Hits HTTP /api/status and /metrics on each node and prints a compact
# per-node summary. Exits non-zero if any node is unhealthy.
#
# Usage:
#   status.sh              # check all 4 nodes (default)
#   status.sh vps1         # check bootstrap only
#   status.sh vps2         # check followers only

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=oci-common.sh
source "${SCRIPT_DIR}/oci-common.sh"

usage() {
  cat <<EOF
Usage: $(basename "$0") [TARGET]

TARGET:
  both      Check all 4 nodes (default)
  vps1      Check bootstrap only
  vps2      Check followers only (validator1, validator2, observer)

Environment:
  CURL_TIMEOUT  Per-request timeout in seconds (default: 3)
EOF
}

TARGET="${1:-both}"
case "$TARGET" in
  both|vps1|vps2) ;;
  -h|--help) usage; exit 0 ;;
  *) err "Unknown target: $TARGET"; usage; exit 2 ;;
esac

require_cmd curl
require_cmd jq

[[ -f "$STATE_FILE" ]] || die "No testbed state at $STATE_FILE. Run oci-provision.sh --apply first."

VPS1_IP="$(state_get vps1_public_ip)"
VPS2_IP="$(state_get vps2_public_ip)"
[[ -n "$VPS1_IP" && -n "$VPS2_IP" ]] || die "VPS public IPs missing from state."

CURL_TIMEOUT="${CURL_TIMEOUT:-3}"
FAIL_COUNT=0

# check_node <label> <host> <http-port>
check_node() {
  local label="$1" host="$2" port="$3"
  local status_url="http://${host}:${port}/api/status"
  local metrics_url="http://${host}:${port}/metrics"

  printf "%-12s %-20s  " "$label" "${host}:${port}"

  local status_json http_code metrics_code
  if status_json="$(curl -fsS --max-time "$CURL_TIMEOUT" "$status_url" 2>/dev/null)"; then
    local peers nodes
    peers="$(echo "$status_json" | jq -r '.peers // "?"')"
    nodes="$(echo "$status_json" | jq -r '.nodes // "?"')"
    http_code="ok (peers=${peers} nodes=${nodes})"
  else
    http_code="UNREACHABLE"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi

  metrics_code="$(curl -s -o /dev/null -w "%{http_code}" --max-time "$CURL_TIMEOUT" "$metrics_url" 2>/dev/null || echo "000")"

  printf "status=%-40s metrics=%s\n" "$http_code" "$metrics_code"
}

echo "=== Shard Status ==="
if [[ "$TARGET" == "both" || "$TARGET" == "vps1" ]]; then
  check_node "bootstrap" "$VPS1_IP" 40403
fi
if [[ "$TARGET" == "both" || "$TARGET" == "vps2" ]]; then
  check_node "validator1" "$VPS2_IP" 40413
  check_node "validator2" "$VPS2_IP" 40423
  check_node "observer"   "$VPS2_IP" 40453
fi

if (( FAIL_COUNT > 0 )); then
  err "${FAIL_COUNT} node(s) unhealthy"
  exit 1
fi
info "All checked nodes responded"

#!/usr/bin/env bash
# Graceful shutdown of the distributed shard on both VPSes.
# Runs `docker compose down -v` remotely so volumes are wiped before the
# OCI instances are terminated by oci-destroy.sh. Independent of oci-destroy
# so you can stop the shard without tearing down the VMs (e.g. for a quick
# redeploy).
#
# Default mode is DRY-RUN. Pass --apply to actually stop containers.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=oci-common.sh
source "${SCRIPT_DIR}/oci-common.sh"

REMOTE_DOCKER_DIR="/home/opc/docker"
SSH_USER="${SSH_USER:-opc}"
SSH_OPTS=(-i "$KEY_FILE" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR)

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --apply           Actually stop containers on both VPSes (default dry-run)
  --dry-run         Print planned commands without executing (default)
  -h, --help        Show this help

Does NOT terminate OCI VPSes. Use oci-destroy.sh for that.
EOF
}

APPLY=0
for arg in "$@"; do
  case "$arg" in
    --apply)   APPLY=1 ;;
    --dry-run) APPLY=0 ;;
    -h|--help) usage; exit 0 ;;
    *) err "Unknown argument: $arg"; usage; exit 2 ;;
  esac
done
export APPLY

preflight
[[ -f "$STATE_FILE" ]] || die "No testbed state at $STATE_FILE; nothing to tear down."
[[ -f "$KEY_FILE"   ]] || die "No SSH key at $KEY_FILE."

VPS1_IP="$(state_get vps1_public_ip)"
VPS2_IP="$(state_get vps2_public_ip)"
[[ -n "$VPS1_IP" && -n "$VPS2_IP" ]] || die "VPS public IPs missing from state."

compose_down() {
  local ip="$1" compose_file="$2" label="$3"
  info "[${label}] docker compose -f ${compose_file} down -v"
  if [[ "$APPLY" == "1" ]]; then
    ssh "${SSH_OPTS[@]}" "${SSH_USER}@${ip}" \
      "cd ${REMOTE_DOCKER_DIR} && docker compose --env-file .env.remote -f ${compose_file} down -v" || \
      warn "[${label}] compose down failed (container may already be stopped)"
  else
    echo "DRY-RUN [${label}]: ssh ${SSH_USER}@${ip} 'docker compose --env-file .env.remote -f ${compose_file} down -v'" >&2
  fi
}

compose_down "$VPS2_IP" shard.vps2.yml vps2
compose_down "$VPS1_IP" shard.vps1.yml vps1

info "=== Teardown Summary ==="
if [[ "$APPLY" == "1" ]]; then
  info "Containers stopped and volumes wiped on both VPSes."
  info "OCI VPSes still running. Use oci-destroy.sh --apply to terminate them."
else
  info "DRY-RUN complete. Re-run with --apply to actually stop containers."
fi

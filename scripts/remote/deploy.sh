#!/usr/bin/env bash
# Deploy the F1R3FLY distributed shard to the OCI testbed.
#
# Steps:
#   1. Render docker/.env.remote from .env.remote.template using VPS IPs
#      from testbed-state.json
#   2. scp the docker/ tree (conf, genesis, certs, compose files) to both
#      VPSes in parallel
#   3. Start the bootstrap on VPS-1 (docker compose up -d)
#   4. Wait for bootstrap HTTP /api/status to respond
#   5. Start the followers on VPS-2 (docker compose up -d)
#
# Prerequisites:
#   - testbed-state.json from oci-provision.sh --apply
#   - testbed.pem SSH key
#   - Docker image already loaded on both VPSes via image-transfer.sh
#
# Default mode is DRY-RUN. Pass --apply to actually deploy.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=oci-common.sh
source "${SCRIPT_DIR}/oci-common.sh"

REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
DOCKER_DIR="${REPO_ROOT}/docker"
ENV_TEMPLATE="${DOCKER_DIR}/.env.remote.template"
ENV_FILE="${DOCKER_DIR}/.env.remote"
ENV_KEYS_FILE="${DOCKER_DIR}/.env"
REMOTE_DOCKER_DIR="/home/opc/docker"
SSH_USER="${SSH_USER:-opc}"
SSH_OPTS=(-i "$KEY_FILE" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR)

# Bootstrap readiness polling
HEALTH_TIMEOUT="${HEALTH_TIMEOUT:-180}"
HEALTH_INTERVAL="${HEALTH_INTERVAL:-5}"

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --apply           Actually deploy to the testbed (default is dry-run)
  --dry-run         Print planned commands without executing (default)
  -h, --help        Show this help

Reads VPS IPs from $STATE_FILE.
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
require_cmd scp
require_cmd ssh
require_cmd sed
require_cmd curl

[[ -f "$STATE_FILE"    ]] || die "No testbed state at $STATE_FILE. Run oci-provision.sh --apply first."
[[ -f "$KEY_FILE"      ]] || die "No SSH key at $KEY_FILE. Run oci-provision.sh --apply first."
[[ -f "$ENV_TEMPLATE"  ]] || die "Missing env template: $ENV_TEMPLATE"
[[ -f "${DOCKER_DIR}/shard.vps1.yml" ]] || die "Missing compose file: docker/shard.vps1.yml"
[[ -f "${DOCKER_DIR}/shard.vps2.yml" ]] || die "Missing compose file: docker/shard.vps2.yml"

VPS1_IP="$(state_get vps1_public_ip)"
VPS2_IP="$(state_get vps2_public_ip)"
[[ -n "$VPS1_IP" && -n "$VPS2_IP" ]] || die "VPS public IPs missing from state."

info "VPS-1 (bootstrap): ${SSH_USER}@${VPS1_IP}"
info "VPS-2 (followers): ${SSH_USER}@${VPS2_IP}"

if [[ "$APPLY" == "1" ]]; then
  warn "APPLY mode: will render env, scp, and start containers on both VPSes."
else
  info "DRY-RUN mode (default). Use --apply to actually deploy."
fi

# --- 1. Render .env.remote ---
# Reads key material from $ENV_KEYS_FILE (gitignored docker/.env) so the
# committed template never carries key values. Required keys:
#   BOOTSTRAP_NODE_ID, BOOTSTRAP_PUBLIC_KEY, BOOTSTRAP_PRIVATE_KEY,
#   VALIDATOR{1,2}_PUBLIC_KEY, VALIDATOR{1,2}_PRIVATE_KEY.
render_env() {
  info "Rendering .env.remote from template (VPS1=$VPS1_IP, VPS2=$VPS2_IP)"
  [[ -f "$ENV_KEYS_FILE" ]] || die "Missing $ENV_KEYS_FILE (source of key material). Populate it before running deploy."

  local keys=(
    BOOTSTRAP_NODE_ID BOOTSTRAP_PUBLIC_KEY BOOTSTRAP_PRIVATE_KEY
    VALIDATOR1_PUBLIC_KEY VALIDATOR1_PRIVATE_KEY
    VALIDATOR2_PUBLIC_KEY VALIDATOR2_PRIVATE_KEY
  )
  local sed_args=(-e "s|__VPS1_PUBLIC_HOST__|${VPS1_IP}|g" -e "s|__VPS2_PUBLIC_HOST__|${VPS2_IP}|g")
  local k val
  for k in "${keys[@]}"; do
    val=$(grep -E "^${k}=" "$ENV_KEYS_FILE" | head -n1 | cut -d= -f2-)
    [[ -n "$val" ]] || die "$ENV_KEYS_FILE is missing required entry: ${k}="
    sed_args+=(-e "s|__${k}__|${val}|g")
  done

  if [[ "$APPLY" == "1" ]]; then
    sed "${sed_args[@]}" "$ENV_TEMPLATE" > "$ENV_FILE"
    chmod 600 "$ENV_FILE"
  else
    echo "DRY-RUN: render $ENV_TEMPLATE -> $ENV_FILE (sub VPS hosts + ${#keys[@]} key tokens from $ENV_KEYS_FILE)" >&2
  fi
}

# --- 2. scp docker/ subset to a VPS ---
# Ships everything compose needs: conf, genesis, certs, the two shard.vps*.yml
# files, and the rendered .env.remote. Excludes data/, helm/, minikube/,
# resources/, monitoring/, prometheus-grafana.md, README.md, standalone.yml,
# observer.yml, validator4.yml, shard.yml — none of those run on the testbed.
transfer_to_vps() {
  local ip="$1" label="$2"
  info "[${label}] preparing ${REMOTE_DOCKER_DIR} on ${ip}"
  if [[ "$APPLY" == "1" ]]; then
    ssh "${SSH_OPTS[@]}" "${SSH_USER}@${ip}" "mkdir -p ${REMOTE_DOCKER_DIR}"
    scp -r "${SSH_OPTS[@]}" \
      "${DOCKER_DIR}/conf" \
      "${DOCKER_DIR}/genesis" \
      "${DOCKER_DIR}/certs" \
      "${DOCKER_DIR}/shard.vps1.yml" \
      "${DOCKER_DIR}/shard.vps2.yml" \
      "$ENV_FILE" \
      "${SSH_USER}@${ip}:${REMOTE_DOCKER_DIR}/"
    info "[${label}] transfer complete"
  else
    echo "DRY-RUN [${label}]: ssh ${SSH_USER}@${ip} mkdir -p ${REMOTE_DOCKER_DIR}" >&2
    echo "DRY-RUN [${label}]: scp -r docker/{conf,genesis,certs,shard.vps1.yml,shard.vps2.yml,.env.remote} ${SSH_USER}@${ip}:${REMOTE_DOCKER_DIR}/" >&2
  fi
}

# --- 3. Start a compose stack on a VPS ---
compose_up() {
  local ip="$1" compose_file="$2" label="$3"
  info "[${label}] docker compose -f ${compose_file} up -d"
  if [[ "$APPLY" == "1" ]]; then
    ssh "${SSH_OPTS[@]}" "${SSH_USER}@${ip}" \
      "cd ${REMOTE_DOCKER_DIR} && docker compose --env-file .env.remote -f ${compose_file} up -d"
  else
    echo "DRY-RUN [${label}]: ssh ${SSH_USER}@${ip} 'cd ${REMOTE_DOCKER_DIR} && docker compose --env-file .env.remote -f ${compose_file} up -d'" >&2
  fi
}

# --- 4. Wait for bootstrap HTTP /api/status ---
wait_for_bootstrap() {
  info "Waiting for bootstrap /api/status on http://${VPS1_IP}:40403/api/status (timeout ${HEALTH_TIMEOUT}s)"
  if [[ "$APPLY" != "1" ]]; then
    echo "DRY-RUN: poll http://${VPS1_IP}:40403/api/status" >&2
    return
  fi
  local deadline
  deadline=$(( $(date +%s) + HEALTH_TIMEOUT ))
  while true; do
    if curl -fsS --max-time 3 "http://${VPS1_IP}:40403/api/status" >/dev/null 2>&1; then
      info "Bootstrap is healthy"
      return
    fi
    (( $(date +%s) >= deadline )) && die "Bootstrap failed to become healthy in ${HEALTH_TIMEOUT}s"
    sleep "$HEALTH_INTERVAL"
  done
}

# --- Main ---
render_env

if [[ "$APPLY" == "1" ]]; then
  info "Starting parallel transfer to both VPSes"
  transfer_to_vps "$VPS1_IP" vps1 &
  PID1=$!
  transfer_to_vps "$VPS2_IP" vps2 &
  PID2=$!
  wait "$PID1" || die "vps1 transfer failed"
  wait "$PID2" || die "vps2 transfer failed"
else
  transfer_to_vps "$VPS1_IP" vps1
  transfer_to_vps "$VPS2_IP" vps2
fi

compose_up "$VPS1_IP" shard.vps1.yml vps1
wait_for_bootstrap
compose_up "$VPS2_IP" shard.vps2.yml vps2

info "=== Deploy Summary ==="
if [[ "$APPLY" == "1" ]]; then
  info "Bootstrap:   http://${VPS1_IP}:40403/api/status"
  info "Validator1:  http://${VPS2_IP}:40413/api/status"
  info "Validator2:  http://${VPS2_IP}:40423/api/status"
  info "Observer:    http://${VPS2_IP}:40453/api/status"
  info "Check shard health with: just vps-status"
else
  info "DRY-RUN complete. Re-run with --apply to actually deploy."
fi

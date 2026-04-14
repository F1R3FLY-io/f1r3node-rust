#!/usr/bin/env bash
# Transfer a Docker image from the local daemon to both testbed VPSes
# in parallel via docker save -> scp -> docker load.
#
# Reads VPS public IPs from testbed-state.json (produced by oci-provision.sh).
# SSH identity is the dedicated keypair at scripts/remote/testbed.pem.
#
# Default mode is DRY-RUN. Pass --apply to actually transfer.
#
# Migration note: once the CI pipeline publishes to OCIR on master push,
# replace this script with `docker pull <OCIR-URL>` on each VPS. Keep as
# a fallback for air-gapped testbeds or local-build smoke-tests.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=oci-common.sh
source "${SCRIPT_DIR}/oci-common.sh"

DEFAULT_IMAGE="sjc.ocir.io/axd0qezqa9z3/f1r3fly-rust:latest"
SSH_USER="${SSH_USER:-opc}"
SSH_OPTS=(-i "$KEY_FILE" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR)
REMOTE_TAR="/tmp/f1r3fly-image.tar.gz"

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS] [IMAGE]

Transfers a Docker image from the local daemon to both testbed VPSes.

Arguments:
  IMAGE             Docker image name:tag to transfer (default: $DEFAULT_IMAGE)

Options:
  --apply           Actually perform the transfer (default is dry-run)
  --dry-run         Print planned commands without executing (default)
  -h, --help        Show this help

Prerequisites:
  - testbed-state.json exists (run oci-provision.sh --apply first)
  - testbed.pem SSH key exists
  - Local Docker daemon has the image
  - Docker installed on VPSes (handled by cloud-init.yaml)

Examples:
  # Transfer the default OCIR-tagged image
  $(basename "$0") --apply

  # Transfer a locally built image (remember to override F1R3FLY_IMAGE on VPS)
  $(basename "$0") --apply f1r3fly-rust:local

  # Tag a local build as the OCIR URL first, then transfer (compose defaults work)
  docker tag f1r3fly-rust:local $DEFAULT_IMAGE
  $(basename "$0") --apply
EOF
}

APPLY=0
IMAGE="$DEFAULT_IMAGE"
for arg in "$@"; do
  case "$arg" in
    --apply)   APPLY=1 ;;
    --dry-run) APPLY=0 ;;
    -h|--help) usage; exit 0 ;;
    -*)        err "Unknown option: $arg"; usage; exit 2 ;;
    *)         IMAGE="$arg" ;;
  esac
done

preflight
require_cmd docker
require_cmd scp
require_cmd ssh

[[ -f "$STATE_FILE" ]] || die "No testbed state at $STATE_FILE. Run oci-provision.sh --apply first."
[[ -f "$KEY_FILE"   ]] || die "No SSH key at $KEY_FILE. Run oci-provision.sh --apply first."

VPS1_IP="$(state_get vps1_public_ip)"
VPS2_IP="$(state_get vps2_public_ip)"
[[ -n "$VPS1_IP" && -n "$VPS2_IP" ]] || die "VPS public IPs missing from state. Re-run oci-provision.sh."

info "Image:  $IMAGE"
info "VPS-1:  ${SSH_USER}@${VPS1_IP}"
info "VPS-2:  ${SSH_USER}@${VPS2_IP}"

if [[ "$APPLY" == "1" ]]; then
  warn "APPLY mode: will save, scp, and load the image on both VPSes."
else
  info "DRY-RUN mode (default). Use --apply to actually transfer."
fi

# Verify the image exists locally.
if [[ "$APPLY" == "1" ]]; then
  docker image inspect "$IMAGE" >/dev/null 2>&1 \
    || die "Image '$IMAGE' not found in local Docker daemon. Build or pull it first."
fi

LOCAL_TAR="$(mktemp -t f1r3fly-image-XXXXXX.tar.gz)"
trap 'rm -f "$LOCAL_TAR"' EXIT

# --- 1. docker save ---
info "Saving image to $LOCAL_TAR"
if [[ "$APPLY" == "1" ]]; then
  docker save "$IMAGE" | gzip > "$LOCAL_TAR"
  local_size="$(du -h "$LOCAL_TAR" | cut -f1)"
  info "Image tarball: ${local_size}"
else
  echo "DRY-RUN: docker save $IMAGE | gzip > $LOCAL_TAR" >&2
fi

# --- 2 & 3. scp + load (parallel per VPS) ---
transfer_one() {
  local ip="$1" label="$2"
  if [[ "$APPLY" == "1" ]]; then
    info "[${label}] scp -> ${ip}:${REMOTE_TAR}"
    scp "${SSH_OPTS[@]}" "$LOCAL_TAR" "${SSH_USER}@${ip}:${REMOTE_TAR}"
    info "[${label}] docker load on ${ip}"
    ssh "${SSH_OPTS[@]}" "${SSH_USER}@${ip}" \
      "gunzip -c ${REMOTE_TAR} | docker load && rm -f ${REMOTE_TAR}"
    info "[${label}] done"
  else
    echo "DRY-RUN [${label}]: scp ${LOCAL_TAR} ${SSH_USER}@${ip}:${REMOTE_TAR}" >&2
    echo "DRY-RUN [${label}]: ssh ${SSH_USER}@${ip} 'gunzip -c ${REMOTE_TAR} | docker load && rm -f ${REMOTE_TAR}'" >&2
  fi
}

if [[ "$APPLY" == "1" ]]; then
  info "Starting parallel transfer to both VPSes"
  transfer_one "$VPS1_IP" vps1 &
  PID1=$!
  transfer_one "$VPS2_IP" vps2 &
  PID2=$!
  wait "$PID1" || die "vps1 transfer failed"
  wait "$PID2" || die "vps2 transfer failed"
else
  transfer_one "$VPS1_IP" vps1
  transfer_one "$VPS2_IP" vps2
fi

# --- 4. Summary ---
info "=== Image Transfer Summary ==="
if [[ "$APPLY" == "1" ]]; then
  info "Image '$IMAGE' now loaded on both VPSes."
  info "Verify remotely: ssh -i $KEY_FILE ${SSH_USER}@${VPS1_IP} 'docker image inspect $IMAGE >/dev/null && echo OK'"
else
  info "DRY-RUN complete. No files transferred."
  info "Re-run with --apply to actually transfer."
fi

#!/usr/bin/env bash
# Shared configuration and helper functions for OCI testbed scripts.
# Sourced by oci-provision.sh and oci-destroy.sh.
set -euo pipefail

# --- Configuration (overridable via env) ---
: "${OCI_REGION:=us-sanjose-1}"
: "${OCI_COMPARTMENT_ID:=ocid1.compartment.oc1..aaaaaaaagxeazaquqkvniko2zq5m7i7mxa37fz5u6gyjny4svupgwh4ao3fa}"
: "${OCI_AD:=fnZP:US-SANJOSE-1-AD-1}"

: "${TESTBED_NAME:=f1r3node-rust-testbed}"
: "${VCN_NAME:=${TESTBED_NAME}-vcn}"
: "${IGW_NAME:=${TESTBED_NAME}-igw}"
: "${SECLIST_NAME:=${TESTBED_NAME}-seclist}"
: "${SUBNET_NAME:=${TESTBED_NAME}-subnet}"
: "${VPS1_NAME:=${TESTBED_NAME}-vps1-bootstrap}"
: "${VPS2_NAME:=${TESTBED_NAME}-vps2-followers}"

: "${VCN_CIDR:=10.1.0.0/16}"
: "${SUBNET_CIDR:=10.1.1.0/24}"

: "${SHAPE:=VM.Standard.A1.Flex}"
: "${VPS1_OCPUS:=2}"
: "${VPS1_MEMORY_GB:=4}"
: "${VPS2_OCPUS:=4}"
: "${VPS2_MEMORY_GB:=8}"

# --- Paths ---
REMOTE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STATE_FILE="${STATE_FILE:-${REMOTE_DIR}/testbed-state.json}"
KEY_FILE="${KEY_FILE:-${REMOTE_DIR}/testbed.pem}"

# --- Logging ---
log()  { echo "[$(date +%H:%M:%S)] $*" >&2; }
info() { log "[info] $*"; }
warn() { log "[warn] $*"; }
err()  { log "[err ] $*"; }
die()  { err "$@"; exit 1; }

# --- Preflight ---
require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "Missing required command: $1"
}

preflight() {
  require_cmd oci
  require_cmd jq
  require_cmd ssh-keygen
}

# --- State file ---
state_init() {
  [[ -f "$STATE_FILE" ]] || echo '{}' > "$STATE_FILE"
}

state_get() {
  jq -r --arg k "$1" '.[$k] // empty' "$STATE_FILE"
}

state_set() {
  local key="$1" val="$2" tmp
  tmp="$(mktemp)"
  jq --arg k "$key" --arg v "$val" '.[$k]=$v' "$STATE_FILE" > "$tmp"
  mv "$tmp" "$STATE_FILE"
}

state_del() {
  local key="$1" tmp
  tmp="$(mktemp)"
  jq --arg k "$key" 'del(.[$k])' "$STATE_FILE" > "$tmp"
  mv "$tmp" "$STATE_FILE"
}

# --- Dry-run wrapper ---
# In dry-run, prints the command but skips execution. Returns empty string
# on stdout so callers using command substitution don't capture garbage.
# Real execution returns the actual OCI CLI output.
: "${APPLY:=0}"

oci_run() {
  if [[ "$APPLY" == "1" ]]; then
    oci "$@"
  else
    echo "DRY-RUN: oci $*" >&2
  fi
}

# --- Image lookup: latest Oracle Linux 9 aarch64 for the shape ---
latest_ol9_arm_image() {
  oci compute image list \
    --compartment-id "$OCI_COMPARTMENT_ID" \
    --operating-system "Oracle Linux" \
    --operating-system-version "9" \
    --shape "$SHAPE" \
    --sort-by TIMECREATED --sort-order DESC --limit 1 \
    --query 'data[0].id' --raw-output
}

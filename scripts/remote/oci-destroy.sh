#!/usr/bin/env bash
# Tear down the F1R3FLY distributed testbed on OCI.
# Reverses oci-provision.sh in dependency order.
#
# Default mode is DRY-RUN. Pass --apply to actually terminate resources.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=oci-common.sh
source "${SCRIPT_DIR}/oci-common.sh"

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --apply           Actually terminate OCI resources (default is dry-run)
  --dry-run         Print planned commands without executing (default)
  --force           Skip the name confirmation prompt (use with --apply)
  -h, --help        Show this help

Reads state from: $STATE_FILE
EOF
}

APPLY=0
FORCE=0
for arg in "$@"; do
  case "$arg" in
    --apply)   APPLY=1 ;;
    --dry-run) APPLY=0 ;;
    --force)   FORCE=1 ;;
    -h|--help) usage; exit 0 ;;
    *) err "Unknown argument: $arg"; usage; exit 2 ;;
  esac
done
export APPLY

preflight

if [[ ! -f "$STATE_FILE" ]]; then
  die "No state file at $STATE_FILE — nothing to destroy."
fi

if [[ "$APPLY" == "1" && "$FORCE" == "0" ]]; then
  warn "APPLY mode: real OCI resources WILL be terminated."
  echo -n "Type the VCN name ($VCN_NAME) to confirm: "
  read -r confirm
  [[ "$confirm" == "$VCN_NAME" ]] || die "Confirmation did not match. Aborting."
fi

# --- Terminate instances ---
terminate_instance() {
  local role="$1"
  local iid
  iid="$(state_get "${role}_instance_id")"
  if [[ -z "$iid" ]]; then
    info "No $role instance to terminate"
    return
  fi
  info "Terminating $role instance: $iid"
  if [[ "$APPLY" == "1" ]]; then
    oci compute instance terminate --instance-id "$iid" --force \
      --wait-for-state TERMINATED >/dev/null
    state_del "${role}_instance_id"
    state_del "${role}_public_ip"
  else
    oci_run compute instance terminate --instance-id "$iid" --force
  fi
}

# --- Delete subnet ---
delete_subnet() {
  local sid
  sid="$(state_get subnet_id)"
  [[ -n "$sid" ]] || { info "No subnet to delete"; return; }
  info "Deleting subnet: $sid"
  if [[ "$APPLY" == "1" ]]; then
    oci network subnet delete --subnet-id "$sid" --force \
      --wait-for-state TERMINATED >/dev/null
    state_del subnet_id
  else
    oci_run network subnet delete --subnet-id "$sid" --force
  fi
}

# --- Delete security list ---
delete_seclist() {
  local slid
  slid="$(state_get seclist_id)"
  [[ -n "$slid" ]] || { info "No security list to delete"; return; }
  info "Deleting security list: $slid"
  if [[ "$APPLY" == "1" ]]; then
    oci network security-list delete --security-list-id "$slid" --force \
      --wait-for-state TERMINATED >/dev/null
    state_del seclist_id
  else
    oci_run network security-list delete --security-list-id "$slid" --force
  fi
}

# --- Reset default route table (remove the 0.0.0.0/0 rule we added) ---
reset_route_table() {
  local rt_id
  rt_id="$(state_get route_table_id)"
  [[ -n "$rt_id" ]] || { info "No route table update to reset"; return; }
  info "Clearing default route table rules: $rt_id"
  if [[ "$APPLY" == "1" ]]; then
    oci network route-table update --rt-id "$rt_id" --force \
      --route-rules '[]' --wait-for-state AVAILABLE >/dev/null
    state_del route_table_id
    state_del route_updated
  else
    oci_run network route-table update --rt-id "$rt_id" --route-rules '[]'
  fi
}

# --- Delete IGW ---
delete_igw() {
  local iid
  iid="$(state_get igw_id)"
  [[ -n "$iid" ]] || { info "No IGW to delete"; return; }
  info "Deleting Internet Gateway: $iid"
  if [[ "$APPLY" == "1" ]]; then
    oci network internet-gateway delete --ig-id "$iid" --force \
      --wait-for-state TERMINATED >/dev/null
    state_del igw_id
  else
    oci_run network internet-gateway delete --ig-id "$iid" --force
  fi
}

# --- Delete VCN ---
delete_vcn() {
  local vid
  vid="$(state_get vcn_id)"
  [[ -n "$vid" ]] || { info "No VCN to delete"; return; }
  info "Deleting VCN: $vid"
  if [[ "$APPLY" == "1" ]]; then
    oci network vcn delete --vcn-id "$vid" --force \
      --wait-for-state TERMINATED >/dev/null
    state_del vcn_id
  else
    oci_run network vcn delete --vcn-id "$vid" --force
  fi
}

print_summary() {
  info "=== Testbed Teardown Summary ==="
  if [[ "$APPLY" == "1" ]]; then
    info "Remaining state (should be empty):"
    jq . "$STATE_FILE"
    info "Local SSH keypair at $KEY_FILE is preserved. Delete manually if desired."
  else
    info "DRY-RUN complete. No resources were terminated."
    info "Re-run with --apply to actually tear down the testbed."
  fi
}

# --- Main (reverse order) ---
terminate_instance vps2
terminate_instance vps1
delete_subnet
delete_seclist
reset_route_table
delete_igw
delete_vcn
print_summary

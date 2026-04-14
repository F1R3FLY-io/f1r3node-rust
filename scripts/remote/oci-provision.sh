#!/usr/bin/env bash
# Provision the F1R3FLY distributed testbed on OCI.
#
# Creates (in order): VCN, internet gateway, default route table update,
# security list (public F1R3FLY ports), subnet, SSH keypair, 2 VMs.
# Resource OCIDs are persisted to testbed-state.json so teardown can
# reverse cleanly. Idempotent: re-running skips already-created resources.
#
# Default mode is DRY-RUN. Pass --apply to actually create resources.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=oci-common.sh
source "${SCRIPT_DIR}/oci-common.sh"

# --- Args ---
usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --apply           Actually create OCI resources (default is dry-run)
  --dry-run         Print planned commands without executing (default)
  -h, --help        Show this help

Configuration is read from environment variables in oci-common.sh.
State is persisted to: $STATE_FILE
SSH keypair:          $KEY_FILE (+ ${KEY_FILE}.pub)
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

if [[ "$APPLY" == "1" ]]; then
  warn "APPLY mode: real OCI resources will be created"
else
  info "DRY-RUN mode (default). Use --apply to create real resources."
fi

preflight
state_init

# --- 1. SSH keypair ---
create_ssh_keypair() {
  if [[ -f "$KEY_FILE" && -f "${KEY_FILE}.pub" ]]; then
    info "SSH keypair already exists at $KEY_FILE"
    return
  fi
  info "Generating ed25519 SSH keypair at $KEY_FILE"
  if [[ "$APPLY" == "1" ]]; then
    ssh-keygen -t ed25519 -N "" -C "$TESTBED_NAME" -f "$KEY_FILE"
    chmod 600 "$KEY_FILE"
  else
    echo "DRY-RUN: ssh-keygen -t ed25519 -N '' -f $KEY_FILE" >&2
  fi
}

# --- 2. VCN ---
create_vcn() {
  local existing
  existing="$(state_get vcn_id)"
  if [[ -n "$existing" ]]; then
    info "VCN already tracked in state: $existing"
    return
  fi
  info "Creating VCN $VCN_NAME ($VCN_CIDR)"
  if [[ "$APPLY" == "1" ]]; then
    local vcn_id
    vcn_id="$(oci network vcn create \
      --compartment-id "$OCI_COMPARTMENT_ID" \
      --display-name "$VCN_NAME" \
      --cidr-block "$VCN_CIDR" \
      --dns-label "$(echo "${TESTBED_NAME//-/}" | cut -c1-15)" \
      --wait-for-state AVAILABLE \
      --query 'data.id' --raw-output)"
    state_set vcn_id "$vcn_id"
    info "VCN created: $vcn_id"
  else
    oci_run network vcn create --display-name "$VCN_NAME" --cidr-block "$VCN_CIDR"
  fi
}

# --- 3. Internet Gateway ---
create_igw() {
  local existing
  existing="$(state_get igw_id)"
  if [[ -n "$existing" ]]; then
    info "IGW already tracked: $existing"
    return
  fi
  local vcn_id
  vcn_id="$(state_get vcn_id)"
  info "Creating Internet Gateway $IGW_NAME"
  if [[ "$APPLY" == "1" ]]; then
    local igw_id
    igw_id="$(oci network internet-gateway create \
      --compartment-id "$OCI_COMPARTMENT_ID" \
      --vcn-id "$vcn_id" \
      --display-name "$IGW_NAME" \
      --is-enabled true \
      --wait-for-state AVAILABLE \
      --query 'data.id' --raw-output)"
    state_set igw_id "$igw_id"
    info "IGW created: $igw_id"
  else
    oci_run network internet-gateway create --display-name "$IGW_NAME"
  fi
}

# --- 4. Update VCN default route table ---
update_route_table() {
  if [[ -n "$(state_get route_updated)" ]]; then
    info "Route table already updated"
    return
  fi
  local vcn_id igw_id rt_id
  vcn_id="$(state_get vcn_id)"
  igw_id="$(state_get igw_id)"
  info "Updating default route table to route 0.0.0.0/0 via IGW"
  if [[ "$APPLY" == "1" ]]; then
    rt_id="$(oci network vcn get --vcn-id "$vcn_id" \
      --query 'data."default-route-table-id"' --raw-output)"
    oci network route-table update --rt-id "$rt_id" --force \
      --route-rules "[{\"destination\":\"0.0.0.0/0\",\"destinationType\":\"CIDR_BLOCK\",\"networkEntityId\":\"$igw_id\"}]" \
      --wait-for-state AVAILABLE >/dev/null
    state_set route_table_id "$rt_id"
    state_set route_updated "true"
  else
    oci_run network route-table update --force --route-rules '[{"destination":"0.0.0.0/0","networkEntityId":"<igw>"}]'
  fi
}

# --- 5. Security list ---
# Public testbed: SSH (22/tcp), F1R3FLY P2P+APIs (40400-40405/tcp),
# Kademlia discovery (40404/udp), all-egress.
create_seclist() {
  local existing
  existing="$(state_get seclist_id)"
  if [[ -n "$existing" ]]; then
    info "Security list already tracked: $existing"
    return
  fi
  local vcn_id
  vcn_id="$(state_get vcn_id)"
  info "Creating security list $SECLIST_NAME"

  local ingress
  ingress='[
    {"source":"0.0.0.0/0","sourceType":"CIDR_BLOCK","protocol":"6","isStateless":false,"tcpOptions":{"destinationPortRange":{"min":22,"max":22}}},
    {"source":"0.0.0.0/0","sourceType":"CIDR_BLOCK","protocol":"6","isStateless":false,"tcpOptions":{"destinationPortRange":{"min":40400,"max":40405}}},
    {"source":"0.0.0.0/0","sourceType":"CIDR_BLOCK","protocol":"17","isStateless":false,"udpOptions":{"destinationPortRange":{"min":40404,"max":40404}}}
  ]'
  local egress
  egress='[{"destination":"0.0.0.0/0","destinationType":"CIDR_BLOCK","protocol":"all","isStateless":false}]'

  if [[ "$APPLY" == "1" ]]; then
    local seclist_id
    seclist_id="$(oci network security-list create \
      --compartment-id "$OCI_COMPARTMENT_ID" \
      --vcn-id "$vcn_id" \
      --display-name "$SECLIST_NAME" \
      --ingress-security-rules "$ingress" \
      --egress-security-rules "$egress" \
      --wait-for-state AVAILABLE \
      --query 'data.id' --raw-output)"
    state_set seclist_id "$seclist_id"
    info "Security list created: $seclist_id"
  else
    oci_run network security-list create --display-name "$SECLIST_NAME" --ingress-security-rules "<22,40400-40405/tcp,40404/udp>"
  fi
}

# --- 6. Subnet ---
create_subnet() {
  local existing
  existing="$(state_get subnet_id)"
  if [[ -n "$existing" ]]; then
    info "Subnet already tracked: $existing"
    return
  fi
  local vcn_id seclist_id
  vcn_id="$(state_get vcn_id)"
  seclist_id="$(state_get seclist_id)"
  info "Creating public subnet $SUBNET_NAME ($SUBNET_CIDR)"
  if [[ "$APPLY" == "1" ]]; then
    local subnet_id
    subnet_id="$(oci network subnet create \
      --compartment-id "$OCI_COMPARTMENT_ID" \
      --vcn-id "$vcn_id" \
      --display-name "$SUBNET_NAME" \
      --cidr-block "$SUBNET_CIDR" \
      --security-list-ids "[\"$seclist_id\"]" \
      --prohibit-public-ip-on-vnic false \
      --wait-for-state AVAILABLE \
      --query 'data.id' --raw-output)"
    state_set subnet_id "$subnet_id"
    info "Subnet created: $subnet_id"
  else
    oci_run network subnet create --display-name "$SUBNET_NAME" --cidr-block "$SUBNET_CIDR"
  fi
}

# --- 7. Instances ---
create_instance() {
  local name="$1" ocpus="$2" mem_gb="$3" role="$4"
  local state_key_id="${role}_instance_id"
  local state_key_ip="${role}_public_ip"

  if [[ -n "$(state_get "$state_key_id")" ]]; then
    info "$name already tracked: $(state_get "$state_key_id") @ $(state_get "$state_key_ip")"
    return
  fi

  local subnet_id image_id
  subnet_id="$(state_get subnet_id)"
  if [[ "$APPLY" == "1" ]]; then
    image_id="$(latest_ol9_arm_image)"
    info "Using OL9 aarch64 image: $image_id"
  else
    image_id="ocid1.image.oc1.us-sanjose-1.DRY-RUN"
  fi

  local ssh_pubkey=""
  if [[ -f "${KEY_FILE}.pub" ]]; then
    ssh_pubkey="$(cat "${KEY_FILE}.pub")"
  fi

  info "Launching $name ($ocpus OCPU, $mem_gb GB) as $role"
  if [[ "$APPLY" == "1" ]]; then
    local instance_id public_ip
    instance_id="$(oci compute instance launch \
      --availability-domain "$OCI_AD" \
      --compartment-id "$OCI_COMPARTMENT_ID" \
      --display-name "$name" \
      --shape "$SHAPE" \
      --shape-config "{\"ocpus\":$ocpus,\"memoryInGBs\":$mem_gb}" \
      --image-id "$image_id" \
      --subnet-id "$subnet_id" \
      --assign-public-ip true \
      --ssh-authorized-keys-file "${KEY_FILE}.pub" \
      --user-data-file "${SCRIPT_DIR}/cloud-init.yaml" \
      --wait-for-state RUNNING \
      --query 'data.id' --raw-output)"

    public_ip="$(oci compute instance list-vnics --instance-id "$instance_id" \
      --query 'data[0]."public-ip"' --raw-output)"

    state_set "$state_key_id" "$instance_id"
    state_set "$state_key_ip" "$public_ip"
    info "$name: $instance_id @ $public_ip"
  else
    oci_run compute instance launch --display-name "$name" --shape "$SHAPE" \
      --shape-config "{\"ocpus\":$ocpus,\"memoryInGBs\":$mem_gb}"
  fi
}

print_summary() {
  info "=== Testbed Provisioning Summary ==="
  if [[ "$APPLY" == "1" ]]; then
    jq . "$STATE_FILE"
    echo ""
    info "VPS-1 (bootstrap):  opc@$(state_get vps1_public_ip)"
    info "VPS-2 (followers):  opc@$(state_get vps2_public_ip)"
    info "SSH key:            $KEY_FILE"
    info "Connect: ssh -i $KEY_FILE opc@<ip>"
  else
    info "DRY-RUN complete. No resources were created."
    info "Re-run with --apply to create the testbed."
  fi
}

# --- Main ---
create_ssh_keypair
create_vcn
create_igw
update_route_table
create_seclist
create_subnet
create_instance "$VPS1_NAME" "$VPS1_OCPUS" "$VPS1_MEMORY_GB" vps1
create_instance "$VPS2_NAME" "$VPS2_OCPUS" "$VPS2_MEMORY_GB" vps2
print_summary

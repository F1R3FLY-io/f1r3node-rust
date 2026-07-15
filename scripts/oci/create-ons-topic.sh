#!/usr/bin/env bash
# One-time provisioning of the ONS topic for weekend-soak benchmark alerts
# (EPOCH-010 TASK-010-5). Run by a maintainer with OCI CLI auth (or on an OCI
# instance with --auth instance_principal via OCI_AUTH below).
#
# Usage:
#   COMPARTMENT_OCID=ocid1.compartment.oc1..xxxx scripts/oci/create-ons-topic.sh
#
# After creation, store the printed topic OCID as the GitHub Actions repository
# variable SOAK_ONS_TOPIC_OCID (Settings -> Secrets and variables -> Actions ->
# Variables). It is an identifier, not a secret.
#
# Subscribing an email: ONS sends a confirmation link; recipients manage their
# own subscription (unsubscribe link in every mail), so no recipient list is
# kept in-repo:
#   oci ons subscription create \
#     --compartment-id "$COMPARTMENT_OCID" \
#     --topic-id "<topic-ocid>" \
#     --protocol EMAIL \
#     --subscription-endpoint user@example.com

set -euo pipefail

TOPIC_NAME="${TOPIC_NAME:-soak-benchmark-reports}"
COMPARTMENT_OCID="${COMPARTMENT_OCID:?COMPARTMENT_OCID is required}"
OCI_AUTH="${OCI_AUTH:-}"

AUTH_ARGS=()
[ -n "$OCI_AUTH" ] && AUTH_ARGS=(--auth "$OCI_AUTH")

command -v oci >/dev/null || { echo "oci CLI not found" >&2; exit 2; }

existing="$(oci ons topic list "${AUTH_ARGS[@]}" \
  --compartment-id "$COMPARTMENT_OCID" --name "$TOPIC_NAME" \
  --lifecycle-state ACTIVE --query 'data[0]."topic-id"' --raw-output 2>/dev/null || true)"
if [ -n "$existing" ] && [ "$existing" != "null" ]; then
  echo "Topic '$TOPIC_NAME' already exists:" >&2
  echo "$existing"
  exit 0
fi

oci ons topic create "${AUTH_ARGS[@]}" \
  --compartment-id "$COMPARTMENT_OCID" \
  --name "$TOPIC_NAME" \
  --description "F1R3FLY weekend soak benchmark alerts (EPOCH-010): weekly verdict + dashboard link" \
  --query 'data."topic-id"' --raw-output

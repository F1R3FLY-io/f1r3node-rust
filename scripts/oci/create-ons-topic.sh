#!/usr/bin/env bash
# One-time provisioning of the ONS topic for weekend-soak benchmark alerts
# (EPOCH-010 TASK-010-5), plus idempotent email-subscription management.
# Run by a maintainer with OCI CLI auth (or on an OCI instance with
# --auth instance_principal via OCI_AUTH below).
#
# Usage:
#   COMPARTMENT_OCID=ocid1.compartment.oc1..xxxx \
#     scripts/oci/create-ons-topic.sh [email ...]
#
# Recipient emails may be passed as positional arguments and/or via
# SUBSCRIBER_EMAILS (comma- or space-separated). Each address gets an ONS
# EMAIL subscription on the topic; addresses that already have one (ACTIVE
# or awaiting confirmation) are skipped, so re-running with a grown list is
# safe. ONS sends each new address a confirmation link and every mail
# carries an unsubscribe link — the recipient list lives in OCI, never in
# this repository.
#
# After creation, store the printed topic OCID as the GitHub Actions repository
# variable SOAK_ONS_TOPIC_OCID (Settings -> Secrets and variables -> Actions ->
# Variables). It is an identifier, not a secret.

set -euo pipefail

TOPIC_NAME="${TOPIC_NAME:-soak-benchmark-reports}"
COMPARTMENT_OCID="${COMPARTMENT_OCID:?COMPARTMENT_OCID is required}"
OCI_AUTH="${OCI_AUTH:-}"
SUBSCRIBER_EMAILS="${SUBSCRIBER_EMAILS:-}"

AUTH_ARGS=()
[ -n "$OCI_AUTH" ] && AUTH_ARGS=(--auth "$OCI_AUTH")

command -v oci >/dev/null || { echo "oci CLI not found" >&2; exit 2; }

EMAILS=()
for addr in "$@" ${SUBSCRIBER_EMAILS//,/ }; do
  case "$addr" in
    *@*.*) EMAILS+=("$addr") ;;
    *) echo "skipping invalid email: $addr" >&2 ;;
  esac
done

TOPIC_OCID="$(oci ons topic list "${AUTH_ARGS[@]}" \
  --compartment-id "$COMPARTMENT_OCID" --name "$TOPIC_NAME" \
  --lifecycle-state ACTIVE --query 'data[0]."topic-id"' --raw-output 2>/dev/null || true)"
if [ -n "$TOPIC_OCID" ] && [ "$TOPIC_OCID" != "null" ]; then
  echo "Topic '$TOPIC_NAME' already exists" >&2
else
  TOPIC_OCID="$(oci ons topic create "${AUTH_ARGS[@]}" \
    --compartment-id "$COMPARTMENT_OCID" \
    --name "$TOPIC_NAME" \
    --description "F1R3FLY weekend soak benchmark alerts (EPOCH-010): weekly verdict + dashboard link" \
    --query 'data."topic-id"' --raw-output)"
  echo "Created topic '$TOPIC_NAME'" >&2
fi

if [ "${#EMAILS[@]}" -gt 0 ]; then
  command -v jq >/dev/null || { echo "jq not found (needed for subscription dedup)" >&2; exit 2; }
  existing_endpoints="$(oci ons subscription list "${AUTH_ARGS[@]}" \
    --compartment-id "$COMPARTMENT_OCID" --topic-id "$TOPIC_OCID" --all 2>/dev/null \
    | jq -r '.data[]? | select(.protocol == "EMAIL" and ."lifecycle-state" != "DELETED") | .endpoint' || true)"
  for email in "${EMAILS[@]}"; do
    if printf '%s\n' "$existing_endpoints" | grep -qixF "$email"; then
      echo "already subscribed: $email" >&2
      continue
    fi
    oci ons subscription create "${AUTH_ARGS[@]}" \
      --compartment-id "$COMPARTMENT_OCID" \
      --topic-id "$TOPIC_OCID" \
      --protocol EMAIL \
      --subscription-endpoint "$email" \
      --query 'data.id' --raw-output >&2
    echo "subscribed (pending email confirmation): $email" >&2
  done
fi

echo "$TOPIC_OCID"

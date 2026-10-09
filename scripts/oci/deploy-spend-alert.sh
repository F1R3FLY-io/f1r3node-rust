#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=scripts/oci/lib/fn-deploy.sh
. "$ROOT/scripts/oci/lib/fn-deploy.sh"

for name in OCI_COMPARTMENT_OCID ONS_TOPIC_OCID; do
	if [ -z "${!name:-}" ]; then
		printf '%s is required\n' "$name" >&2
		exit 2
	fi
done
fn_require_commands docker git jq oci

SOURCE="$ROOT/oci/spend-alert"
OCI_PROFILE="${OCI_PROFILE:-DEFAULT}"
OCI_CONFIG_FILE="${OCI_CLI_CONFIG_FILE:-$HOME/.oci/config}"
OCI_REGION="${OCI_REGION:-$(fn_config_value "$OCI_CONFIG_FILE" "$OCI_PROFILE" region)}"
TENANCY_OCID="${OCI_TENANCY_OCID:-$(fn_config_value "$OCI_CONFIG_FILE" "$OCI_PROFILE" tenancy)}"
OCIR_NAMESPACE="${OCIR_NAMESPACE:-$(oci os ns get --profile "$OCI_PROFILE" --query data --raw-output)}"
OCIR_REPOSITORY="${OCIR_REPOSITORY:-f1r3node/spend-alert}"
APPLICATION_NAME="${APPLICATION_NAME:-f1r3node-ci-schedulers}"
FUNCTION_NAME="${FUNCTION_NAME:-f1r3node-spend-alert}"
SCHEDULE_NAME="${SCHEDULE_NAME:-f1r3node-spend-alert-daily}"
SCHEDULE_CRON="${SCHEDULE_CRON:-0 14 * * *}"
THRESHOLD_USD="${THRESHOLD_USD:-70}"
IMAGE_TAG="${IMAGE_TAG:-$(git -C "$ROOT" rev-parse --short=12 HEAD)}"
IMAGE="${OCI_REGION}.ocir.io/${OCIR_NAMESPACE}/${OCIR_REPOSITORY}:${IMAGE_TAG}"

[ -n "$OCI_REGION" ] || {
	echo "OCI_REGION is required" >&2
	exit 2
}
[ -n "$TENANCY_OCID" ] || {
	echo "OCI_TENANCY_OCID is required" >&2
	exit 2
}
if ! [[ "$THRESHOLD_USD" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
	echo "THRESHOLD_USD must be a non-negative number" >&2
	exit 2
fi

application_id="$(fn_active_application_id "$APPLICATION_NAME")"
[ -n "$application_id" ] || {
	printf 'Function application %s is not ACTIVE in the compartment; deploy it with scripts/oci/deploy-soak-scheduler.sh first\n' "$APPLICATION_NAME" >&2
	exit 1
}

fn_build_and_push_image "$OCIR_REPOSITORY" "$SOURCE" "$IMAGE"

function_config="$(jq -cn \
	--arg tenancy "$TENANCY_OCID" \
	--arg topic "$ONS_TOPIC_OCID" \
	--arg threshold "$THRESHOLD_USD" \
	'{TENANCY_OCID:$tenancy,ONS_TOPIC_OCID:$topic,THRESHOLD_USD:$threshold}')"
function_id="$(fn_upsert_function "$application_id" "$FUNCTION_NAME" "$IMAGE" "$function_config")"
schedule_id="$(fn_upsert_schedule "$SCHEDULE_NAME" \
	"Email the ONS topic when the previous UTC day's tenancy spend exceeds the threshold" \
	"$SCHEDULE_CRON" "$function_id" '{"check":"daily-spend"}')"

function_group="f1r3node_spend_alert_function"
schedule_group="f1r3node_spend_alert_schedule"
fn_upsert_dynamic_group "$function_group" "ALL {resource.type='fnfunc', resource.id='${function_id}'}" >/dev/null
fn_upsert_dynamic_group "$schedule_group" "ALL {resource.type='resourceschedule', resource.id='${schedule_id}'}" >/dev/null

statements="$(jq -cn \
	--arg usage "Allow dynamic-group $function_group to read usage-report in tenancy" \
	--arg ons "Allow dynamic-group $function_group to use ons-topics in compartment id $OCI_COMPARTMENT_OCID" \
	--argjson schedule "$(fn_schedule_invoke_statements "$schedule_group" "$function_id")" \
	'[$usage,$ons] + $schedule')"
policy_id="$(fn_upsert_policy f1r3node-spend-alert "$statements")"

jq -n \
	--arg image "$IMAGE" \
	--arg function_id "$function_id" \
	--arg schedule_id "$schedule_id" \
	--arg policy_id "$policy_id" \
	--arg threshold "$THRESHOLD_USD" \
	--arg cron "$SCHEDULE_CRON" \
	'{image:$image,function_id:$function_id,schedule_id:$schedule_id,policy_id:$policy_id,threshold_usd:$threshold,schedule_cron_utc:$cron}'

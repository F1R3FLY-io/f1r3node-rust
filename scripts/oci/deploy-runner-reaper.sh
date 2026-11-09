#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=scripts/oci/lib/fn-deploy.sh
. "$ROOT/scripts/oci/lib/fn-deploy.sh"

fn_require_commands docker git jq oci

# The compartment comes from the one literal in ci-runner-reaper.yml, which
# check-workflow-invariants.sh (invariant 6) keeps unique. The Function can
# then never reap a different compartment than the GitHub reaper.
WORKFLOW="$ROOT/.github/workflows/ci-runner-reaper.yml"
workflow_compartment="$(grep -hoE 'CI_RUNNER_COMPARTMENT_OCID:[[:space:]]*"ocid1\.compartment\.[A-Za-z0-9._-]+"' "$WORKFLOW" |
	sed -E 's/.*"(ocid1\.compartment\.[A-Za-z0-9._-]+)"/\1/' || true)"
if [ "$(printf '%s' "$workflow_compartment" | grep -c . || true)" -ne 1 ]; then
	printf '%s must pin exactly one CI_RUNNER_COMPARTMENT_OCID literal\n' "$WORKFLOW" >&2
	exit 1
fi
if [ -n "${OCI_COMPARTMENT_OCID:-}" ] && [ "$OCI_COMPARTMENT_OCID" != "$workflow_compartment" ]; then
	echo "OCI_COMPARTMENT_OCID differs from the compartment in ci-runner-reaper.yml" >&2
	exit 1
fi
OCI_COMPARTMENT_OCID="$workflow_compartment"

SOURCE="$ROOT/oci/runner-reaper"
OCI_PROFILE="${OCI_PROFILE:-DEFAULT}"
OCI_CONFIG_FILE="${OCI_CLI_CONFIG_FILE:-$HOME/.oci/config}"
OCI_REGION="${OCI_REGION:-$(fn_config_value "$OCI_CONFIG_FILE" "$OCI_PROFILE" region)}"
TENANCY_OCID="${OCI_TENANCY_OCID:-$(fn_config_value "$OCI_CONFIG_FILE" "$OCI_PROFILE" tenancy)}"
OCIR_NAMESPACE="${OCIR_NAMESPACE:-$(oci os ns get --profile "$OCI_PROFILE" --query data --raw-output)}"
OCIR_REPOSITORY="${OCIR_REPOSITORY:-f1r3node/runner-reaper}"
APPLICATION_NAME="${APPLICATION_NAME:-f1r3node-ci-schedulers}"
FUNCTION_NAME="${FUNCTION_NAME:-f1r3node-runner-reaper}"
SCHEDULE_NAME="${SCHEDULE_NAME:-f1r3node-runner-reaper-30m}"
SCHEDULE_CRON="${SCHEDULE_CRON:-*/30 * * * *}"
MAX_AGE_HOURS="${MAX_AGE_HOURS:-2}"
DRY_RUN="${DRY_RUN:-false}"
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
case "$MAX_AGE_HOURS" in
'' | *[!0-9]* | 0)
	echo "MAX_AGE_HOURS must be a positive integer" >&2
	exit 2
	;;
esac
case "$DRY_RUN" in
true | false) ;;
*)
	echo "DRY_RUN must be true or false" >&2
	exit 2
	;;
esac

application_id="$(fn_active_application_id "$APPLICATION_NAME")"
[ -n "$application_id" ] || {
	printf 'Function application %s is not ACTIVE in the compartment; deploy it with scripts/oci/deploy-soak-scheduler.sh first\n' "$APPLICATION_NAME" >&2
	exit 1
}

fn_build_and_push_image "$OCIR_REPOSITORY" "$SOURCE" "$IMAGE"

function_config="$(jq -cn \
	--arg compartment "$OCI_COMPARTMENT_OCID" \
	--arg max_age "$MAX_AGE_HOURS" \
	--arg dry_run "$DRY_RUN" \
	'{COMPARTMENT_OCID:$compartment,MAX_AGE_HOURS:$max_age,DRY_RUN:$dry_run}')"
function_id="$(fn_upsert_function "$application_id" "$FUNCTION_NAME" "$IMAGE" "$function_config")"
schedule_id="$(fn_upsert_schedule "$SCHEDULE_NAME" \
	"Terminate leaked ci-eph-* runner VMs in the ci-runner compartment" \
	"$SCHEDULE_CRON" "$function_id" '{"check":"runner-reaper"}')"

function_group="f1r3node_runner_reaper_function"
schedule_group="f1r3node_runner_reaper_schedule"
fn_upsert_dynamic_group "$function_group" "ALL {resource.type='fnfunc', resource.id='${function_id}'}" >/dev/null
fn_upsert_dynamic_group "$schedule_group" "ALL {resource.type='resourceschedule', resource.id='${schedule_id}'}" >/dev/null

# Same grant as ci-runner-ephemeral-policy, which lets each runner terminate
# itself, scoped to the ci-runner compartment only.
statements="$(jq -cn \
	--arg instances "Allow dynamic-group $function_group to manage instance-family in compartment id $OCI_COMPARTMENT_OCID" \
	--argjson schedule "$(fn_schedule_invoke_statements "$schedule_group" "$function_id")" \
	'[$instances] + $schedule')"
policy_id="$(fn_upsert_policy f1r3node-runner-reaper "$statements")"

jq -n \
	--arg image "$IMAGE" \
	--arg function_id "$function_id" \
	--arg schedule_id "$schedule_id" \
	--arg policy_id "$policy_id" \
	--arg max_age "$MAX_AGE_HOURS" \
	--arg dry_run "$DRY_RUN" \
	--arg cron "$SCHEDULE_CRON" \
	'{image:$image,function_id:$function_id,schedule_id:$schedule_id,policy_id:$policy_id,max_age_hours:$max_age,dry_run:$dry_run,schedule_cron_utc:$cron}'

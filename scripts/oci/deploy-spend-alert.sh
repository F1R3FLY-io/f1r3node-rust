#!/usr/bin/env bash
set -euo pipefail

required=(
	OCI_COMPARTMENT_OCID
	ONS_TOPIC_OCID
)
for name in "${required[@]}"; do
	if [ -z "${!name:-}" ]; then
		printf '%s is required\n' "$name" >&2
		exit 2
	fi
done
for command in docker git jq oci; do
	command -v "$command" >/dev/null || {
		printf '%s is required\n' "$command" >&2
		exit 2
	}
done

config_value() {
	local file="$1" profile="$2" key="$3"
	[ -r "$file" ] || return 0
	awk -F= -v profile="$profile" -v key="$key" '
        /^[[:space:]]*\[/ {
            section = $0
            gsub(/^[[:space:]]*\[|\][[:space:]]*$/, "", section)
            active = section == profile
            next
        }
        active {
            candidate = $1
            gsub(/^[[:space:]]+|[[:space:]]+$/, "", candidate)
            if (candidate == key) {
                value = substr($0, index($0, "=") + 1)
                gsub(/^[[:space:]]+|[[:space:]]+$/, "", value)
                print value
                exit
            }
        }
    ' "$file"
}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOURCE="$ROOT/oci/spend-alert"
OCI_PROFILE="${OCI_PROFILE:-DEFAULT}"
OCI_CONFIG_FILE="${OCI_CLI_CONFIG_FILE:-$HOME/.oci/config}"
OCI_REGION="${OCI_REGION:-$(config_value "$OCI_CONFIG_FILE" "$OCI_PROFILE" region)}"
OCIR_NAMESPACE="${OCIR_NAMESPACE:-$(oci os ns get --profile "$OCI_PROFILE" --query data --raw-output)}"
OCIR_REPOSITORY="${OCIR_REPOSITORY:-f1r3node/spend-alert}"
APPLICATION_NAME="${APPLICATION_NAME:-f1r3node-ci-schedulers}"
FUNCTION_NAME="${FUNCTION_NAME:-f1r3node-spend-alert}"
SCHEDULE_NAME="${SCHEDULE_NAME:-f1r3node-spend-alert-daily}"
SCHEDULE_CRON="${SCHEDULE_CRON:-0 14 * * *}"
THRESHOLD_USD="${THRESHOLD_USD:-70}"
IMAGE_TAG="${IMAGE_TAG:-$(git -C "$ROOT" rev-parse --short=12 HEAD)}"
IMAGE="${OCI_REGION}.ocir.io/${OCIR_NAMESPACE}/${OCIR_REPOSITORY}:${IMAGE_TAG}"
TENANCY_OCID="${OCI_TENANCY_OCID:-$(config_value "$OCI_CONFIG_FILE" "$OCI_PROFILE" tenancy)}"

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

application_id="$(oci fn application list \
	--profile "$OCI_PROFILE" \
	--compartment-id "$OCI_COMPARTMENT_OCID" \
	--all \
	--output json |
	jq -r --arg name "$APPLICATION_NAME" '.data[] | select(."display-name" == $name and ."lifecycle-state" == "ACTIVE") | .id' |
	head -1)"
[ -n "$application_id" ] || {
	printf 'Function application %s is not ACTIVE in the compartment; deploy it with scripts/oci/deploy-soak-scheduler.sh first\n' "$APPLICATION_NAME" >&2
	exit 1
}

repo_id="$(oci artifacts container repository list \
	--profile "$OCI_PROFILE" \
	--compartment-id "$OCI_COMPARTMENT_OCID" \
	--all \
	--output json |
	jq -r --arg name "$OCIR_REPOSITORY" '.data.items[] | select(."display-name" == $name) | .id' |
	head -1)"
if [ -z "$repo_id" ]; then
	oci artifacts container repository create \
		--profile "$OCI_PROFILE" \
		--compartment-id "$OCI_COMPARTMENT_OCID" \
		--display-name "$OCIR_REPOSITORY" \
		--is-public false >/dev/null
fi

docker build --platform linux/amd64 -t "$IMAGE" "$SOURCE"
docker push "$IMAGE"

function_config="$(jq -cn \
	--arg tenancy "$TENANCY_OCID" \
	--arg topic "$ONS_TOPIC_OCID" \
	--arg threshold "$THRESHOLD_USD" \
	'{TENANCY_OCID:$tenancy,ONS_TOPIC_OCID:$topic,THRESHOLD_USD:$threshold}')"
function_id="$(oci fn function list \
	--profile "$OCI_PROFILE" \
	--application-id "$application_id" \
	--all \
	--output json |
	jq -r --arg name "$FUNCTION_NAME" '.data[] | select(."display-name" == $name and ."lifecycle-state" != "DELETED") | .id' |
	head -1)"
if [ -z "$function_id" ]; then
	function_id="$(oci fn function create \
		--profile "$OCI_PROFILE" \
		--application-id "$application_id" \
		--display-name "$FUNCTION_NAME" \
		--image "$IMAGE" \
		--memory-in-mbs 256 \
		--timeout-in-seconds 120 \
		--config "$function_config" \
		--wait-for-state ACTIVE \
		--query data.id \
		--raw-output)"
else
	oci fn function update \
		--profile "$OCI_PROFILE" \
		--function-id "$function_id" \
		--image "$IMAGE" \
		--memory-in-mbs 256 \
		--timeout-in-seconds 120 \
		--config "$function_config" \
		--wait-for-state ACTIVE \
		--force >/dev/null
fi

resources="$(jq -cn --arg id "$function_id" \
	'[{id:$id,metadata:{resourceType:"FunctionsFunction"},parameters:[{parameterType:"BODY",value:{check:"daily-spend"}}]}]')"
schedule_id="$(oci resource-scheduler schedule list \
	--profile "$OCI_PROFILE" \
	--compartment-id "$OCI_COMPARTMENT_OCID" \
	--all \
	--output json |
	jq -r --arg name "$SCHEDULE_NAME" '.data.items[] | select(."display-name" == $name and ."lifecycle-state" != "DELETED") | .id' |
	head -1)"
if [ -z "$schedule_id" ]; then
	schedule_id="$(oci resource-scheduler schedule create \
		--profile "$OCI_PROFILE" \
		--compartment-id "$OCI_COMPARTMENT_OCID" \
		--display-name "$SCHEDULE_NAME" \
		--description "Email the ONS topic when the previous UTC day's tenancy spend exceeds the threshold" \
		--action START_RESOURCE \
		--recurrence-type CRON \
		--recurrence-details "$SCHEDULE_CRON" \
		--resources "$resources" \
		--freeform-tags '{"managed-by":"f1r3node-rust"}' \
		--query data.id \
		--raw-output)"
else
	oci resource-scheduler schedule update \
		--profile "$OCI_PROFILE" \
		--schedule-id "$schedule_id" \
		--action START_RESOURCE \
		--recurrence-type CRON \
		--recurrence-details "$SCHEDULE_CRON" \
		--resources "$resources" \
		--force >/dev/null
fi

upsert_dynamic_group() {
	local name="$1" rule="$2" id actual
	id="$(oci iam dynamic-group list \
		--profile "$OCI_PROFILE" \
		--compartment-id "$TENANCY_OCID" \
		--all \
		--output json |
		jq -r --arg name "$name" '.data[] | select(.name == $name and ."lifecycle-state" != "DELETED") | .id' |
		head -1)"
	if [ -z "$id" ]; then
		id="$(oci iam dynamic-group create \
			--profile "$OCI_PROFILE" \
			--compartment-id "$TENANCY_OCID" \
			--name "$name" \
			--description "$name" \
			--matching-rule "$rule" \
			--query data.id \
			--raw-output)"
	else
		oci iam dynamic-group update \
			--profile "$OCI_PROFILE" \
			--dynamic-group-id "$id" \
			--matching-rule "$rule" \
			--force >/dev/null
	fi
	if ! actual="$(oci iam dynamic-group get \
		--profile "$OCI_PROFILE" \
		--dynamic-group-id "$id" \
		--query 'data."matching-rule"' \
		--raw-output)"; then
		echo "ERROR: could not verify dynamic group $name matching rule via OCI GET." >&2
		return 1
	fi
	if [ "$actual" != "$rule" ]; then
		echo "ERROR: dynamic group $name matching rule did not persist." >&2
		return 1
	fi
	printf '%s\n' "$id"
}

function_group="f1r3node_spend_alert_function"
schedule_group="f1r3node_spend_alert_schedule"
upsert_dynamic_group "$function_group" "ALL {resource.type='fnfunc', resource.id='${function_id}'}" >/dev/null
upsert_dynamic_group "$schedule_group" "ALL {resource.type='resourceschedule', resource.id='${schedule_id}'}" >/dev/null

policy_name="f1r3node-spend-alert"
statements="$(jq -cn \
	--arg usage "Allow dynamic-group $function_group to read usage-report in tenancy" \
	--arg ons "Allow dynamic-group $function_group to use ons-topics in compartment id $OCI_COMPARTMENT_OCID" \
	--arg invoke "Allow dynamic-group $schedule_group to use fn-invocation in compartment id $OCI_COMPARTMENT_OCID where target.function.id = '$function_id'" \
	--arg read "Allow dynamic-group $schedule_group to read functions-family in compartment id $OCI_COMPARTMENT_OCID where target.function.id = '$function_id'" \
	'[$usage,$ons,$invoke,$read]')"
policy_id="$(oci iam policy list \
	--profile "$OCI_PROFILE" \
	--compartment-id "$TENANCY_OCID" \
	--all \
	--output json |
	jq -r --arg name "$policy_name" '.data[] | select(.name == $name and ."lifecycle-state" != "DELETED") | .id' |
	head -1)"
if [ -z "$policy_id" ]; then
	policy_id="$(oci iam policy create \
		--profile "$OCI_PROFILE" \
		--compartment-id "$TENANCY_OCID" \
		--name "$policy_name" \
		--description "$policy_name" \
		--statements "$statements" \
		--query data.id \
		--raw-output)"
else
	oci iam policy update \
		--profile "$OCI_PROFILE" \
		--policy-id "$policy_id" \
		--statements "$statements" \
		--version-date "" \
		--force >/dev/null
fi

jq -n \
	--arg image "$IMAGE" \
	--arg function_id "$function_id" \
	--arg schedule_id "$schedule_id" \
	--arg policy_id "$policy_id" \
	--arg threshold "$THRESHOLD_USD" \
	--arg cron "$SCHEDULE_CRON" \
	'{image:$image,function_id:$function_id,schedule_id:$schedule_id,policy_id:$policy_id,threshold_usd:$threshold,schedule_cron_utc:$cron}'

#!/usr/bin/env bash
set -euo pipefail

required=(
	OCI_COMPARTMENT_OCID
	OCI_SUBNET_OCID
	GITHUB_APP_ID
	GITHUB_APP_INSTALLATION_ID
	GITHUB_APP_PRIVATE_KEY_SECRET_OCID
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

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOURCE="$ROOT/oci/soak-scheduler"
OCI_REGION="${OCI_REGION:-$(awk -F= '$1 == "region" {gsub(/[[:space:]]/, "", $2); print $2; exit}' "$HOME/.oci/config")}"
OCI_PROFILE="${OCI_PROFILE:-DEFAULT}"
OCIR_NAMESPACE="${OCIR_NAMESPACE:-$(oci os ns get --profile "$OCI_PROFILE" --query data --raw-output)}"
OCIR_REPOSITORY="${OCIR_REPOSITORY:-f1r3node/soak-scheduler}"
APPLICATION_NAME="${APPLICATION_NAME:-f1r3node-ci-schedulers}"
FUNCTION_NAME="${FUNCTION_NAME:-f1r3node-soak-scheduler}"
IMAGE_TAG="${IMAGE_TAG:-$(git -C "$ROOT" rev-parse --short=12 HEAD)}"
IMAGE="${OCI_REGION}.ocir.io/${OCIR_NAMESPACE}/${OCIR_REPOSITORY}:${IMAGE_TAG}"
TENANCY_OCID="${OCI_TENANCY_OCID:-$(awk -F= '$1 == "tenancy" {gsub(/[[:space:]]/, "", $2); print $2; exit}' "$HOME/.oci/config")}"
SECRET_COMPARTMENT_OCID="${GITHUB_SECRET_COMPARTMENT_OCID:-$OCI_COMPARTMENT_OCID}"

[ -n "$OCI_REGION" ] || {
	echo "OCI_REGION is required" >&2
	exit 2
}
[ -n "$TENANCY_OCID" ] || {
	echo "OCI_TENANCY_OCID is required" >&2
	exit 2
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

application_id="$(oci fn application list \
	--profile "$OCI_PROFILE" \
	--compartment-id "$OCI_COMPARTMENT_OCID" \
	--all \
	--output json |
	jq -r --arg name "$APPLICATION_NAME" '.data[] | select(."display-name" == $name and ."lifecycle-state" != "DELETED") | .id' |
	head -1)"
if [ -z "$application_id" ]; then
	subnet_ids="$(jq -cn --arg id "$OCI_SUBNET_OCID" '[$id]')"
	application_id="$(oci fn application create \
		--profile "$OCI_PROFILE" \
		--compartment-id "$OCI_COMPARTMENT_OCID" \
		--display-name "$APPLICATION_NAME" \
		--subnet-ids "$subnet_ids" \
		--query data.id \
		--raw-output)"
fi

function_config="$(jq -cn \
	--arg app_id "$GITHUB_APP_ID" \
	--arg installation_id "$GITHUB_APP_INSTALLATION_ID" \
	--arg secret_ocid "$GITHUB_APP_PRIVATE_KEY_SECRET_OCID" \
	'{GITHUB_REPOSITORY:"F1R3FLY-io/f1r3node-rust",GITHUB_WORKFLOW:"merge-recovery-soak.yml",GITHUB_WORKFLOW_REF:"master",GITHUB_APP_ID:$app_id,GITHUB_APP_INSTALLATION_ID:$installation_id,GITHUB_APP_PRIVATE_KEY_SECRET_OCID:$secret_ocid,MAX_TRIGGER_DELAY_SECONDS:"900"}')"
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
		--force >/dev/null
fi

create_schedule() {
	local hour="$1" name schedule_id resources
	name="f1r3node-soak-${hour}30-utc"
	resources="$(jq -cn \
		--arg id "$function_id" \
		--argjson hour "$hour" \
		'[{id:$id,metadata:{resourceType:"FunctionsFunction"},parameters:[{parameterType:"BODY",value:{slot_hour_utc:$hour}}]}]')"
	schedule_id="$(oci resource-scheduler schedule list \
		--profile "$OCI_PROFILE" \
		--compartment-id "$OCI_COMPARTMENT_OCID" \
		--all \
		--output json |
		jq -r --arg name "$name" '.data.items[] | select(."display-name" == $name and ."lifecycle-state" != "DELETED") | .id' |
		head -1)"
	if [ -z "$schedule_id" ]; then
		schedule_id="$(oci resource-scheduler schedule create \
			--profile "$OCI_PROFILE" \
			--compartment-id "$OCI_COMPARTMENT_OCID" \
			--display-name "$name" \
			--description "Dispatch the F1R3node merge recovery soak at the eligible Pacific slot" \
			--action START_RESOURCE \
			--recurrence-type CRON \
			--recurrence-details "30 ${hour} * * *" \
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
			--recurrence-details "30 ${hour} * * *" \
			--resources "$resources" \
			--force >/dev/null
	fi
	printf '%s\n' "$schedule_id"
}

schedule_02_id="$(create_schedule 2)"
schedule_03_id="$(create_schedule 3)"

upsert_dynamic_group() {
	local name="$1" rule="$2" id
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
	printf '%s\n' "$id"
}

function_group="f1r3node_soak_scheduler_function"
schedule_02_group="f1r3node_soak_scheduler_02"
schedule_03_group="f1r3node_soak_scheduler_03"
upsert_dynamic_group "$function_group" "ALL {resource.type='fnfunc', resource.id='${function_id}'}" >/dev/null
upsert_dynamic_group "$schedule_02_group" "ALL {resource.type='resourceschedule', resource.id='${schedule_02_id}'}" >/dev/null
upsert_dynamic_group "$schedule_03_group" "ALL {resource.type='resourceschedule', resource.id='${schedule_03_id}'}" >/dev/null

policy_name="f1r3node-soak-scheduler"
statements="$(jq -cn \
	--arg function_group "$function_group" \
	--arg schedule_02_group "$schedule_02_group" \
	--arg schedule_03_group "$schedule_03_group" \
	--arg function_compartment "$OCI_COMPARTMENT_OCID" \
	--arg secret_compartment "$SECRET_COMPARTMENT_OCID" \
	'["Allow dynamic-group \($function_group) to read secret-bundles in compartment id \($secret_compartment)","Allow dynamic-group \($schedule_02_group) to manage functions-family in compartment id \($function_compartment)","Allow dynamic-group \($schedule_03_group) to manage functions-family in compartment id \($function_compartment)"]')"
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
		--force >/dev/null
fi

jq -n \
	--arg image "$IMAGE" \
	--arg application_id "$application_id" \
	--arg function_id "$function_id" \
	--arg schedule_02_id "$schedule_02_id" \
	--arg schedule_03_id "$schedule_03_id" \
	--arg policy_id "$policy_id" \
	'{image:$image,application_id:$application_id,function_id:$function_id,schedules:[$schedule_02_id,$schedule_03_id],policy_id:$policy_id}'

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
SOURCE="$ROOT/oci/soak-scheduler"
OCI_PROFILE="${OCI_PROFILE:-DEFAULT}"
OCI_CONFIG_FILE="${OCI_CLI_CONFIG_FILE:-$HOME/.oci/config}"
OCI_REGION="${OCI_REGION:-$(config_value "$OCI_CONFIG_FILE" "$OCI_PROFILE" region)}"
OCIR_NAMESPACE="${OCIR_NAMESPACE:-$(oci os ns get --profile "$OCI_PROFILE" --query data --raw-output)}"
OCIR_REPOSITORY="${OCIR_REPOSITORY:-f1r3node/soak-scheduler}"
APPLICATION_NAME="${APPLICATION_NAME:-f1r3node-ci-schedulers}"
FUNCTION_NAME="${FUNCTION_NAME:-f1r3node-soak-scheduler}"
IMAGE_TAG="${IMAGE_TAG:-$(git -C "$ROOT" rev-parse --short=12 HEAD)}"
IMAGE="${OCI_REGION}.ocir.io/${OCIR_NAMESPACE}/${OCIR_REPOSITORY}:${IMAGE_TAG}"
TENANCY_OCID="${OCI_TENANCY_OCID:-$(config_value "$OCI_CONFIG_FILE" "$OCI_PROFILE" tenancy)}"
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
subnet_ids="$(jq -cn --arg id "$OCI_SUBNET_OCID" '[$id]')"
if [ -z "$application_id" ]; then
	application_id="$(oci fn application create \
		--profile "$OCI_PROFILE" \
		--compartment-id "$OCI_COMPARTMENT_OCID" \
		--display-name "$APPLICATION_NAME" \
		--subnet-ids "$subnet_ids" \
		--wait-for-state ACTIVE \
		--query data.id \
		--raw-output)"
else
	application="$(oci fn application get \
		--profile "$OCI_PROFILE" \
		--application-id "$application_id" \
		--output json)"
	actual_subnet_ids="$(jq -c '.data."subnet-ids" | sort' <<<"$application")"
	desired_subnet_ids="$(jq -c 'sort' <<<"$subnet_ids")"
	if [ "$actual_subnet_ids" != "$desired_subnet_ids" ]; then
		printf 'existing Function application %s uses subnet IDs %s; OCI cannot update application subnets in place, so recreate it with %s\n' \
			"$application_id" "$actual_subnet_ids" "$desired_subnet_ids" >&2
		exit 1
	fi
	if [ "$(jq -r '.data."lifecycle-state"' <<<"$application")" != ACTIVE ]; then
		printf 'existing Function application %s is not ACTIVE\n' "$application_id" >&2
		exit 1
	fi
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
	# Verify-after-write, via GET only. The 2026-08-04 deploy left all three
	# groups with NO matching rule — empty groups authorize nothing, so every
	# nightly schedule 404'd on the function for a week while the GitHub cron
	# fallback silently carried the soaks. The failure was invisible because
	# BOTH `oci iam dynamic-group list` and the identity-domains list endpoint
	# report matching-rule=null regardless of the real value in this tenancy;
	# only a GET returns the truth. A rule that does not bind is worse than no
	# rule, because it reads as a guarantee and stops people re-checking.
	local actual
	actual="$(oci iam dynamic-group get \
		--profile "$OCI_PROFILE" \
		--dynamic-group-id "$id" \
		--query 'data."matching-rule"' \
		--raw-output 2>/dev/null || true)"
	if [ "$actual" != "$rule" ]; then
		echo "ERROR: dynamic group $name matching rule did not persist." >&2
		echo "  wanted: $rule" >&2
		echo "  got:    ${actual:-<null>}" >&2
		exit 1
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
secret_statement="Allow dynamic-group $function_group to read secret-bundles in compartment id $SECRET_COMPARTMENT_OCID"
schedule_02_statement="Allow dynamic-group $schedule_02_group to use fn-invocation in compartment id $OCI_COMPARTMENT_OCID where target.function.id = '$function_id'"
schedule_03_statement="Allow dynamic-group $schedule_03_group to use fn-invocation in compartment id $OCI_COMPARTMENT_OCID where target.function.id = '$function_id'"
# The read grants are load-bearing, not belt-and-braces: `use fn-invocation`
# does not cover the scheduler's resource-RESOLUTION step, and without `read
# functions-family` the START_RESOURCE work request dies at 0% with a 404 on
# the function OCID (OCI reports authorization failures as 404). Added live
# 2026-08-11; this list REPLACES the policy wholesale on update, so removing
# them here would silently revert the fix on the next redeploy.
schedule_02_read_statement="Allow dynamic-group $schedule_02_group to read functions-family in compartment id $OCI_COMPARTMENT_OCID where target.function.id = '$function_id'"
schedule_03_read_statement="Allow dynamic-group $schedule_03_group to read functions-family in compartment id $OCI_COMPARTMENT_OCID where target.function.id = '$function_id'"
statements="$(jq -cn \
	--arg secret "$secret_statement" \
	--arg schedule_02 "$schedule_02_statement" \
	--arg schedule_03 "$schedule_03_statement" \
	--arg schedule_02_read "$schedule_02_read_statement" \
	--arg schedule_03_read "$schedule_03_read_statement" \
	'[$secret,$schedule_02,$schedule_03,$schedule_02_read,$schedule_03_read]')"
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

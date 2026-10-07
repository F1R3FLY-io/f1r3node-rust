#!/usr/bin/env bash
# Shared upserts for the Bash OCI Functions in the f1r3node-ci-schedulers
# application. The caller sets OCI_PROFILE, OCI_COMPARTMENT_OCID, and
# TENANCY_OCID before it calls these functions.

fn_require_commands() {
	local command
	for command in "$@"; do
		command -v "$command" >/dev/null || {
			printf '%s is required\n' "$command" >&2
			return 2
		}
	done
}

fn_config_value() {
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

fn_active_application_id() {
	local name="$1"
	oci fn application list \
		--profile "$OCI_PROFILE" \
		--compartment-id "$OCI_COMPARTMENT_OCID" \
		--all \
		--output json |
		jq -r --arg name "$name" '.data[] | select(."display-name" == $name and ."lifecycle-state" == "ACTIVE") | .id' |
		head -1
}

fn_build_and_push_image() {
	local repository="$1" source="$2" image="$3" repo_id
	repo_id="$(oci artifacts container repository list \
		--profile "$OCI_PROFILE" \
		--compartment-id "$OCI_COMPARTMENT_OCID" \
		--all \
		--output json |
		jq -r --arg name "$repository" '.data.items[] | select(."display-name" == $name) | .id' |
		head -1)"
	if [ -z "$repo_id" ]; then
		oci artifacts container repository create \
			--profile "$OCI_PROFILE" \
			--compartment-id "$OCI_COMPARTMENT_OCID" \
			--display-name "$repository" \
			--is-public false >/dev/null
	fi
	docker build --platform linux/amd64 -t "$image" "$source"
	docker push "$image"
}

fn_upsert_function() {
	local application_id="$1" name="$2" image="$3" config="$4" function_id
	function_id="$(oci fn function list \
		--profile "$OCI_PROFILE" \
		--application-id "$application_id" \
		--all \
		--output json |
		jq -r --arg name "$name" '.data[] | select(."display-name" == $name and ."lifecycle-state" != "DELETED") | .id' |
		head -1)"
	if [ -z "$function_id" ]; then
		function_id="$(oci fn function create \
			--profile "$OCI_PROFILE" \
			--application-id "$application_id" \
			--display-name "$name" \
			--image "$image" \
			--memory-in-mbs 256 \
			--timeout-in-seconds 120 \
			--config "$config" \
			--wait-for-state ACTIVE \
			--query data.id \
			--raw-output)"
	else
		oci fn function update \
			--profile "$OCI_PROFILE" \
			--function-id "$function_id" \
			--image "$image" \
			--memory-in-mbs 256 \
			--timeout-in-seconds 120 \
			--config "$config" \
			--wait-for-state ACTIVE \
			--force >/dev/null
	fi
	printf '%s\n' "$function_id"
}

fn_upsert_schedule() {
	local name="$1" description="$2" cron="$3" function_id="$4" body="$5" resources schedule_id
	resources="$(jq -cn --arg id "$function_id" --argjson body "$body" \
		'[{id:$id,metadata:{resourceType:"FunctionsFunction"},parameters:[{parameterType:"BODY",value:$body}]}]')"
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
			--description "$description" \
			--action START_RESOURCE \
			--recurrence-type CRON \
			--recurrence-details "$cron" \
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
			--recurrence-details "$cron" \
			--resources "$resources" \
			--force >/dev/null
	fi
	printf '%s\n' "$schedule_id"
}

# Verify-after-write via GET: in this tenancy the list endpoints report
# matching-rule=null whatever the real value is (see deploy-soak-scheduler.sh).
fn_upsert_dynamic_group() {
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

fn_upsert_policy() {
	local name="$1" statements="$2" policy_id
	policy_id="$(oci iam policy list \
		--profile "$OCI_PROFILE" \
		--compartment-id "$TENANCY_OCID" \
		--all \
		--output json |
		jq -r --arg name "$name" '.data[] | select(.name == $name and ."lifecycle-state" != "DELETED") | .id' |
		head -1)"
	if [ -z "$policy_id" ]; then
		policy_id="$(oci iam policy create \
			--profile "$OCI_PROFILE" \
			--compartment-id "$TENANCY_OCID" \
			--name "$name" \
			--description "$name" \
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
	printf '%s\n' "$policy_id"
}

fn_schedule_invoke_statements() {
	local group="$1" function_id="$2"
	jq -cn \
		--arg invoke "Allow dynamic-group $group to use fn-invocation in compartment id $OCI_COMPARTMENT_OCID where target.function.id = '$function_id'" \
		--arg read "Allow dynamic-group $group to read functions-family in compartment id $OCI_COMPARTMENT_OCID where target.function.id = '$function_id'" \
		'[$invoke,$read]'
}

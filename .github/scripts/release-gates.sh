#!/usr/bin/env bash
set -euo pipefail

# Promotion gate evaluator (docs/release-process.md sections 8, 10, 11).
#
# The script reads JSON documents only. The workflow fetches them from the
# GitHub API and from the candidate prerelease assets, then calls
# `evaluate`. The script never contacts the network, so the full decision
# is reproducible from the gate directory alone.
#
# Gate directory contents (every file is a gate-evidence document):
#   release-evidence.json       candidate evidence (release-evidence.sh schema)
#   ci-run.json                 GET actions/runs/{ci.run_id}
#   ci-jobs.json                GET actions/runs/{ci.run_id}/attempts/{n}/jobs
#   slashing-run.json           GET actions/runs/{id} for the slashing run
#   slashing-jobs.json          GET .../attempts/{n}/jobs for the slashing run
#   oci-validation-evidence.json  published by the OCI validation workflow
#   soak-evidence.json          published by the 60h stability soak
#   verdict.json                soak regression verdict
#   maintainer-review.json      optional, accepts a regress verdict
#
# Each gate resolves to pass, hold, or fail. A hold means the evidence is
# absent or incomplete and promotion waits. A fail means the evidence exists
# and contradicts the candidate; no override can replace it.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EVIDENCE_TOOL="$SCRIPT_DIR/release-evidence.sh"
REQUIRED_SLASHING_JOBS_FILE="$SCRIPT_DIR/release-required-slashing-jobs.txt"
SOAK_DURATION_SECONDS=216000

EXIT_PROMOTABLE=0
EXIT_HELD=10
EXIT_FAILED=20

fail() {
	printf 'release gates error: %s\n' "$1" >&2
	exit 1
}

require_file() {
	[ -f "$1" ] || fail "missing file: $1"
}

require_sha() {
	[[ "$1" =~ ^[0-9a-f]{40}$ ]] || fail "$2 must be a full lowercase commit SHA"
}

gate_result() {
	jq -n --arg id "$1" --arg status "$2" --arg reason "$3" \
		'{id: $id, status: $status, reason: $reason}'
}

# Verify that a workflow-run document describes the exact candidate source
# and a successful completion. Prints a reason on failure, nothing on success.
run_mismatch_reason() {
	local run_json="$1" workflow_path="$2" source_sha="$3" repository="$4"
	local reason
	reason="$(jq -r \
		--arg workflow_path "$workflow_path" \
		--arg source_sha "$source_sha" \
		--arg repository "$repository" '
		if type != "object" then "run document is not an object"
		elif .repository.full_name != $repository then "run belongs to repository " + (.repository.full_name // "null")
		elif .path != $workflow_path then "run uses workflow " + (.path // "null") + " instead of " + $workflow_path
		elif .head_sha != $source_sha then "run head " + (.head_sha // "null") + " is not the candidate source"
		elif .status != "completed" then "run status is " + (.status // "null")
		elif .conclusion != "success" then "run conclusion is " + (.conclusion // "null")
		elif (.id | type != "number") then "run has no numeric id"
		elif (.run_attempt | type != "number") then "run has no numeric attempt"
		else ""
		end' "$run_json")"
	printf '%s' "$reason"
}

# Required jobs may be exact names or `Name (*)` for a matrix. A matrix
# entry requires at least one job and all of them must succeed.
required_jobs_reason() {
	local jobs_json="$1" required_file="$2"
	local line pattern count failed
	require_file "$required_file"
	jq -e '.jobs | type == "array"' "$jobs_json" >/dev/null || {
		printf 'jobs document has no jobs array'
		return
	}
	while IFS= read -r line; do
		[ -n "$line" ] || continue
		if [[ "$line" == *" (*)" ]]; then
			pattern="${line% (\*)} ("
			count="$(jq --arg prefix "$pattern" '[.jobs[] | select(.name | startswith($prefix))] | length' "$jobs_json")"
			failed="$(jq --arg prefix "$pattern" '[.jobs[] | select(.name | startswith($prefix)) | select(.status != "completed" or .conclusion != "success")] | length' "$jobs_json")"
			if [ "$count" -eq 0 ]; then
				printf 'required matrix job %s has no entries' "$line"
				return
			fi
		else
			count="$(jq --arg name "$line" '[.jobs[] | select(.name == $name)] | length' "$jobs_json")"
			failed="$(jq --arg name "$line" '[.jobs[] | select(.name == $name) | select(.status != "completed" or .conclusion != "success")] | length' "$jobs_json")"
			if [ "$count" -ne 1 ]; then
				printf 'required job %s occurs %s times' "$line" "$count"
				return
			fi
		fi
		if [ "$failed" -ne 0 ]; then
			printf 'required job %s did not succeed' "$line"
			return
		fi
	done <"$required_file"
	printf ''
}

gate_full_ci() {
	local dir="$1" evidence="$2" repository="$3" source_sha run_id run_attempt reason
	[ -f "$dir/ci-run.json" ] || { gate_result full_ci hold "ci-run.json is absent"; return; }
	source_sha="$(jq -r '.source_sha' "$evidence")"
	run_id="$(jq -r '.ci.run_id' "$evidence")"
	run_attempt="$(jq -r '.ci.run_attempt' "$evidence")"
	reason="$(run_mismatch_reason "$dir/ci-run.json" .github/workflows/ci.yml "$source_sha" "$repository")"
	[ -z "$reason" ] || { gate_result full_ci fail "$reason"; return; }
	jq -e --argjson id "$run_id" --argjson attempt "$run_attempt" \
		'.id == $id and .run_attempt == $attempt and .event == "push" and .head_branch == "master"' \
		"$dir/ci-run.json" >/dev/null ||
		{ gate_result full_ci fail "CI run does not match the evidence run id, attempt, event, and branch"; return; }
	gate_result full_ci pass "CI run $run_id attempt $run_attempt succeeded for the candidate source"
}

gate_heavy_integration() {
	local dir="$1" evidence="$2" reason required_json digest recorded
	[ -f "$dir/ci-jobs.json" ] || { gate_result heavy_integration hold "ci-jobs.json is absent"; return; }
	reason="$(required_jobs_reason "$dir/ci-jobs.json" "$SCRIPT_DIR/release-required-ci-jobs.txt")"
	[ -z "$reason" ] || { gate_result heavy_integration fail "$reason"; return; }
	required_json="$("$EVIDENCE_TOOL" required-jobs)"
	digest="$(jq -cS --argjson names "$required_json" '
		[.jobs[] | select(.name as $n | $names | index($n)) | {id: .id, name: .name, conclusion: .conclusion}]
		| sort_by(.name as $n | $names | index($n))' "$dir/ci-jobs.json" | sha256sum | awk '{print $1}')"
	recorded="$(jq -r '.ci.required_jobs_digest' "$evidence")"
	[ "$digest" = "$recorded" ] ||
		{ gate_result heavy_integration fail "required CI jobs differ from the jobs recorded in the candidate evidence"; return; }
	gate_result heavy_integration pass "all required CI jobs succeeded and match the evidence digest"
}

gate_slashing() {
	local dir="$1" evidence="$2" repository="$3" source_sha reason run_id
	[ -f "$dir/slashing-run.json" ] && [ -f "$dir/slashing-jobs.json" ] ||
		{ gate_result slashing hold "slashing run evidence is absent"; return; }
	source_sha="$(jq -r '.source_sha' "$evidence")"
	reason="$(run_mismatch_reason "$dir/slashing-run.json" .github/workflows/slashing-tests.yml "$source_sha" "$repository")"
	[ -z "$reason" ] || { gate_result slashing fail "$reason"; return; }
	reason="$(required_jobs_reason "$dir/slashing-jobs.json" "$REQUIRED_SLASHING_JOBS_FILE")"
	[ -z "$reason" ] || { gate_result slashing fail "$reason"; return; }
	run_id="$(jq -r '.id' "$dir/slashing-run.json")"
	gate_result slashing pass "slashing run $run_id succeeded for the candidate source with every required job"
}

# Shared checks for a gate document that must bind to the candidate.
candidate_binding_reason() {
	local doc="$1" evidence="$2" gate="$3"
	jq -r --slurpfile ev "$evidence" --arg gate "$gate" '
		$ev[0] as $e
		| if type != "object" then "document is not an object"
		elif .schema_version != 1 then "schema_version is " + ((.schema_version // "null") | tostring)
		elif .gate != $gate then "document gate is " + (.gate // "null") + " instead of " + $gate
		elif .source_sha != $e.source_sha then "document source " + (.source_sha // "null") + " is not the candidate source"
		elif .candidate_tag != $e.candidate_tag then "document candidate tag is " + (.candidate_tag // "null")
		elif .image_index_digest != ($e.images.docker_hub | split("@")[1]) then "document image digest " + (.image_index_digest // "null") + " is not the candidate image"
		elif (.workflow_run.id | type != "number") then "document has no workflow run id"
		elif (.workflow_run.attempt | type != "number") then "document has no workflow run attempt"
		elif .workflow_run.conclusion != "success" then "document workflow conclusion is " + (.workflow_run.conclusion // "null")
		else ""
		end' "$doc"
}

gate_oci_validation() {
	local dir="$1" evidence="$2" doc="$dir/oci-validation-evidence.json" reason
	[ -f "$doc" ] || { gate_result oci_validation hold "oci-validation-evidence.json is absent"; return; }
	reason="$(candidate_binding_reason "$doc" "$evidence" oci_validation)"
	[ -z "$reason" ] || { gate_result oci_validation fail "$reason"; return; }
	jq -e --slurpfile ev "$evidence" '
		.workflow_run.path == ".github/workflows/oci-validation.yml"
		and .mode == "candidate"
		and .system_integration_sha == $ev[0].system_integration_sha
		and (.required_jobs | type == "array" and length > 0)
		and ([.required_jobs[] | select(.conclusion != "success")] | length == 0)' "$doc" >/dev/null ||
		{ gate_result oci_validation fail "OCI validation did not run in candidate mode with the trusted system-integration pin and successful required jobs"; return; }
	gate_result oci_validation pass "OCI validation passed for the candidate image digest"
}

gate_soak_preflight() {
	local dir="$1" evidence="$2" doc="$dir/soak-evidence.json" reason
	[ -f "$doc" ] || { gate_result soak_preflight hold "soak-evidence.json is absent"; return; }
	reason="$(candidate_binding_reason "$doc" "$evidence" stability_soak)"
	[ -z "$reason" ] || { gate_result soak_preflight fail "$reason"; return; }
	jq -e '.preflight.status == "success"' "$doc" >/dev/null ||
		{ gate_result soak_preflight fail "the integration preflight did not succeed for the candidate"; return; }
	gate_result soak_preflight pass "integration preflight succeeded for the candidate source"
}

gate_stability_soak() {
	local dir="$1" evidence="$2" doc="$dir/soak-evidence.json" reason
	[ -f "$doc" ] || { gate_result stability_soak hold "soak-evidence.json is absent"; return; }
	reason="$(candidate_binding_reason "$doc" "$evidence" stability_soak)"
	[ -z "$reason" ] || { gate_result stability_soak fail "$reason"; return; }
	reason="$(jq -r --argjson duration "$SOAK_DURATION_SECONDS" '
		if .workflow_run.path != ".github/workflows/merge-recovery-soak.yml" then "soak ran from workflow " + (.workflow_run.path // "null")
		elif .soak_kind != "weekend" then "soak kind is " + (.soak_kind // "null")
		elif .requested_duration_seconds != $duration then "requested duration is " + ((.requested_duration_seconds // "null") | tostring) + " seconds"
		elif .completed != true then "the final completion marker is absent"
		elif .artifact_mode != "candidate" then "soak did not run in candidate artifact mode"
		elif (.retry_attempt | type != "number") then "retry_attempt is absent"
		elif .retry_attempt > 1 then "soak restarted more than once"
		elif .retry_attempt == 1 and .coverage_preserved != true then "the restart did not preserve full 60-hour coverage"
		else ""
		end' "$doc")"
	[ -z "$reason" ] || { gate_result stability_soak fail "$reason"; return; }
	gate_result stability_soak pass "60h stability soak completed on the candidate image digest"
}

gate_regression_verdict() {
	local dir="$1" evidence="$2" doc="$dir/verdict.json" review="$dir/maintainer-review.json"
	local source_sha verdict
	[ -f "$doc" ] || { gate_result regression_verdict hold "verdict.json is absent"; return; }
	source_sha="$(jq -r '.source_sha' "$evidence")"
	jq -e --arg sha "$source_sha" '.source_sha == $sha' "$doc" >/dev/null ||
		{ gate_result regression_verdict fail "verdict does not identify the candidate source"; return; }
	verdict="$(jq -r '.verdict' "$doc")"
	case "$verdict" in
	pass)
		gate_result regression_verdict pass "regression verdict is pass"
		;;
	regress)
		[ -f "$review" ] ||
			{ gate_result regression_verdict hold "regress verdict awaits documented maintainer review"; return; }
		jq -e --arg sha "$source_sha" --arg tag "$(jq -r '.candidate_tag' "$evidence")" '
			.source_sha == $sha
			and .candidate_tag == $tag
			and .verdict_accepted == true
			and (.reviewer | type == "string" and length > 0)
			and (.reference | type == "string" and length > 0)
			and (.reviewed_at | type == "string" and endswith("Z"))' "$review" >/dev/null ||
			{ gate_result regression_verdict fail "maintainer review does not accept the regress verdict for this candidate"; return; }
		gate_result regression_verdict pass "regress verdict accepted by maintainer review $(jq -r '.reference' "$review")"
		;;
	*)
		gate_result regression_verdict fail "regression verdict is $verdict"
		;;
	esac
}

gate_train_gates() {
	local evidence="$1" train_id
	train_id="$(jq -r '.train_id' "$evidence")"
	if [ "$train_id" = null ]; then
		gate_result train_gates pass "standard release has no train gates"
	else
		gate_result train_gates hold "train gate evaluation for $train_id is not implemented"
	fi
}

evaluate() {
	local dir="$1" repository="$2" output="$3"
	local evidence="$dir/release-evidence.json" gates status
	require_file "$evidence"
	"$EVIDENCE_TOOL" validate "$evidence"
	[ "$(jq -r '.publication_mode' "$evidence")" = canary ] ||
		fail "promotion requires canary evidence with published images"
	gates="$(jq -s '.' \
		<(gate_full_ci "$dir" "$evidence" "$repository") \
		<(gate_heavy_integration "$dir" "$evidence") \
		<(gate_slashing "$dir" "$evidence" "$repository") \
		<(gate_oci_validation "$dir" "$evidence") \
		<(gate_soak_preflight "$dir" "$evidence") \
		<(gate_stability_soak "$dir" "$evidence") \
		<(gate_regression_verdict "$dir" "$evidence") \
		<(gate_train_gates "$evidence"))"
	mkdir -p "$(dirname "$output")"
	jq -n \
		--arg candidate_tag "$(jq -r '.candidate_tag' "$evidence")" \
		--arg source_sha "$(jq -r '.source_sha' "$evidence")" \
		--arg target_version "$(jq -r '.target_version' "$evidence")" \
		--arg evidence_sha256 "$(sha256sum "$evidence" | awk '{print $1}')" \
		--argjson gates "$gates" '
		{
			schema_version: 1,
			candidate_tag: $candidate_tag,
			source_sha: $source_sha,
			target_version: $target_version,
			candidate_evidence_sha256: $evidence_sha256,
			gates: $gates,
			failed: ([$gates[] | select(.status == "fail")] | length > 0),
			held: ([$gates[] | select(.status == "hold")] | length > 0),
			promotable: ([$gates[] | select(.status != "pass")] | length == 0)
		}' >"$output"
	status="$(jq -r 'if .failed then "failed" elif .held then "held" else "promotable" end' "$output")"
	printf 'candidate %s is %s\n' "$(jq -r '.candidate_tag' "$output")" "$status" >&2
	case "$status" in
	promotable) return "$EXIT_PROMOTABLE" ;;
	held) return "$EXIT_HELD" ;;
	failed) return "$EXIT_FAILED" ;;
	esac
}

summarize() {
	local report="$1"
	require_file "$report"
	jq -r '
		"### Promotion gates for `" + .candidate_tag + "`",
		"",
		"| Gate | Status | Reason |",
		"|---|---|---|",
		(.gates[] | "| " + .id + " | " + .status + " | " + .reason + " |"),
		"",
		(if .promotable then "All gates pass. The candidate is promotable."
		 elif .failed then "A gate failed. No override can replace exact-SHA evidence."
		 else "Promotion is held until the missing evidence arrives." end)' "$report"
}

usage() {
	printf '%s\n' \
		"usage: $0 evaluate GATES_DIR REPOSITORY OUTPUT" \
		"       $0 summarize GATE_REPORT" \
		"" \
		"evaluate exits 0 when promotable, $EXIT_HELD when held, $EXIT_FAILED when a gate failed." >&2
	exit 2
}

command="${1:-}"
case "$command" in
evaluate)
	[ "$#" -eq 4 ] || usage
	evaluate "$2" "$3" "$4"
	;;
summarize)
	[ "$#" -eq 2 ] || usage
	summarize "$2"
	;;
*) usage ;;
esac

#!/usr/bin/env bash
set -euo pipefail

# Gate-evidence writer (docs/release-process.md section 8.1).
#
# A gate workflow that runs in candidate mode calls this script at the end
# of a successful run. The script binds the document to the candidate from
# release-evidence.json, so a workflow cannot publish evidence for a source
# or image other than the one it validated. release-gates.sh consumes the
# result; test-release-gate-evidence.sh proves the two agree.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EVIDENCE_TOOL="$SCRIPT_DIR/release-evidence.sh"

fail() {
	printf 'release gate evidence error: %s\n' "$1" >&2
	exit 1
}

require_file() {
	[ -f "$1" ] || fail "missing file: $1"
}

require_positive_integer() {
	[[ "$1" =~ ^[1-9][0-9]*$ ]] || fail "$2 must be a positive integer"
}

# Common envelope. RUN_JSON is the GitHub API document for the gate run
# itself; the envelope records its id, attempt, and workflow path.
envelope() {
	local gate="$1" evidence="$2" run_json="$3" workflow_path="$4"
	require_file "$evidence"
	require_file "$run_json"
	"$EVIDENCE_TOOL" validate "$evidence"
	[ "$(jq -r '.publication_mode' "$evidence")" = canary ] ||
		fail "gate evidence requires canary candidate evidence"
	jq -e --arg path "$workflow_path" '.path == $path' "$run_json" >/dev/null ||
		fail "gate run does not use workflow $workflow_path"
	require_positive_integer "$(jq -r '.id' "$run_json")" "gate run id"
	require_positive_integer "$(jq -r '.run_attempt' "$run_json")" "gate run attempt"
	# The document records the SHA-256 of the exact evidence file the run
	# resolved before testing. The publishing job must pass that same file
	# (carried from the test job as a run artifact), never a fresh download
	# of the mutable release asset, and the promotion controller requires
	# this digest to equal the evidence it evaluates.
	jq -n \
		--arg gate "$gate" \
		--arg source_sha "$(jq -r '.source_sha' "$evidence")" \
		--arg candidate_tag "$(jq -r '.candidate_tag' "$evidence")" \
		--arg image_index_digest "$(jq -r '.images.ocir_index_digest' "$evidence")" \
		--arg evidence_sha256 "$(sha256sum "$evidence" | awk '{print $1}')" \
		--argjson run_id "$(jq -r '.id' "$run_json")" \
		--argjson run_attempt "$(jq -r '.run_attempt' "$run_json")" \
		--arg workflow_path "$workflow_path" '
		{
			schema_version: 1,
			gate: $gate,
			source_sha: $source_sha,
			candidate_tag: $candidate_tag,
			image_index_digest: $image_index_digest,
			candidate_evidence_sha256: $evidence_sha256,
			workflow_run: {
				id: $run_id,
				attempt: $run_attempt,
				path: $workflow_path,
				conclusion: "success"
			}
		}'
}

write_oci_validation() {
	local evidence="$1" run_json="$2" jobs_json="$3" output="$4"
	local base required
	require_file "$jobs_json"
	base="$(envelope oci_validation "$evidence" "$run_json" .github/workflows/oci-validation.yml)"
	# The two architecture summary jobs are the required conclusions. Their
	# names carry the reusable-workflow prefix in the run's job listing.
	required="$(jq -c '
		[.jobs[] | select(.name | test("Integration Tests \\((amd64|arm64)\\)$"))
			| {name: .name, conclusion: .conclusion}]
		| sort_by(.name)' "$jobs_json")"
	[ "$(jq 'length' <<<"$required")" -eq 2 ] ||
		fail "OCI validation run must report exactly the amd64 and arm64 integration summary jobs"
	jq -e '[.[] | select(.conclusion != "success")] | length == 0' <<<"$required" >/dev/null ||
		fail "a required OCI validation job did not succeed"
	mkdir -p "$(dirname "$output")"
	jq \
		--arg pin "$(jq -r '.system_integration_sha' "$evidence")" \
		--argjson required "$required" '
		. + {
			mode: "candidate",
			system_integration_sha: $pin,
			required_jobs: $required
		}' <<<"$base" >"$output"
}

# SOAK_RESULT_JSON is produced by the soak workflow from its own run state:
#   {
#     "soak_kind": "weekend",
#     "requested_duration_seconds": 216000,
#     "completed": true,
#     "artifact_mode": "candidate",
#     "retry_attempt": 0,
#     "coverage_preserved": true,
#     "preflight": {"status": "success"}
#   }
write_stability_soak() {
	local evidence="$1" run_json="$2" soak_result_json="$3" output="$4"
	local base
	require_file "$soak_result_json"
	base="$(envelope stability_soak "$evidence" "$run_json" .github/workflows/merge-recovery-soak.yml)"
	jq -e '
		.soak_kind == "weekend"
		and .requested_duration_seconds == 216000
		and .completed == true
		and .artifact_mode == "candidate"
		and (.retry_attempt | type == "number" and . >= 0 and . <= 1)
		and (.coverage_preserved | type == "boolean")
		and (.preflight.status | type == "string")' "$soak_result_json" >/dev/null ||
		fail "soak result does not describe a completed candidate-mode 60h stability soak"
	mkdir -p "$(dirname "$output")"
	jq --slurpfile result "$soak_result_json" '. + $result[0]' <<<"$base" >"$output"
}

write_verdict() {
	local evidence="$1" verdict="$2" output="$3"
	require_file "$evidence"
	"$EVIDENCE_TOOL" validate "$evidence"
	case "$verdict" in
	pass | regress | inconclusive) ;;
	*) fail "verdict must be pass, regress, or inconclusive" ;;
	esac
	mkdir -p "$(dirname "$output")"
	jq -n \
		--arg source_sha "$(jq -r '.source_sha' "$evidence")" \
		--arg candidate_tag "$(jq -r '.candidate_tag' "$evidence")" \
		--arg verdict "$verdict" '
		{schema_version: 1, source_sha: $source_sha, candidate_tag: $candidate_tag, verdict: $verdict}' >"$output"
}

write_test_net_candidate() {
	local evidence="$1" run_json="$2" soak_doc="$3" verdict_json="$4" output="$5"
	local review="${6:-}" permission="${7:-}"
	local base verdict reference=null
	require_file "$soak_doc"
	require_file "$verdict_json"
	base="$(envelope test_net_candidate "$evidence" "$run_json" .github/workflows/merge-recovery-soak.yml)"
	jq -e --slurpfile ev "$evidence" --slurpfile run "$run_json" \
		--arg evidence_sha "$(sha256sum "$evidence" | awk '{print $1}')" '
		.schema_version == 1
		and .gate == "stability_soak"
		and .source_sha == $ev[0].source_sha
		and .candidate_tag == $ev[0].candidate_tag
		and .candidate_evidence_sha256 == $evidence_sha
		and .workflow_run.id == $run[0].id
		and .workflow_run.attempt == $run[0].run_attempt
		and .soak_kind == "weekend"
		and .requested_duration_seconds == 216000
		and .completed == true
		and .artifact_mode == "candidate"
		and .preflight.status == "success"
		and (.retry_attempt == 0 or (.retry_attempt == 1 and .coverage_preserved == true))' "$soak_doc" >/dev/null ||
		fail "soak document does not record a passing 60h stability soak of this candidate in this run"
	jq -e --slurpfile ev "$evidence" '.source_sha == $ev[0].source_sha' "$verdict_json" >/dev/null ||
		fail "verdict does not identify the candidate source"
	verdict="$(jq -r '.verdict' "$verdict_json")"
	case "$verdict" in
	pass) ;;
	regress)
		[ -n "$review" ] && [ -n "$permission" ] ||
			fail "a regress verdict needs maintainer-review.json and the reviewer permission document"
		require_file "$review"
		require_file "$permission"
		jq -e --slurpfile ev "$evidence" '
			.source_sha == $ev[0].source_sha
			and .candidate_tag == $ev[0].candidate_tag
			and .verdict_accepted == true
			and (.reviewer | type == "string" and length > 0)
			and (.reference | type == "string" and length > 0)
			and (.reviewed_at | type == "string" and endswith("Z"))' "$review" >/dev/null ||
			fail "maintainer review does not accept the regress verdict for this candidate"
		jq -e --slurpfile review "$review" '
			.login == $review[0].reviewer
			and (.permission == "admin" or .permission == "maintain")' "$permission" >/dev/null ||
			fail "the reviewer does not hold maintain or admin permission"
		reference="$(jq '.reference' "$review")"
		;;
	*) fail "verdict $verdict cannot make a test net candidate" ;;
	esac
	mkdir -p "$(dirname "$output")"
	jq \
		--arg consensus_model "$(jq -r '.consensus_model // "cbc-casper"' "$evidence")" \
		--arg soak_sha "$(sha256sum "$soak_doc" | awk '{print $1}')" \
		--arg verdict "$verdict" \
		--argjson reference "$reference" '
		. + {
			consensus_model: $consensus_model,
			soak_evidence_sha256: $soak_sha,
			verdict: $verdict,
			maintainer_review_reference: $reference
		}' <<<"$base" >"$output"
}

write_candidate_marker() {
	local evidence="$1" output="$2"
	require_file "$evidence"
	"$EVIDENCE_TOOL" validate "$evidence"
	mkdir -p "$(dirname "$output")"
	jq '{candidate_tag: .candidate_tag, source_sha: .source_sha}' "$evidence" >"$output"
}

usage() {
	printf '%s\n' \
		"usage: $0 oci-validation EVIDENCE_JSON RUN_JSON JOBS_JSON OUTPUT" \
		"       $0 stability-soak EVIDENCE_JSON RUN_JSON SOAK_RESULT_JSON OUTPUT" \
		"       $0 verdict EVIDENCE_JSON VERDICT OUTPUT" \
		"       $0 test-net-candidate EVIDENCE_JSON RUN_JSON SOAK_EVIDENCE_JSON VERDICT_JSON OUTPUT [REVIEW_JSON PERMISSION_JSON]" \
		"       $0 candidate-marker EVIDENCE_JSON OUTPUT" >&2
	exit 2
}

command="${1:-}"
case "$command" in
oci-validation)
	[ "$#" -eq 5 ] || usage
	write_oci_validation "$2" "$3" "$4" "$5"
	;;
stability-soak)
	[ "$#" -eq 5 ] || usage
	write_stability_soak "$2" "$3" "$4" "$5"
	;;
verdict)
	[ "$#" -eq 4 ] || usage
	write_verdict "$2" "$3" "$4"
	;;
test-net-candidate)
	[ "$#" -eq 6 ] || [ "$#" -eq 8 ] || usage
	write_test_net_candidate "$2" "$3" "$4" "$5" "$6" "${7:-}" "${8:-}"
	;;
candidate-marker)
	[ "$#" -eq 3 ] || usage
	write_candidate_marker "$2" "$3"
	;;
*) usage ;;
esac

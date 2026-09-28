#!/usr/bin/env bash
set -euo pipefail

# Deployment Train validator and setup planner
# (docs/release-process.md section 13).
#
# The script reads a manifest and JSON documents only. The workflow fetches
# the documents from the GitHub API and calls the commands in order; the
# script never contacts the network, so every decision is reproducible
# from the inputs.
#
# Inputs directory for `validate-stack` (one file per fetched object):
#   pull-<N>.json        GET pulls/<N> for every member (and the top)
#   compare-<L>-<U>.json GET compare/<lower_head>...<upper_head> for each
#                        adjacent member pair (status ahead|identical means
#                        the lower head is an ancestor of the upper head)
#   merge-<N>.json       GET commits/<merge_commit_sha> for a merged member
#   reach-<N>.json       GET compare/<merge_commit_sha>...<integration_branch>
#                        for a merged member (status ahead|identical means the
#                        merge commit is reachable from integration_branch)
#
# Every JSON input is parsed with jq -e, so an empty or malformed file fails
# the validation the same way a missing file does.
#
# Outputs `train-record.json`: the reviewed intent plus every member head
# observed at setup. Section 13.3 re-validates the chain against this record
# after each member merge, and the promotion controller verifies every
# recorded head at promotion.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EVIDENCE_TOOL="$SCRIPT_DIR/release-evidence.sh"

fail() {
	printf 'release train error: %s\n' "$1" >&2
	exit 1
}

require_file() {
	[ -f "$1" ] || fail "missing file: $1"
}

manifest_json() {
	require_file "$1"
	ruby -ryaml -rjson -e '
		doc = YAML.safe_load(File.read(ARGV[0]), permitted_classes: [], aliases: false)
		abort("manifest must be a mapping") unless doc.is_a?(Hash)
		puts JSON.generate(doc)' "$1" || fail "manifest is not valid YAML"
}

# Normalize a manifest: apply defaults, validate every field, and return the
# normalized JSON. Version 1 manifests have no stack block. Version 2
# manifests add stack and publishing.
validate_manifest() {
	local manifest="$1" json
	json="$(manifest_json "$manifest")"
	jq -e '
		type == "object"
		and (.schema_version == 1 or .schema_version == 2)
		and (.id | type == "string" and test("^[a-z0-9]+(-[a-z0-9]+)*$"))
		and (.state | IN("proposed", "active", "soaking", "held", "promoted", "cancelled"))
		and (.target_version | type == "string" and test("^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$"))
		and (.pull_request | type == "number" and . > 0 and floor == .)
		and (.head_sha | type == "string" and test("^[0-9a-f]{40}$"))
		and (.base_branch | type == "string" and length > 0)
		and (.required_gates | type == "array")
		and ([.required_gates[] | select(
			(.id | type == "string" and test("^[a-z0-9]+(-[a-z0-9]+)*$"))
			and (.workflow | type == "string" and test("^[A-Za-z0-9._-]+\\.yml$"))
			and (.job | type == "string" and length > 0)
			and (.binds_image_digest | type == "boolean") | not)] | length == 0)
		and ((if has("publishing") then .publishing else true end) | type == "boolean")
		and (.schema_version == 2 or (has("stack") | not))
		and (.schema_version == 2 or (has("publishing") | not))
		and ((has("stack") | not) or (
			(.stack | type == "object")
			and ((.stack.integration_branch // "dev") | type == "string" and length > 0)
			and (.stack.members | type == "array" and length >= 2)
			and ([.stack.members[] | select((.pull_request | type == "number" and . > 0 and floor == .) | not)] | length == 0)
			and ((.stack.members | map(.pull_request) | unique | length) == (.stack.members | length))
			and (.stack.members[-1].pull_request == .pull_request)
			and ((.stack.head_pull_request // .pull_request) == .pull_request)))
	' <<<"$json" >/dev/null || fail "manifest schema validation failed"
	jq '
		.publishing = (if has("publishing") then .publishing else true end)
		| if has("stack") then .stack.integration_branch = (.stack.integration_branch // "dev") else . end' <<<"$json"
}

validate_top_merge() {
	local manifest_json="$1" inputs="$2" output="$3"
	local n pull head base merge merge_base
	n="$(jq -r '.pull_request' <<<"$manifest_json")"
	pull="$inputs/pull-$n.json"
	require_file "$pull"
	head="$(jq -r '.head.sha // empty' "$pull")"
	base="$(jq -r '.base.sha // empty' "$pull")"
	merge="$(jq -r '.merge_commit_sha // empty' "$pull")"
	[[ "$head" =~ ^[0-9a-f]{40}$ ]] || fail "top member #$n has no full head SHA"
	[[ "$base" =~ ^[0-9a-f]{40}$ ]] || fail "top member #$n has no full base SHA"
	[[ "$merge" =~ ^[0-9a-f]{40}$ ]] || fail "top member #$n has no full synthetic merge SHA"
	[ "$head" = "$(jq -r '.head_sha' <<<"$manifest_json")" ] ||
		fail "manifest head_sha $(jq -r '.head_sha' <<<"$manifest_json") is not the head of the top member ($head)"
	require_file "$inputs/top-merge.json"
	jq -e --arg merge "$merge" --arg head "$head" '
		type == "object"
		and .sha == $merge
		and (.parents | length == 2)
		and (.parents[0].sha | type == "string" and test("^[0-9a-f]{40}$"))
		and .parents[1].sha == $head' "$inputs/top-merge.json" >/dev/null ||
		fail "top member #$n synthetic merge $merge does not join the observed head"
	merge_base="$(jq -r '.parents[0].sha' "$inputs/top-merge.json")"
	require_file "$inputs/top-base.json"
	jq -e '.status == "ahead" or .status == "identical"' "$inputs/top-base.json" >/dev/null ||
		fail "top member #$n logical base $base is not an ancestor of synthetic base $merge_base"
	mkdir -p "$(dirname "$output")"
	jq -n --argjson n "$n" --arg head "$head" --arg base "$base" --arg merge_base "$merge_base" --arg merge "$merge" \
		'{head_pull_request: $n, head_sha: $head, base_sha: $base, merge_base_sha: $merge_base, merge_sha: $merge}' >"$output"
}

# Section 13.2 steps 3 to 6 from the fetched API documents.
validate_stack() {
	local manifest_json="$1" inputs="$2" output="$3"
	local members integration top_sha n lower upper prev_branch prev_merged base integration_base
	local record_members="[]" merged count
	jq -e 'has("stack")' <<<"$manifest_json" >/dev/null || fail "validate-stack requires a stack manifest"
	integration="$(jq -r '.stack.integration_branch' <<<"$manifest_json")"
	top_sha="$(jq -r '.head_sha' <<<"$manifest_json")"
	members="$(jq -r '.stack.members[].pull_request' <<<"$manifest_json")"
	prev_branch="$integration"
	prev_merged=true
	lower=""
	count=0
	while IFS= read -r n; do
		count=$((count + 1))
		require_file "$inputs/pull-$n.json"
		# Step 4: member chain. The bottom member targets integration_branch;
		# every later member targets the branch of the preceding member.
		# After the preceding member merges and its branch is deleted, GitHub
		# retargets the next member to integration_branch; that base is then
		# also accepted, and steps 5 and 6 keep the chain honest.
		jq -e --arg base "$prev_branch" --arg integration "$integration" --argjson prev_merged "$prev_merged" '
			type == "object"
			and (.number | type == "number")
			and (.state == "open" or .merged == true)
			and (.base.ref == $base or ($prev_merged and .base.ref == $integration))' "$inputs/pull-$n.json" >/dev/null ||
			fail "member #$n must be open or merged and must target $prev_branch$([ "$prev_merged" = true ] && [ "$prev_branch" != "$integration" ] && printf ' (or %s after the preceding member merged)' "$integration")"
		base="$(jq -r '.base.ref' "$inputs/pull-$n.json")"
		if [ "$count" -eq 1 ]; then
			integration_base="$(jq -r '.base.sha // empty' "$inputs/pull-$n.json")"
			[[ "$integration_base" =~ ^[0-9a-f]{40}$ ]] || fail "bottom member #$n has no full integration base SHA"
		fi
		upper="$(jq -r '.head.sha' "$inputs/pull-$n.json")"
		[[ "$upper" =~ ^[0-9a-f]{40}$ ]] || fail "member #$n has no full head SHA"
		# Step 5: head ancestry. The lower head must be an ancestor of this
		# member's head.
		if [ -n "$lower" ]; then
			require_file "$inputs/compare-$lower-$upper.json"
			jq -e '.status == "ahead" or .status == "identical"' "$inputs/compare-$lower-$upper.json" >/dev/null ||
				fail "member #$n head $upper does not contain the preceding member head $lower"
		fi
		# Step 6: merged-member topology. A merged member must have a true
		# merge commit whose second parent is its head, and that merge commit
		# must be reachable from integration_branch.
		merged="$(jq -r '.merged // false' "$inputs/pull-$n.json")"
		if [ "$merged" = true ]; then
			local merge_sha
			merge_sha="$(jq -r '.merge_commit_sha // empty' "$inputs/pull-$n.json")"
			[[ "$merge_sha" =~ ^[0-9a-f]{40}$ ]] || fail "merged member #$n has no merge commit"
			require_file "$inputs/merge-$n.json"
			jq -e --arg sha "$merge_sha" --arg head "$upper" '
				.sha == $sha and (.parents | length == 2) and .parents[1].sha == $head' "$inputs/merge-$n.json" >/dev/null ||
				fail "merged member #$n did not merge with a true merge commit whose second parent is its head"
			require_file "$inputs/reach-$n.json"
			jq -e '.status == "ahead" or .status == "identical"' "$inputs/reach-$n.json" >/dev/null ||
				fail "merged member #$n merge commit $merge_sha is not reachable from $integration"
		fi
		record_members="$(jq --argjson n "$n" --arg head "$upper" --arg base "$base" --argjson merged "$merged" \
			'. + [{pull_request: $n, head_sha: $head, base: $base, merged: $merged}]' <<<"$record_members")"
		prev_branch="$(jq -r '.head.ref' "$inputs/pull-$n.json")"
		prev_merged="$merged"
		lower="$upper"
	done <<<"$members"
	[ "$lower" = "$top_sha" ] || fail "manifest head_sha $top_sha is not the head of the top member ($lower)"
	local top_record
	top_record="$(mktemp)"
	validate_top_merge "$manifest_json" "$inputs" "$top_record"
	require_file "$inputs/integration-base.json"
	jq -e '.status == "ahead" or .status == "identical"' "$inputs/integration-base.json" >/dev/null ||
		fail "integration base $integration_base is not an ancestor of the top synthetic base"
	mkdir -p "$(dirname "$output")"
	jq --argjson members "$record_members" --arg integration_base "$integration_base" --slurpfile top "$top_record" '
		{
			schema_version: 1,
			train_id: .id,
			state: .state,
			target_version: .target_version,
			publishing: .publishing,
			base_branch: .base_branch,
			integration_branch: .stack.integration_branch,
			head_sha: .head_sha,
			head_pull_request: .pull_request,
			base_sha: $top[0].base_sha,
			integration_base_sha: $integration_base,
			merge_base_sha: $top[0].merge_base_sha,
			merge_sha: $top[0].merge_sha,
			members: $members,
			required_gates: .required_gates
		}' <<<"$manifest_json" >"$output"
	rm -f "$top_record"
	printf 'stack of %s members verified; head %s\n' "$count" "$top_sha" >&2
}

# Section 13.2 step 7: the exact merge source must carry target_version.
# Step 8 (reservation) runs for publishing trains only.
validate_version() {
	local manifest_json="$1" source_dir="$2" manifests_dir="$3"
	local target publishing inspect source_version eligible id
	target="$(jq -r '.target_version' <<<"$manifest_json")"
	publishing="$(jq -r '.publishing' <<<"$manifest_json")"
	id="$(jq -r '.id' <<<"$manifest_json")"
	inspect="$("$EVIDENCE_TOOL" inspect-source "$source_dir")"
	source_version="$(jq -r '.target_version' <<<"$inspect")"
	[ "$source_version" = "$target" ] ||
		fail "source version $source_version differs from target_version $target"
	if [ "$publishing" = true ]; then
		eligible="$(jq -r '.release_eligible' <<<"$inspect")"
		[ "$eligible" = true ] ||
			fail "target version $target is not greater than the highest stable version $(jq -r '.highest_stable_version' <<<"$inspect")"
		if [ -d "$manifests_dir" ]; then
			local other
			for other in "$manifests_dir"/*.yml; do
				[ -f "$other" ] || continue
				local oj
				oj="$(manifest_json "$other")"
				[ "$(jq -r '.id' <<<"$oj")" = "$id" ] && continue
				if jq -e --arg v "$target" '.target_version == $v and (.state | IN("active", "soaking", "held"))' <<<"$oj" >/dev/null; then
					fail "version $target is reserved by active train $(jq -r '.id' <<<"$oj")"
				fi
			done
		fi
	fi
	printf 'source version %s matches target_version\n' "$target" >&2
}

plan_ci() {
	local runs_json="$1" merge_sha="$2" repository="$3" default_branch="$4" control_sha="$5"
	local title id
	require_file "$runs_json"
	title="CI [exact merge $merge_sha]"
	id="$(jq -r --arg title "$title" --arg repo "$repository" --arg branch "$default_branch" --arg control "$control_sha" '
		def trusted:
			.path == ".github/workflows/ci.yml"
			and .event == "workflow_dispatch"
			and .display_title == $title
			and .head_branch == $branch
			and .head_sha == $control
			and .repository.full_name == $repo
			and .head_repository.full_name == $repo;
		[.workflow_runs[] | select(trusted and .status == "completed" and .conclusion == "success")]
		| sort_by(.run_number) | last | .id // empty' "$runs_json")"
	if [ -n "$id" ]; then
		printf '%s\n' "$id"
		return
	fi
	id="$(jq -r --arg title "$title" --arg repo "$repository" --arg branch "$default_branch" --arg control "$control_sha" '
		def trusted:
			.path == ".github/workflows/ci.yml"
			and .event == "workflow_dispatch"
			and .display_title == $title
			and .head_branch == $branch
			and .head_sha == $control
			and .repository.full_name == $repo
			and .head_repository.full_name == $repo;
		[.workflow_runs[] | select(trusted and (.status == "queued" or .status == "in_progress"))]
		| sort_by(.run_number) | last | .id // empty' "$runs_json")"
	if [ -n "$id" ]; then printf 'wait:%s\n' "$id"; else printf 'dispatch\n'; fi
}

validate_current_stack() {
	local record="$1" current="$2"
	require_file "$record"
	require_file "$current"
	jq -e --slurpfile current "$current" '
		{train_id, target_version, head_sha, head_pull_request, integration_branch,
		 integration_base_sha, base_sha, merge_base_sha, merge_sha, members}
		== ($current[0] | {train_id, target_version, head_sha, head_pull_request, integration_branch,
		 integration_base_sha, base_sha, merge_base_sha, merge_sha, members})' "$record" >/dev/null ||
		fail "stack changed while exact-merge CI ran"
}

validate_ci_evidence() {
	local target="$1" jobs="$2" merge="$3" top="$4" head="$5" base="$6" merge_base="$7" run_id="$8" run_attempt="$9" repository="${10}"
	require_file "$target"
	require_file "$jobs"
	jq -e --arg merge "$merge" --argjson top "$top" --arg head "$head" --arg base "$base" \
		--arg merge_base "$merge_base" --argjson run "$run_id" --argjson attempt "$run_attempt" --arg repo "$repository" '
		type == "object"
		and .schema_version == 1
		and .repository == $repo
		and .workflow == ".github/workflows/ci.yml"
		and .event == "workflow_dispatch"
		and .target_sha == $merge
		and .top_pull_request == $top
		and .head_sha == $head
		and .base_sha == $base
		and .merge_base_sha == $merge_base
		and .run_id == $run
		and .run_attempt == $attempt' "$target" >/dev/null ||
		fail "CI target evidence does not bind run $run_id attempt $run_attempt to merge $merge"
	jq -e '
		def aggregator($arch):
			[.jobs[] | select(.name == ("Integration Tests (" + $arch + ")"))]
			| length == 1 and .[0].status == "completed" and .[0].conclusion == "success";
		type == "object" and (.jobs | type == "array")
		and aggregator("amd64") and aggregator("arm64")' "$jobs" >/dev/null ||
		fail "CI run does not contain one successful Heavy Pipeline aggregator for each architecture"
}

summarize() {
	local record="$1"
	require_file "$record"
	jq -r '
		"### Deployment Train `" + .train_id + "` (" + (if .publishing then "publishing" else "rehearsal, non-publishing" end) + ")",
		"",
		"- Target version: `" + .target_version + "`",
		"- Head: `" + .head_sha + "` (#" + (.head_pull_request | tostring) + ")",
		"- Exact merge: `" + .merge_sha + "` onto `" + .merge_base_sha + "` (PR base `" + .base_sha + "`, integration base `" + .integration_base_sha + "`)",
		"- Integration branch: `" + .integration_branch + "` → base `" + .base_branch + "`",
		"",
		"| Member | Head | Base | Merged |",
		"|---|---|---|---|",
		(.members[] | "| #" + (.pull_request | tostring) + " | `" + .head_sha[0:12] + "` | `" + .base + "` | " + (.merged | tostring) + " |")' "$record"
}

usage() {
	printf '%s\n' \
		"usage: $0 manifest-json MANIFEST_YML" \
		"       $0 validate-manifest MANIFEST_YML" \
		"       $0 validate-stack MANIFEST_JSON_FILE INPUTS_DIR OUTPUT" \
		"       $0 validate-top-merge MANIFEST_JSON_FILE INPUTS_DIR OUTPUT" \
		"       $0 validate-version MANIFEST_JSON_FILE SOURCE_DIR MANIFESTS_DIR" \
		"       $0 validate-current-stack TRAIN_RECORD CURRENT_RECORD" \
		"       $0 plan-ci RUNS_JSON MERGE_SHA REPOSITORY DEFAULT_BRANCH CONTROL_SHA" \
		"       $0 validate-ci-evidence TARGET_JSON JOBS_JSON MERGE_SHA TOP_PR HEAD_SHA BASE_SHA MERGE_BASE_SHA RUN_ID RUN_ATTEMPT REPOSITORY" \
		"       $0 summarize TRAIN_RECORD" >&2
	exit 2
}

command="${1:-}"
case "$command" in
manifest-json)
	[ "$#" -eq 2 ] || usage
	manifest_json "$2"
	;;
validate-manifest)
	[ "$#" -eq 2 ] || usage
	validate_manifest "$2"
	;;
validate-stack)
	[ "$#" -eq 4 ] || usage
	require_file "$2"
	validate_stack "$(cat "$2")" "$3" "$4"
	;;
validate-top-merge)
	[ "$#" -eq 4 ] || usage
	require_file "$2"
	validate_top_merge "$(cat "$2")" "$3" "$4"
	;;
validate-version)
	[ "$#" -eq 4 ] || usage
	require_file "$2"
	validate_version "$(cat "$2")" "$3" "$4"
	;;
validate-current-stack)
	[ "$#" -eq 3 ] || usage
	validate_current_stack "$2" "$3"
	;;
plan-ci)
	[ "$#" -eq 6 ] || usage
	plan_ci "$2" "$3" "$4" "$5" "$6"
	;;
validate-ci-evidence)
	[ "$#" -eq 11 ] || usage
	validate_ci_evidence "$2" "$3" "$4" "$5" "$6" "$7" "$8" "$9" "${10}" "${11}"
	;;
summarize)
	[ "$#" -eq 2 ] || usage
	summarize "$2"
	;;
*) usage ;;
esac

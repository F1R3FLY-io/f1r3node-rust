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

# Section 13.2 steps 3 to 6 from the fetched API documents.
validate_stack() {
	local manifest_json="$1" inputs="$2" output="$3"
	local members integration top_sha n lower upper prev_branch
	local record_members="[]" merged count
	jq -e 'has("stack")' <<<"$manifest_json" >/dev/null || fail "validate-stack requires a stack manifest"
	integration="$(jq -r '.stack.integration_branch' <<<"$manifest_json")"
	top_sha="$(jq -r '.head_sha' <<<"$manifest_json")"
	members="$(jq -r '.stack.members[].pull_request' <<<"$manifest_json")"
	prev_branch="$integration"
	lower=""
	count=0
	while IFS= read -r n; do
		count=$((count + 1))
		require_file "$inputs/pull-$n.json"
		# Step 4: member chain. The bottom member targets integration_branch;
		# every later member targets the branch of the preceding member.
		jq -e --arg base "$prev_branch" '
			type == "object"
			and (.number | type == "number")
			and (.state == "open" or .merged == true)
			and .base.ref == $base' "$inputs/pull-$n.json" >/dev/null ||
			fail "member #$n must be open or merged and must target $prev_branch"
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
		# merge commit on integration_branch whose second parent is its head.
		merged="$(jq -r '.merged' "$inputs/pull-$n.json")"
		if [ "$merged" = true ]; then
			local merge_sha
			merge_sha="$(jq -r '.merge_commit_sha // empty' "$inputs/pull-$n.json")"
			[[ "$merge_sha" =~ ^[0-9a-f]{40}$ ]] || fail "merged member #$n has no merge commit"
			require_file "$inputs/merge-$n.json"
			jq -e --arg sha "$merge_sha" --arg head "$upper" '
				.sha == $sha and (.parents | length == 2) and .parents[1].sha == $head' "$inputs/merge-$n.json" >/dev/null ||
				fail "merged member #$n did not merge with a true merge commit whose second parent is its head"
		fi
		record_members="$(jq --argjson n "$n" --arg head "$upper" --arg base "$prev_branch" --argjson merged "$merged" \
			'. + [{pull_request: $n, head_sha: $head, base: $base, merged: $merged}]' <<<"$record_members")"
		prev_branch="$(jq -r '.head.ref' "$inputs/pull-$n.json")"
		lower="$upper"
	done <<<"$members"
	[ "$lower" = "$top_sha" ] || fail "manifest head_sha $top_sha is not the head of the top member ($lower)"
	mkdir -p "$(dirname "$output")"
	jq --argjson members "$record_members" '
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
			members: $members,
			required_gates: .required_gates
		}' <<<"$manifest_json" >"$output"
	printf 'stack of %s members verified; head %s\n' "$count" "$top_sha" >&2
}

# Section 13.2 step 7: the source at head_sha must carry target_version.
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
		fail "source version $source_version at head_sha differs from target_version $target"
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

# Section 13.2 step 9: choose the CI evidence. RUNS_JSON is the list of
# ci.yml runs for head_sha from any event. Prints the newest successful
# run id, or "dispatch" when none exists.
plan_ci() {
	local runs_json="$1" head_sha="$2" repository="$3" id
	require_file "$runs_json"
	id="$(jq -r --arg sha "$head_sha" --arg repo "$repository" '
		[.workflow_runs[]
			| select(.head_sha == $sha and .path == ".github/workflows/ci.yml"
				and .status == "completed" and .conclusion == "success"
				and .repository.full_name == $repo)]
		| sort_by(.run_number) | last | .id // empty' "$runs_json")"
	if [ -n "$id" ]; then printf '%s\n' "$id"; else printf 'dispatch\n'; fi
}

summarize() {
	local record="$1"
	require_file "$record"
	jq -r '
		"### Deployment Train `" + .train_id + "` (" + (if .publishing then "publishing" else "rehearsal, non-publishing" end) + ")",
		"",
		"- Target version: `" + .target_version + "`",
		"- Head: `" + .head_sha + "` (#" + (.head_pull_request | tostring) + ")",
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
		"       $0 validate-version MANIFEST_JSON_FILE SOURCE_DIR MANIFESTS_DIR" \
		"       $0 plan-ci RUNS_JSON HEAD_SHA REPOSITORY" \
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
validate-version)
	[ "$#" -eq 4 ] || usage
	require_file "$2"
	validate_version "$(cat "$2")" "$3" "$4"
	;;
plan-ci)
	[ "$#" -eq 4 ] || usage
	plan_ci "$2" "$3" "$4"
	;;
summarize)
	[ "$#" -eq 2 ] || usage
	summarize "$2"
	;;
*) usage ;;
esac

#!/usr/bin/env bash
set -euo pipefail

# Stable promotion planner (docs/release-process.md section 11).
#
# The script decides what promotion must create and verifies what already
# exists. It never publishes. The workflow executes the plan with gh and
# docker, so every privileged action is a one-line consequence of a
# reviewed, reproducible JSON document.
#
# `plan` reads:
#   release-evidence.json   candidate evidence (publication_mode canary)
#   gate-report.json        output of release-gates.sh evaluate
#   stable-state.json       what the workflow observed before acting:
#     {
#       "stable_tags": ["v0.4.45", ...],
#       "stable_tag": null | {"sha": "<commit the tag resolves to>"},
#       "stable_release": null | {"prerelease": false, "assets": ["..."]},
#       "registry": {"stable_tag_digest": null | "sha256:...",
#                    "latest_digest": null | "sha256:..."},
#       "ocir": {"stable_tag_digest": null | "sha256:...",
#                "latest_digest": null | "sha256:..."}
#     }
#
# OCIR is the canonical registry for candidate gates and Docker Hub is the
# public mirror. Both receive the stable tag and the latest alias. The OCIR
# repository path never enters a plan or evidence document; the workflow
# resolves it from secrets and the plan carries only digests.
#
# Every existing object must already match the candidate. A mismatch stops
# promotion (section 4.4 and section 15). A matching object is skipped, which
# makes a resumed promotion idempotent.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EVIDENCE_TOOL="$SCRIPT_DIR/release-evidence.sh"

fail() {
	printf 'promote release error: %s\n' "$1" >&2
	exit 1
}

require_file() {
	[ -f "$1" ] || fail "missing file: $1"
}

require_sha() {
	[[ "$1" =~ ^[0-9a-f]{40}$ ]] || fail "$2 must be a full lowercase commit SHA"
}

require_digest() {
	[[ "$1" =~ ^sha256:[0-9a-f]{64}$ ]] || fail "$2 must be a sha256 digest"
}

next_patch_version() {
	local version="$1" major minor patch
	IFS=. read -r major minor patch <<<"$version"
	printf '%s.%s.%s\n' "$major" "$minor" "$((patch + 1))"
}

plan() {
	local evidence="$1" report="$2" state="$3" output="$4"
	local source_sha target_version stable_tag candidate_tag index_ref index_digest
	local evidence_sha report_sha highest tag_sha release_json registry_digest latest_digest
	local ocir_registry_digest ocir_latest_digest
	require_file "$evidence"
	require_file "$report"
	require_file "$state"
	"$EVIDENCE_TOOL" validate "$evidence"
	[ "$(jq -r '.publication_mode' "$evidence")" = canary ] || fail "candidate evidence has no published images"
	evidence_sha="$(sha256sum "$evidence" | awk '{print $1}')"
	jq -e --arg sha "$evidence_sha" '
		.schema_version == 1 and .promotable == true and .failed == false and .held == false
		and .candidate_evidence_sha256 == $sha' "$report" >/dev/null ||
		fail "gate report does not declare this exact candidate evidence promotable"
	source_sha="$(jq -r '.source_sha' "$evidence")"
	require_sha "$source_sha" "candidate source"
	[ "$(jq -r '.source_sha' "$report")" = "$source_sha" ] || fail "gate report source differs from the evidence"
	target_version="$(jq -r '.target_version' "$evidence")"
	candidate_tag="$(jq -r '.candidate_tag' "$evidence")"
	stable_tag="v$target_version"
	index_ref="$(jq -r '.images.docker_hub' "$evidence")"
	index_digest="${index_ref#*@}"
	require_digest "$index_digest" "candidate image index digest"
	report_sha="$(sha256sum "$report" | awk '{print $1}')"

	# Section 11 step 4: the stable version must still be available, unless
	# this run resumes a promotion that already created the tag on the
	# candidate source.
	jq -e '.stable_tags | type == "array"' "$state" >/dev/null || fail "stable state has no stable_tags array"
	highest="$(jq -r '[.stable_tags[] | select(test("^v(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$")) | ltrimstr("v")] | sort_by(split(".") | map(tonumber)) | last // "0.0.0"' "$state")"
	tag_sha="$(jq -r '.stable_tag.sha // empty' "$state")"
	# Ordering holds on every path, fresh or resumed: the target must be the
	# highest stable version, or equal to it only when this run resumes the
	# promotion that created that very tag on the candidate source. A resume
	# after a newer stable release exists must stop, or it would move the
	# aliases backward (section 2 invariant 15).
	[ "$(printf '%s\n%s\n' "$target_version" "$highest" | sort -V | tail -1)" = "$target_version" ] ||
		fail "target version $target_version is lower than the highest stable version $highest"
	if [ -n "$tag_sha" ]; then
		[ "$tag_sha" = "$source_sha" ] ||
			fail "stable tag $stable_tag exists and resolves to $tag_sha, not the candidate source $source_sha"
	else
		! jq -e --arg tag "$stable_tag" '.stable_tags | index($tag)' "$state" >/dev/null ||
			fail "stable tag $stable_tag is listed but its target is unknown"
		[ "$target_version" != "$highest" ] ||
			fail "target version $target_version is already the highest stable version but its tag is absent"
	fi

	release_json="$(jq -c '.stable_release' "$state")"
	if [ "$release_json" != null ]; then
		[ -n "$tag_sha" ] || fail "stable release $stable_tag exists without its tag"
		jq -e '.stable_release.prerelease == false' "$state" >/dev/null ||
			fail "release $stable_tag exists as a prerelease"
		# An existing stable release is skipped only when every required
		# asset is present (section 15: resume verifies every existing
		# object). A partial release stops promotion for investigation.
		jq -e '.stable_release.assets as $a
			| ["f1r3node-linux-amd64", "f1r3node-linux-arm64", "checksums.txt", "release-evidence.json",
			   "gate-report.json", "stable-release-evidence.json", "stable-release-evidence.json.sha256"]
			| all(. as $n | $a | index($n) != null)' "$state" >/dev/null ||
			fail "stable release $stable_tag exists but is missing required assets; investigate before rerunning"
	fi

	registry_digest="$(jq -r '.registry.stable_tag_digest // empty' "$state")"
	if [ -n "$registry_digest" ]; then
		require_digest "$registry_digest" "registry stable tag digest"
		[ "$registry_digest" = "$index_digest" ] ||
			fail "registry tag $stable_tag resolves to $registry_digest, not the candidate index $index_digest"
	fi
	latest_digest="$(jq -r '.registry.latest_digest // empty' "$state")"
	ocir_registry_digest="$(jq -r '.ocir.stable_tag_digest // empty' "$state")"
	if [ -n "$ocir_registry_digest" ]; then
		require_digest "$ocir_registry_digest" "OCIR stable tag digest"
		[ "$ocir_registry_digest" = "$index_digest" ] ||
			fail "OCIR tag $stable_tag resolves to $ocir_registry_digest, not the candidate index $index_digest"
	fi
	ocir_latest_digest="$(jq -r '.ocir.latest_digest // empty' "$state")"
	[ "$(jq -r '.images.ocir_index_digest' "$evidence")" = "$index_digest" ] ||
		fail "candidate evidence OCIR index digest differs from the Docker Hub index digest"

	mkdir -p "$(dirname "$output")"
	jq -n \
		--arg stable_tag "$stable_tag" \
		--arg candidate_tag "$candidate_tag" \
		--arg source_sha "$source_sha" \
		--arg target_version "$target_version" \
		--arg next_version "$(next_patch_version "$target_version")" \
		--arg index_ref "$index_ref" \
		--arg index_digest "$index_digest" \
		--arg image_repository "${index_ref%@*}" \
		--arg evidence_sha "$evidence_sha" \
		--arg report_sha "$report_sha" \
		--argjson create_tag "$([ -z "$tag_sha" ] && echo true || echo false)" \
		--argjson create_release "$([ "$release_json" = null ] && echo true || echo false)" \
		--argjson copy_image "$([ -z "$registry_digest" ] && echo true || echo false)" \
		--argjson move_latest "$([ "$latest_digest" != "$index_digest" ] && echo true || echo false)" \
		--argjson copy_image_ocir "$([ -z "$ocir_registry_digest" ] && echo true || echo false)" \
		--argjson move_latest_ocir "$([ "$ocir_latest_digest" != "$index_digest" ] && echo true || echo false)" \
		--argjson binaries "$(jq -c '{
			"f1r3node-linux-amd64": .artifacts.linux_amd64.binary_sha256,
			"f1r3node-linux-arm64": .artifacts.linux_arm64.binary_sha256}' "$evidence")" '
		{
			schema_version: 1,
			stable_tag: $stable_tag,
			candidate_tag: $candidate_tag,
			source_sha: $source_sha,
			target_version: $target_version,
			next_version: $next_version,
			candidate_evidence_sha256: $evidence_sha,
			gate_report_sha256: $report_sha,
			image: {
				repository: $image_repository,
				candidate_index: $index_ref,
				index_digest: $index_digest,
				stable_reference: ($image_repository + ":" + $stable_tag),
				latest_reference: ($image_repository + ":latest"),
				ocir: {
					canonical: true,
					index_digest: $index_digest,
					stable_tag: $stable_tag,
					latest_tag: "latest"
				}
			},
			binaries: $binaries,
			actions: {
				create_tag: $create_tag,
				create_release: $create_release,
				copy_image: $copy_image,
				move_latest: $move_latest,
				copy_image_ocir: $copy_image_ocir,
				move_latest_ocir: $move_latest_ocir,
				open_next_version_pr: true
			}
		}' >"$output"
	printf 'promotion plan for %s written\n' "$stable_tag" >&2
}

verify_binaries() {
	local plan="$1" dir="$2" name expected actual
	require_file "$plan"
	while IFS=$'\t' read -r name expected; do
		require_file "$dir/$name"
		actual="$(sha256sum "$dir/$name" | awk '{print $1}')"
		[ "$actual" = "$expected" ] || fail "binary $name checksum $actual differs from candidate evidence $expected"
	done < <(jq -r '.binaries | to_entries[] | [.key, .value] | @tsv' "$plan")
	printf 'candidate binaries match the evidence\n' >&2
}

stable_evidence() {
	local plan="$1" stable_digest="$2" promoted_at="$3" output="$4"
	require_file "$plan"
	require_digest "$stable_digest" "stable image digest"
	[ "$stable_digest" = "$(jq -r '.image.index_digest' "$plan")" ] ||
		fail "stable image digest $stable_digest differs from the candidate index"
	[[ "$promoted_at" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]] ||
		fail "promoted_at must be an RFC 3339 UTC timestamp"
	mkdir -p "$(dirname "$output")"
	jq --arg promoted_at "$promoted_at" '{
		schema_version: 1,
		stable_tag: .stable_tag,
		candidate_tag: .candidate_tag,
		source_sha: .source_sha,
		target_version: .target_version,
		candidate_evidence_sha256: .candidate_evidence_sha256,
		gate_report_sha256: .gate_report_sha256,
		images: {
			docker_hub: (.image.repository + "@" + .image.index_digest),
			ocir_index_digest: .image.ocir.index_digest,
			stable_reference: .image.stable_reference
		},
		binaries: .binaries,
		promoted_at: $promoted_at
	}' "$plan" >"$output"
	jq -e '([.. | strings | select(startswith("/"))] | length == 0)' "$output" >/dev/null ||
		fail "stable evidence contains an absolute path"
}

bump_next_version() {
	local source="$1" next="$2" current
	require_file "$source/node/Cargo.toml"
	require_file "$source/node/Dockerfile"
	require_file "$source/Cargo.lock"
	current="$("$EVIDENCE_TOOL" inspect-source "$source" | jq -r '.target_version')"
	[ "$current" != "$next" ] || fail "source already carries version $next"
	awk -v target="$next" '
		$0 == "[package]" { package = 1 }
		/^\[/ && $0 != "[package]" { package = 0 }
		package && /^version[[:space:]]*=/ { $0 = "version = \"" target "\"" }
		{ print }' "$source/node/Cargo.toml" >"$source/node/Cargo.toml.next"
	mv "$source/node/Cargo.toml.next" "$source/node/Cargo.toml"
	sed -E "s/^LABEL version=\"[^\"]+\"[[:space:]]*$/LABEL version=\"$next\"/" "$source/node/Dockerfile" >"$source/node/Dockerfile.next"
	mv "$source/node/Dockerfile.next" "$source/node/Dockerfile"
	awk -v target="$next" '
		/^\[\[package\]\]$/ { name = "" }
		/^name[[:space:]]*=/ { name = $0; sub(/^[^=]*=[[:space:]]*"/, "", name); sub(/"[[:space:]]*$/, "", name) }
		name == "node" && /^version[[:space:]]*=/ { $0 = "version = \"" target "\"" }
		{ print }' "$source/Cargo.lock" >"$source/Cargo.lock.next"
	mv "$source/Cargo.lock.next" "$source/Cargo.lock"
	[ "$("$EVIDENCE_TOOL" inspect-source "$source" | jq -r '.target_version')" = "$next" ] ||
		fail "next-version bump did not produce a consistent source version"
	printf 'source version is now %s\n' "$next" >&2
}

usage() {
	printf '%s\n' \
		"usage: $0 plan EVIDENCE_JSON GATE_REPORT STABLE_STATE_JSON OUTPUT" \
		"       $0 verify-binaries PLAN_JSON BINARIES_DIR" \
		"       $0 stable-evidence PLAN_JSON STABLE_IMAGE_DIGEST PROMOTED_AT OUTPUT" \
		"       $0 bump-next-version SOURCE_DIR NEXT_VERSION" >&2
	exit 2
}

command="${1:-}"
case "$command" in
plan)
	[ "$#" -eq 5 ] || usage
	plan "$2" "$3" "$4" "$5"
	;;
verify-binaries)
	[ "$#" -eq 3 ] || usage
	verify_binaries "$2" "$3"
	;;
stable-evidence)
	[ "$#" -eq 5 ] || usage
	stable_evidence "$2" "$3" "$4" "$5"
	;;
bump-next-version)
	[ "$#" -eq 3 ] || usage
	bump_next_version "$2" "$3"
	;;
*) usage ;;
esac

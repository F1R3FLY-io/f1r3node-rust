#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REQUIRED_JOBS_FILE="$SCRIPT_DIR/release-required-ci-jobs.txt"

fail() {
	printf 'release evidence error: %s\n' "$1" >&2
	exit 1
}

require_file() {
	[ -f "$1" ] || fail "missing file: $1"
}

require_sha() {
	[[ "$1" =~ ^[0-9a-f]{40}$ ]] || fail "$2 must be a full lowercase commit SHA"
}

require_sha256() {
	[[ "$1" =~ ^[0-9a-f]{64}$ ]] || fail "$2 must be a lowercase SHA-256 value"
}

require_artifact_digest() {
	[[ "$1" =~ ^sha256:[0-9a-f]{64}$ ]] || fail "$2 must be a GitHub SHA-256 artifact digest"
}

require_positive_integer() {
	[[ "$1" =~ ^[1-9][0-9]*$ ]] || fail "$2 must be a positive integer"
}

require_semver() {
	[[ "$1" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] ||
		fail "$2 must be a stable Semantic Versioning value"
}

version_gt() {
	[ "$1" != "$2" ] && [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -1)" = "$1" ]
}

node_cargo_version() {
	awk '
		$0 == "[package]" { package = 1; next }
		/^\[/ && $0 != "[package]" { package = 0 }
		package && /^version[[:space:]]*=/ {
			value = $0
			sub(/^[^=]*=[[:space:]]*"/, "", value)
			sub(/"[[:space:]]*$/, "", value)
			count++
		}
		END {
			if (count != 1) exit 1
			print value
		}
	' "$1" || fail "$1 must contain one package version"
}

node_lock_version() {
	awk '
		function finish() {
			if (name == "node") {
				count++
				found = version
			}
		}
		/^\[\[package\]\]$/ { finish(); name = ""; version = ""; next }
		/^name[[:space:]]*=/ {
			name = $0
			sub(/^[^=]*=[[:space:]]*"/, "", name)
			sub(/"[[:space:]]*$/, "", name)
			next
		}
		/^version[[:space:]]*=/ {
			version = $0
			sub(/^[^=]*=[[:space:]]*"/, "", version)
			sub(/"[[:space:]]*$/, "", version)
		}
		END {
			finish()
			if (count != 1 || found == "") exit 1
			print found
		}
	' "$1" || fail "$1 must contain one node package"
}

node_docker_version() {
	local values count
	values="$(sed -nE 's/^LABEL version="([^"]+)"[[:space:]]*$/\1/p' "$1")"
	count="$(printf '%s\n' "$values" | grep -c . || true)"
	[ "$count" -eq 1 ] || fail "$1 must contain one version label"
	printf '%s\n' "$values"
}

system_integration_sha() {
	local values count
	values="$(sed -nE 's/^SYSTEM_INTEGRATION_REF=([0-9a-f]+)[[:space:]]*$/\1/p' "$1")"
	count="$(printf '%s\n' "$values" | grep -c . || true)"
	[ "$count" -eq 1 ] || fail "$1 must contain one SYSTEM_INTEGRATION_REF"
	require_sha "$values" SYSTEM_INTEGRATION_REF
	printf '%s\n' "$values"
}

inspect_source() {
	local source node_cargo node_docker cargo_lock target source_sha integration_sha
	local stable_tags highest_tag highest_version eligible
	source="$(cd "$1" && pwd)"
	require_file "$source/node/Cargo.toml"
	require_file "$source/node/Dockerfile"
	require_file "$source/Cargo.lock"
	require_file "$source/.github/oci-validation.env"
	node_cargo="$(node_cargo_version "$source/node/Cargo.toml")"
	node_docker="$(node_docker_version "$source/node/Dockerfile")"
	cargo_lock="$(node_lock_version "$source/Cargo.lock")"
	[ "$node_cargo" = "$node_docker" ] && [ "$node_cargo" = "$cargo_lock" ] ||
		fail "source versions differ: node/Cargo.toml=$node_cargo node/Dockerfile=$node_docker Cargo.lock=$cargo_lock"
	target="$node_cargo"
	require_semver "$target" "target version"
	source_sha="$(git -C "$source" rev-parse HEAD | tr '[:upper:]' '[:lower:]')"
	require_sha "$source_sha" "source checkout"
	integration_sha="$(system_integration_sha "$source/.github/oci-validation.env")"
	stable_tags="$(git -C "$source" tag --list | grep -E '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' || true)"
	highest_tag="$(printf '%s\n' "$stable_tags" | grep -E '^v' | sort -V | tail -1 || true)"
	if [ -n "$highest_tag" ]; then
		highest_version="${highest_tag#v}"
	else
		highest_version=0.0.0
	fi
	eligible=false
	if version_gt "$target" "$highest_version" && ! grep -Fxq "v$target" <<<"$stable_tags"; then
		eligible=true
	fi
	jq -n \
		--arg source_sha "$source_sha" \
		--arg target_version "$target" \
		--arg node_cargo "$node_cargo" \
		--arg node_docker "$node_docker" \
		--arg cargo_lock "$cargo_lock" \
		--arg highest_stable_tag "$highest_tag" \
		--arg highest_stable_version "$highest_version" \
		--arg system_integration_sha "$integration_sha" \
		--argjson release_eligible "$eligible" \
		'{
			source_sha: $source_sha,
			target_version: $target_version,
			source_versions: {
				node_cargo: $node_cargo,
				node_docker: $node_docker,
				cargo_lock: $cargo_lock
			},
			highest_stable_tag: (if $highest_stable_tag == "" then null else $highest_stable_tag end),
			highest_stable_version: $highest_stable_version,
			release_eligible: $release_eligible,
			system_integration_sha: $system_integration_sha
		}'
}

required_jobs_json() {
	require_file "$REQUIRED_JOBS_FILE"
	jq -Rn '[inputs | select(length > 0)]' <"$REQUIRED_JOBS_FILE"
}

validate_run() {
	local run_json="$1" source_json="$2" repository="$3"
	local source_sha run_id run_number run_attempt completed_at
	require_file "$run_json"
	source_sha="$(jq -r '.source_sha' <<<"$source_json")"
	jq -e \
		--arg repository "$repository" \
		--arg source_sha "$source_sha" '
		type == "object"
		and (.id | type == "number" and . > 0 and floor == .)
		and (.run_number | type == "number" and . > 0 and floor == .)
		and (.run_attempt | type == "number" and . > 0 and floor == .)
		and .path == ".github/workflows/ci.yml"
		and .event == "push"
		and .head_branch == "master"
		and .status == "completed"
		and .conclusion == "success"
		and .head_sha == $source_sha
		and .repository.full_name == $repository
		and (.updated_at | type == "string" and endswith("Z"))
	' "$run_json" >/dev/null || fail "CI run does not identify a successful master push for the source SHA"
	run_id="$(jq -r '.id' "$run_json")"
	run_number="$(jq -r '.run_number' "$run_json")"
	run_attempt="$(jq -r '.run_attempt' "$run_json")"
	completed_at="$(jq -r '.updated_at' "$run_json")"
	require_positive_integer "$run_id" "CI run ID"
	require_positive_integer "$run_number" "CI run number"
	require_positive_integer "$run_attempt" "CI run attempt"
	jq -n \
		--argjson run_id "$run_id" \
		--argjson run_number "$run_number" \
		--argjson run_attempt "$run_attempt" \
		--arg completed_at "$completed_at" \
		'{
			run_id: $run_id,
			run_number: $run_number,
			run_attempt: $run_attempt,
			workflow: ".github/workflows/ci.yml",
			event: "push",
			conclusion: "success",
			completed_at: $completed_at
		}'
}

validate_jobs() {
	local jobs_json="$1" required_json selected_file name count id
	require_file "$jobs_json"
	jq -e '.jobs | type == "array"' "$jobs_json" >/dev/null || fail "CI jobs document has no jobs array"
	required_json="$(required_jobs_json)"
	selected_file="$(mktemp)"
	: >"$selected_file"
	while IFS= read -r name; do
		count="$(jq --arg name "$name" '[.jobs[] | select(.name == $name)] | length' "$jobs_json")"
		[ "$count" -eq 1 ] || fail "required CI job '$name' occurs $count times"
		jq -e --arg name "$name" '.jobs[] | select(.name == $name) | .status == "completed" and .conclusion == "success"' "$jobs_json" >/dev/null ||
			fail "required CI job '$name' did not succeed"
		id="$(jq -r --arg name "$name" '.jobs[] | select(.name == $name) | .id' "$jobs_json")"
		require_positive_integer "$id" "job ID for $name"
		jq -n --argjson id "$id" --arg name "$name" '{id: $id, name: $name, conclusion: "success"}' >>"$selected_file"
	done < <(jq -r '.[]' <<<"$required_json")
	jq -s '.' "$selected_file"
	rm -f "$selected_file"
}

artifact_record() {
	local artifacts_json="$1" run_id="$2" name="$3" archive="$4" binary="$5"
	local count artifact_id digest archive_sha binary_sha
	require_file "$artifacts_json"
	require_file "$archive"
	require_file "$binary"
	count="$(jq --arg name "$name" '[.artifacts[] | select(.name == $name)] | length' "$artifacts_json")"
	[ "$count" -eq 1 ] || fail "GitHub artifact '$name' occurs $count times"
	jq -e --arg name "$name" --argjson run_id "$run_id" '
		.artifacts[]
		| select(.name == $name)
		| .expired == false and .workflow_run.id == $run_id
	' "$artifacts_json" >/dev/null || fail "GitHub artifact '$name' is expired or belongs to another run"
	artifact_id="$(jq -r --arg name "$name" '.artifacts[] | select(.name == $name) | .id' "$artifacts_json")"
	digest="$(jq -r --arg name "$name" '.artifacts[] | select(.name == $name) | .digest' "$artifacts_json")"
	require_positive_integer "$artifact_id" "artifact ID for $name"
	require_artifact_digest "$digest" "artifact digest for $name"
	archive_sha="$(sha256sum "$archive" | awk '{print $1}')"
	binary_sha="$(sha256sum "$binary" | awk '{print $1}')"
	require_sha256 "$archive_sha" "archive checksum for $name"
	require_sha256 "$binary_sha" "binary checksum for $name"
	jq -n \
		--arg binary_sha256 "$binary_sha" \
		--arg docker_archive_sha256 "$archive_sha" \
		--argjson github_artifact_id "$artifact_id" \
		--arg github_artifact_digest "$digest" \
		'{
			binary_sha256: $binary_sha256,
			docker_archive_sha256: $docker_archive_sha256,
			github_artifact_id: $github_artifact_id,
			github_artifact_digest: $github_artifact_digest
		}'
}

generate_evidence() {
	local source_dir="$1" repository="$2" run_json="$3" jobs_json="$4"
	local artifacts_json="$5" artifacts_dir="$6" output="$7"
	local source_json run_record jobs jobs_digest run_id target_version run_number
	local amd64 arm64
	source_json="$(inspect_source "$source_dir")"
	[ "$(jq -r '.release_eligible' <<<"$source_json")" = true ] ||
		fail "target version $(jq -r '.target_version' <<<"$source_json") is not greater than stable version $(jq -r '.highest_stable_version' <<<"$source_json")"
	run_record="$(validate_run "$run_json" "$source_json" "$repository")"
	jobs="$(validate_jobs "$jobs_json")"
	jobs_digest="$(jq -cS . <<<"$jobs" | sha256sum | awk '{print $1}')"
	run_id="$(jq -r '.run_id' <<<"$run_record")"
	target_version="$(jq -r '.target_version' <<<"$source_json")"
	run_number="$(jq -r '.run_number' <<<"$run_record")"
	amd64="$(artifact_record \
		"$artifacts_json" "$run_id" artifacts-docker-amd64 \
		"$artifacts_dir/artifacts-docker-amd64/rust-node-docker.tar.gz" \
		"$artifacts_dir/release/f1r3node-linux-amd64")"
	arm64="$(artifact_record \
		"$artifacts_json" "$run_id" artifacts-docker-arm64 \
		"$artifacts_dir/artifacts-docker-arm64/rust-node-docker.tar.gz" \
		"$artifacts_dir/release/f1r3node-linux-arm64")"
	mkdir -p "$(dirname "$output")"
	jq -n \
		--arg candidate_tag "v${target_version}-canary.${run_number}" \
		--arg target_version "$target_version" \
		--arg source_sha "$(jq -r '.source_sha' <<<"$source_json")" \
		--arg system_integration_sha "$(jq -r '.system_integration_sha' <<<"$source_json")" \
		--argjson ci "$run_record" \
		--arg required_jobs_digest "$jobs_digest" \
		--argjson required_jobs "$jobs" \
		--argjson linux_amd64 "$amd64" \
		--argjson linux_arm64 "$arm64" \
		--arg created_at "$(jq -r '.completed_at' <<<"$run_record")" \
		'{
			schema_version: 1,
			publication_mode: "evidence_only",
			candidate_tag: $candidate_tag,
			target_version: $target_version,
			source_sha: $source_sha,
			source_ref: "refs/heads/master",
			train_id: null,
			ci: ($ci + {
				required_jobs_digest: $required_jobs_digest,
				required_jobs: $required_jobs
			}),
			system_integration_sha: $system_integration_sha,
			artifacts: {
				linux_amd64: $linux_amd64,
				linux_arm64: $linux_arm64
			},
			images: {
				publication_state: "not_published",
				docker_hub: null,
				ocir: null
			},
			created_at: $created_at
		}' >"$output"
	validate_evidence "$output"
}

validate_evidence() {
	local evidence="$1" required_json jobs_digest recorded_digest
	require_file "$evidence"
	required_json="$(required_jobs_json)"
	jq -e --argjson required_jobs "$required_json" '
		type == "object"
		and .schema_version == 1
		and .publication_mode == "evidence_only"
		and (.target_version | type == "string" and test("^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$"))
		and .candidate_tag == ("v" + .target_version + "-canary." + (.ci.run_number | tostring))
		and (.source_sha | type == "string" and test("^[0-9a-f]{40}$"))
		and .source_ref == "refs/heads/master"
		and .train_id == null
		and (.system_integration_sha | type == "string" and test("^[0-9a-f]{40}$"))
		and (.ci.run_id | type == "number" and . > 0 and floor == .)
		and (.ci.run_number | type == "number" and . > 0 and floor == .)
		and (.ci.run_attempt | type == "number" and . > 0 and floor == .)
		and .ci.workflow == ".github/workflows/ci.yml"
		and .ci.event == "push"
		and .ci.conclusion == "success"
		and (.ci.completed_at | type == "string" and endswith("Z"))
		and (.ci.required_jobs_digest | type == "string" and test("^[0-9a-f]{64}$"))
		and ([.ci.required_jobs[].name] == $required_jobs)
		and ([.ci.required_jobs[] | select(.conclusion != "success")] | length == 0)
		and (.artifacts | type == "object")
		and ([.artifacts.linux_amd64, .artifacts.linux_arm64] | all(
			(.binary_sha256 | type == "string" and test("^[0-9a-f]{64}$"))
			and (.docker_archive_sha256 | type == "string" and test("^[0-9a-f]{64}$"))
			and (.github_artifact_id | type == "number" and . > 0 and floor == .)
			and (.github_artifact_digest | type == "string" and test("^sha256:[0-9a-f]{64}$"))
		))
		and .images.publication_state == "not_published"
		and .images.docker_hub == null
		and .images.ocir == null
		and (.created_at | type == "string" and endswith("Z"))
		and ([.. | strings | select(startswith("/"))] | length == 0)
	' "$evidence" >/dev/null || fail "evidence schema validation failed"
	jobs_digest="$(jq -cS '.ci.required_jobs' "$evidence" | sha256sum | awk '{print $1}')"
	recorded_digest="$(jq -r '.ci.required_jobs_digest' "$evidence")"
	[ "$jobs_digest" = "$recorded_digest" ] || fail "required CI jobs digest does not match"
}

usage() {
	printf '%s\n' \
		"usage: $0 inspect-source SOURCE_DIR" \
		"       $0 required-jobs" \
		"       $0 generate SOURCE_DIR REPOSITORY RUN_JSON JOBS_JSON ARTIFACTS_JSON ARTIFACTS_DIR OUTPUT" \
		"       $0 validate EVIDENCE_JSON" >&2
	exit 2
}

command="${1:-}"
case "$command" in
inspect-source)
	[ "$#" -eq 2 ] || usage
	inspect_source "$2"
	;;
required-jobs)
	[ "$#" -eq 1 ] || usage
	required_jobs_json
	;;
generate)
	[ "$#" -eq 8 ] || usage
	generate_evidence "$2" "$3" "$4" "$5" "$6" "$7" "$8"
	;;
validate)
	[ "$#" -eq 2 ] || usage
	validate_evidence "$2"
	;;
*) usage ;;
esac

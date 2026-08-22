#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EVIDENCE_TOOL="$ROOT/.github/scripts/release-evidence.sh"
GATES_TOOL="$ROOT/.github/scripts/release-gates.sh"
TOOL="$ROOT/.github/scripts/promote-release.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
REPOSITORY=example/repository
PIN=0123456789abcdef0123456789abcdef01234567

# --- Promotable candidate fixture (evidence + passing gate report) --------
SOURCE="$TMP/source"
ARTIFACTS="$TMP/artifacts"
mkdir -p "$SOURCE/node" "$SOURCE/.github" \
	"$ARTIFACTS/artifacts-docker-amd64" "$ARTIFACTS/artifacts-docker-arm64" "$ARTIFACTS/release"
printf '%s\n' '[package]' 'name = "node"' 'version = "0.4.46"' '' '[dependencies]' 'version = "ignored"' >"$SOURCE/node/Cargo.toml"
printf '%s\n' 'FROM scratch' 'LABEL version="0.4.46"' >"$SOURCE/node/Dockerfile"
printf '%s\n' 'version = 4' '[[package]]' 'name = "other"' 'version = "9.9.9"' '[[package]]' 'name = "node"' 'version = "0.4.46"' >"$SOURCE/Cargo.lock"
printf 'SYSTEM_INTEGRATION_REF=%s\n' "$PIN" >"$SOURCE/.github/oci-validation.env"
git -C "$SOURCE" init -q
git -C "$SOURCE" config user.name 'Promote Release Test'
git -C "$SOURCE" config user.email 'test@example.com'
git -C "$SOURCE" add .
git -C "$SOURCE" commit -qm 'test fixture'
git -C "$SOURCE" tag v0.4.45
SOURCE_SHA="$(git -C "$SOURCE" rev-parse HEAD)"
printf 'amd64 archive\n' >"$ARTIFACTS/artifacts-docker-amd64/rust-node-docker.tar.gz"
printf 'arm64 archive\n' >"$ARTIFACTS/artifacts-docker-arm64/rust-node-docker.tar.gz"
printf 'amd64 binary\n' >"$ARTIFACTS/release/f1r3node-linux-amd64"
printf 'arm64 binary\n' >"$ARTIFACTS/release/f1r3node-linux-arm64"
jq -n --arg sha "$SOURCE_SHA" --arg repo "$REPOSITORY" '{
	id: 123456789, run_number: 812, run_attempt: 1, path: ".github/workflows/ci.yml", event: "push",
	head_branch: "master", status: "completed", conclusion: "success", head_sha: $sha,
	repository: {full_name: $repo}, updated_at: "2026-08-16T00:00:00Z"}' >"$TMP/ci-run.json"
"$EVIDENCE_TOOL" required-jobs | jq '[to_entries[] | {
	id: (.key + 1000), name: .value, status: "completed", conclusion: "success",
	completed_at: "2026-08-16T00:00:00Z"}] | {jobs: .}' >"$TMP/ci-jobs.json"
cat >"$TMP/artifacts.json" <<EOF
{"artifacts": [
  {"id": 201, "name": "artifacts-docker-amd64", "expired": false,
   "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "workflow_run": {"id": 123456789}},
  {"id": 202, "name": "artifacts-docker-arm64", "expired": false,
   "digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "workflow_run": {"id": 123456789}}
]}
EOF
GATES="$TMP/gates"
mkdir -p "$GATES"
"$EVIDENCE_TOOL" generate "$SOURCE" "$REPOSITORY" "$TMP/ci-run.json" "$TMP/ci-jobs.json" "$TMP/artifacts.json" "$ARTIFACTS" "$GATES/release-evidence.json"
INDEX_DIGEST="sha256:$(printf 'index' | sha256sum | awk '{print $1}')"
REPO_REF=docker.io/f1r3flyindustries/f1r3fly-rust
"$EVIDENCE_TOOL" record-images "$GATES/release-evidence.json" "$REPO_REF@$INDEX_DIGEST" \
	"sha256:$(printf 'amd64' | sha256sum | awk '{print $1}')" "sha256:$(printf 'arm64' | sha256sum | awk '{print $1}')"
EVIDENCE="$GATES/release-evidence.json"
CANDIDATE_TAG="$(jq -r '.candidate_tag' "$EVIDENCE")"
cp "$TMP/ci-run.json" "$TMP/ci-jobs.json" "$GATES/"
jq '.path = ".github/workflows/slashing-tests.yml" | .id = 555' "$TMP/ci-run.json" >"$GATES/slashing-run.json"
jq -n '{jobs: ([{name: "Example-based UC tests"}, {name: "Property-based theorem tests"}, {name: "Loom exhaustive interleaving (T-9.2)"},
	{name: "Pre-fix regression backstops (1)"}] | map(. + {status: "completed", conclusion: "success"}))}' >"$GATES/slashing-jobs.json"
binding() {
	jq -n --arg gate "$1" --arg sha "$SOURCE_SHA" --arg tag "$CANDIDATE_TAG" --arg digest "$INDEX_DIGEST" --arg path "$2" '{
		schema_version: 1, gate: $gate, source_sha: $sha, candidate_tag: $tag, image_index_digest: $digest,
		workflow_run: {id: 7, attempt: 1, path: $path, conclusion: "success"}}'
}
binding oci_validation .github/workflows/oci-validation.yml | jq --arg pin "$PIN" '. + {mode: "candidate", system_integration_sha: $pin,
	required_jobs: [{name: "OCI validation", conclusion: "success"}]}' >"$GATES/oci-validation-evidence.json"
binding stability_soak .github/workflows/merge-recovery-soak.yml | jq '. + {soak_kind: "weekend", requested_duration_seconds: 216000,
	completed: true, artifact_mode: "candidate", retry_attempt: 0, coverage_preserved: true, preflight: {status: "success"}}' >"$GATES/soak-evidence.json"
jq -n --arg sha "$SOURCE_SHA" '{schema_version: 1, source_sha: $sha, verdict: "pass"}' >"$GATES/verdict.json"
jq '.path = ".github/workflows/oci-validation.yml" | .id = 7 | .event = "workflow_dispatch"' "$TMP/ci-run.json" >"$GATES/oci-validation-run.json"
jq '.path = ".github/workflows/merge-recovery-soak.yml" | .id = 7 | .event = "workflow_dispatch"' "$TMP/ci-run.json" >"$GATES/soak-run.json"
REPORT="$TMP/gate-report.json"
"$GATES_TOOL" evaluate "$GATES" "$REPOSITORY" "$REPORT" 2>/dev/null

expect_failure() {
	local label="$1"
	shift
	if "$@" >"$TMP/out" 2>"$TMP/err"; then
		printf 'expected failure: %s\n' "$label" >&2
		exit 1
	fi
}

state() {
	jq -n "$1"
}

# --- Fresh promotion: every action planned ----------------------------------
state '{stable_tags: ["v0.4.45"], stable_tag: null, stable_release: null, registry: {stable_tag_digest: null, latest_digest: null}}' >"$TMP/fresh.json"
PLAN="$TMP/plan.json"
"$TOOL" plan "$EVIDENCE" "$REPORT" "$TMP/fresh.json" "$PLAN" 2>/dev/null
jq -e --arg sha "$SOURCE_SHA" --arg digest "$INDEX_DIGEST" --arg tag "$CANDIDATE_TAG" '
	.stable_tag == "v0.4.46" and .candidate_tag == $tag and .source_sha == $sha and .next_version == "0.4.47"
	and .image.index_digest == $digest
	and .image.stable_reference == "docker.io/f1r3flyindustries/f1r3fly-rust:v0.4.46"
	and .image.latest_reference == "docker.io/f1r3flyindustries/f1r3fly-rust:latest"
	and .actions == {create_tag: true, create_release: true, copy_image: true, move_latest: true, open_next_version_pr: true}
	and (.binaries | keys) == ["f1r3node-linux-amd64", "f1r3node-linux-arm64"]' "$PLAN" >/dev/null
"$TOOL" plan "$EVIDENCE" "$REPORT" "$TMP/fresh.json" "$TMP/plan-repeat.json" 2>/dev/null
cmp "$PLAN" "$TMP/plan-repeat.json"

# --- Resume: existing objects that match are skipped -----------------------
state "{stable_tags: [\"v0.4.45\", \"v0.4.46\"], stable_tag: {sha: \"$SOURCE_SHA\"}, stable_release: {prerelease: false, assets: [\"f1r3node-linux-amd64\", \"f1r3node-linux-arm64\", \"checksums.txt\", \"release-evidence.json\", \"gate-report.json\", \"stable-release-evidence.json\", \"stable-release-evidence.json.sha256\"]},
	registry: {stable_tag_digest: \"$INDEX_DIGEST\", latest_digest: \"$INDEX_DIGEST\"}}" >"$TMP/resume.json"
"$TOOL" plan "$EVIDENCE" "$REPORT" "$TMP/resume.json" "$TMP/resume-plan.json" 2>/dev/null
jq -e '.actions == {create_tag: false, create_release: false, copy_image: false, move_latest: false, open_next_version_pr: true}' "$TMP/resume-plan.json" >/dev/null
state "{stable_tags: [\"v0.4.45\", \"v0.4.46\"], stable_tag: {sha: \"$SOURCE_SHA\"}, stable_release: null,
	registry: {stable_tag_digest: null, latest_digest: \"sha256:$(printf 'older' | sha256sum | awk '{print $1}')\"}}" >"$TMP/partial.json"
"$TOOL" plan "$EVIDENCE" "$REPORT" "$TMP/partial.json" "$TMP/partial-plan.json" 2>/dev/null
jq -e '.actions == {create_tag: false, create_release: true, copy_image: true, move_latest: true, open_next_version_pr: true}' "$TMP/partial-plan.json" >/dev/null

# --- Stop conditions ---------------------------------------------------------
OTHER_SHA=fedcba9876543210fedcba9876543210fedcba98
state "{stable_tags: [\"v0.4.45\", \"v0.4.46\"], stable_tag: {sha: \"$OTHER_SHA\"}, stable_release: null, registry: {stable_tag_digest: null, latest_digest: null}}" >"$TMP/bad-tag.json"
expect_failure 'stable tag points elsewhere' "$TOOL" plan "$EVIDENCE" "$REPORT" "$TMP/bad-tag.json" "$TMP/x.json"
state "{stable_tags: [\"v0.4.45\"], stable_tag: null, stable_release: null, registry: {stable_tag_digest: \"sha256:$(printf 'other' | sha256sum | awk '{print $1}')\", latest_digest: null}}" >"$TMP/bad-registry.json"
expect_failure 'registry stable tag points elsewhere' "$TOOL" plan "$EVIDENCE" "$REPORT" "$TMP/bad-registry.json" "$TMP/x.json"
state '{stable_tags: ["v0.4.45", "v0.4.47"], stable_tag: null, stable_release: null, registry: {stable_tag_digest: null, latest_digest: null}}' >"$TMP/overtaken.json"
expect_failure 'higher stable version published first' "$TOOL" plan "$EVIDENCE" "$REPORT" "$TMP/overtaken.json" "$TMP/x.json"
state "{stable_tags: [\"v0.4.45\"], stable_tag: null, stable_release: {prerelease: true, assets: []}, registry: {stable_tag_digest: null, latest_digest: null}}" >"$TMP/release-no-tag.json"
expect_failure 'release without tag' "$TOOL" plan "$EVIDENCE" "$REPORT" "$TMP/release-no-tag.json" "$TMP/x.json"
state "{stable_tags: [\"v0.4.45\", \"v0.4.46\"], stable_tag: {sha: \"$SOURCE_SHA\"}, stable_release: {prerelease: false, assets: [\"f1r3node-linux-amd64\"]}, registry: {stable_tag_digest: null, latest_digest: null}}" >"$TMP/partial-release.json"
expect_failure 'existing release with missing assets' "$TOOL" plan "$EVIDENCE" "$REPORT" "$TMP/partial-release.json" "$TMP/x.json"
state "{stable_tags: [\"v0.4.45\", \"v0.4.46\", \"v0.4.47\"], stable_tag: {sha: \"$SOURCE_SHA\"}, stable_release: null, registry: {stable_tag_digest: null, latest_digest: null}}" >"$TMP/resume-overtaken.json"
expect_failure 'resume after a newer stable release exists' "$TOOL" plan "$EVIDENCE" "$REPORT" "$TMP/resume-overtaken.json" "$TMP/x.json"
jq '.promotable = false | .held = true' "$REPORT" >"$TMP/held-report.json"
expect_failure 'held gate report' "$TOOL" plan "$EVIDENCE" "$TMP/held-report.json" "$TMP/fresh.json" "$TMP/x.json"
jq '.candidate_evidence_sha256 = "0000000000000000000000000000000000000000000000000000000000000000"' "$REPORT" >"$TMP/other-report.json"
expect_failure 'gate report for other evidence' "$TOOL" plan "$EVIDENCE" "$TMP/other-report.json" "$TMP/fresh.json" "$TMP/x.json"
"$EVIDENCE_TOOL" generate "$SOURCE" "$REPOSITORY" "$TMP/ci-run.json" "$TMP/ci-jobs.json" "$TMP/artifacts.json" "$ARTIFACTS" "$TMP/evidence-only.json"
expect_failure 'evidence-only candidate' "$TOOL" plan "$TMP/evidence-only.json" "$REPORT" "$TMP/fresh.json" "$TMP/x.json"

# --- Binary verification ----------------------------------------------------
"$TOOL" verify-binaries "$PLAN" "$ARTIFACTS/release" 2>/dev/null
mkdir -p "$TMP/tampered"
cp "$ARTIFACTS/release/"* "$TMP/tampered/"
printf 'tampered\n' >>"$TMP/tampered/f1r3node-linux-arm64"
expect_failure 'tampered binary' "$TOOL" verify-binaries "$PLAN" "$TMP/tampered"

# --- Stable evidence ---------------------------------------------------------
STABLE="$TMP/stable-release-evidence.json"
"$TOOL" stable-evidence "$PLAN" "$INDEX_DIGEST" 2026-08-20T00:00:00Z "$STABLE"
jq -e --arg sha "$SOURCE_SHA" --arg digest "$INDEX_DIGEST" --arg tag "$CANDIDATE_TAG" \
	--arg esha "$(sha256sum "$EVIDENCE" | awk '{print $1}')" '
	.schema_version == 1 and .stable_tag == "v0.4.46" and .candidate_tag == $tag and .source_sha == $sha
	and .images.docker_hub == ("docker.io/f1r3flyindustries/f1r3fly-rust@" + $digest)
	and .promoted_at == "2026-08-20T00:00:00Z"
	and .candidate_evidence_sha256 == $esha' "$STABLE" >/dev/null
expect_failure 'stable digest differs from candidate' "$TOOL" stable-evidence "$PLAN" "sha256:$(printf 'other' | sha256sum | awk '{print $1}')" 2026-08-20T00:00:00Z "$TMP/x.json"
expect_failure 'bad timestamp' "$TOOL" stable-evidence "$PLAN" "$INDEX_DIGEST" yesterday "$TMP/x.json"

# --- Next-version bump -------------------------------------------------------
"$TOOL" bump-next-version "$SOURCE" 0.4.47 2>/dev/null
"$EVIDENCE_TOOL" inspect-source "$SOURCE" | jq -e '.target_version == "0.4.47" and .release_eligible == true' >/dev/null
grep -q '^version = "ignored"$' "$SOURCE/node/Cargo.toml"
grep -q '^version = "9.9.9"$' "$SOURCE/Cargo.lock"
expect_failure 'bump to the current version' "$TOOL" bump-next-version "$SOURCE" 0.4.47

printf 'promote release tests passed\n'

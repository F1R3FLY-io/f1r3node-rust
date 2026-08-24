#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/release-evidence.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
SOURCE="$TMP/source"
ARTIFACTS="$TMP/artifacts"
PIN=0123456789abcdef0123456789abcdef01234567
REPOSITORY=example/repository
mkdir -p "$SOURCE/node" "$SOURCE/.github" \
	"$ARTIFACTS/artifacts-docker-amd64" \
	"$ARTIFACTS/artifacts-docker-arm64" \
	"$ARTIFACTS/release"
printf '%s\n' '[package]' 'name = "node"' 'version = "0.4.46"' >"$SOURCE/node/Cargo.toml"
printf '%s\n' 'FROM scratch' 'LABEL version="0.4.46"' >"$SOURCE/node/Dockerfile"
printf '%s\n' 'version = 4' '[[package]]' 'name = "node"' 'version = "0.4.46"' >"$SOURCE/Cargo.lock"
printf 'SYSTEM_INTEGRATION_REF=%s\n' "$PIN" >"$SOURCE/.github/oci-validation.env"
git -C "$SOURCE" init -q
git -C "$SOURCE" config user.name 'Release Evidence Test'
git -C "$SOURCE" config user.email 'test@example.com'
git -C "$SOURCE" add .
git -C "$SOURCE" commit -qm 'test fixture'
git -C "$SOURCE" tag v0.4.45
SOURCE_SHA="$(git -C "$SOURCE" rev-parse HEAD)"
printf 'amd64 archive\n' >"$ARTIFACTS/artifacts-docker-amd64/rust-node-docker.tar.gz"
printf 'arm64 archive\n' >"$ARTIFACTS/artifacts-docker-arm64/rust-node-docker.tar.gz"
printf 'amd64 binary\n' >"$ARTIFACTS/release/f1r3node-linux-amd64"
printf 'arm64 binary\n' >"$ARTIFACTS/release/f1r3node-linux-arm64"
cat >"$TMP/run.json" <<EOF
{
  "id": 123456789,
  "run_number": 812,
  "run_attempt": 2,
  "path": ".github/workflows/ci.yml",
  "event": "push",
  "head_branch": "master",
  "status": "completed",
  "conclusion": "success",
  "head_sha": "$SOURCE_SHA",
  "repository": {"full_name": "$REPOSITORY"},
  "updated_at": "2026-08-16T00:00:00Z"
}
EOF
"$TOOL" required-jobs | jq '[to_entries[] | {
	id: (.key + 1000),
	name: .value,
	status: "completed",
	conclusion: "success",
	completed_at: ("2026-08-16T0" + ((.key % 10) | tostring) + ":00:00Z")
}] | {jobs: .}' >"$TMP/jobs.json"
cat >"$TMP/artifacts.json" <<EOF
{
  "artifacts": [
    {
      "id": 201,
      "name": "artifacts-docker-amd64",
      "expired": false,
      "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "workflow_run": {"id": 123456789}
    },
    {
      "id": 202,
      "name": "artifacts-docker-arm64",
      "expired": false,
      "digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      "workflow_run": {"id": 123456789}
    }
  ]
}
EOF
OUTPUT="$TMP/release-evidence.json"
"$TOOL" inspect-source "$SOURCE" | jq -e '
	.target_version == "0.4.46"
	and .highest_stable_version == "0.4.45"
	and .release_eligible == true
	and .system_integration_sha == $pin
' --arg pin "$PIN" >/dev/null
"$TOOL" generate "$SOURCE" "$REPOSITORY" "$TMP/run.json" "$TMP/jobs.json" "$TMP/artifacts.json" "$ARTIFACTS" "$OUTPUT"
"$TOOL" validate "$OUTPUT"
"$TOOL" generate "$SOURCE" "$REPOSITORY" "$TMP/run.json" "$TMP/jobs.json" "$TMP/artifacts.json" "$ARTIFACTS" "$TMP/release-evidence-repeat.json"
cmp "$OUTPUT" "$TMP/release-evidence-repeat.json"
jq -e '
	.schema_version == 1
	and .publication_mode == "evidence_only"
	and .candidate_tag == "v0.4.46-canary.812"
	and .source_sha == $source_sha
	and .ci.run_id == 123456789
	and .ci.run_attempt == 2
	and (.ci.required_jobs | length) == 25
	and .images.publication_state == "not_published"
	and .images.docker_hub == null
	and .images.ocir_index_digest == null
' --arg source_sha "$SOURCE_SHA" "$OUTPUT" >/dev/null

expect_failure() {
	local label="$1"
	shift
	if "$@" >"$TMP/failure.out" 2>&1; then
		printf 'expected failure: %s\n' "$label" >&2
		exit 1
	fi
}

cp "$OUTPUT" "$TMP/tampered-evidence.json"
jq '.source_sha = "bad"' "$OUTPUT" >"$TMP/tampered-evidence.json"
expect_failure 'invalid evidence SHA' "$TOOL" validate "$TMP/tampered-evidence.json"
cp "$TMP/jobs.json" "$TMP/failed-jobs.json"
jq '(.jobs[] | select(.name == "Lint") | .conclusion) = "failure"' "$TMP/jobs.json" >"$TMP/failed-jobs.json"
expect_failure 'failed required job' "$TOOL" generate "$SOURCE" "$REPOSITORY" "$TMP/run.json" "$TMP/failed-jobs.json" "$TMP/artifacts.json" "$ARTIFACTS" "$TMP/failed.json"
cp "$TMP/artifacts.json" "$TMP/wrong-run-artifacts.json"
jq '(.artifacts[0].workflow_run.id) = 999' "$TMP/artifacts.json" >"$TMP/wrong-run-artifacts.json"
expect_failure 'artifact from another run' "$TOOL" generate "$SOURCE" "$REPOSITORY" "$TMP/run.json" "$TMP/jobs.json" "$TMP/wrong-run-artifacts.json" "$ARTIFACTS" "$TMP/wrong-run.json"
sed -i.bak 's/version="0.4.46"/version="0.4.47"/' "$SOURCE/node/Dockerfile"
expect_failure 'source version mismatch' "$TOOL" inspect-source "$SOURCE"
mv "$SOURCE/node/Dockerfile.bak" "$SOURCE/node/Dockerfile"
git -C "$SOURCE" tag v0.4.46
"$TOOL" inspect-source "$SOURCE" | jq -e '.release_eligible == false' >/dev/null
expect_failure 'stable version already exists' "$TOOL" generate "$SOURCE" "$REPOSITORY" "$TMP/run.json" "$TMP/jobs.json" "$TMP/artifacts.json" "$ARTIFACTS" "$TMP/ineligible.json"

INDEX_REF="docker.io/f1r3flyindustries/f1r3fly-rust@sha256:$(printf 'index' | sha256sum | awk '{print $1}')"
AMD64_DIGEST="sha256:$(printf 'amd64' | sha256sum | awk '{print $1}')"
ARM64_DIGEST="sha256:$(printf 'arm64' | sha256sum | awk '{print $1}')"
cp "$OUTPUT" "$TMP/canary-evidence.json"
"$TOOL" record-images "$TMP/canary-evidence.json" "$INDEX_REF" "$AMD64_DIGEST" "$ARM64_DIGEST" "${INDEX_REF#*@}"
jq -e '
	.publication_mode == "canary"
	and .images.publication_state == "published"
	and .images.ocir_index_digest == (.images.docker_hub | split("@")[1])
' "$TMP/canary-evidence.json" >/dev/null
"$TOOL" validate "$TMP/canary-evidence.json"
expect_failure 'record-images rejects canary input' "$TOOL" record-images "$TMP/canary-evidence.json" "$INDEX_REF" "$AMD64_DIGEST" "$ARM64_DIGEST" "${INDEX_REF#*@}"
cp "$OUTPUT" "$TMP/bad-ref.json"
expect_failure 'record-images rejects a tag reference' "$TOOL" record-images "$TMP/bad-ref.json" "docker.io/f1r3flyindustries/f1r3fly-rust:v0.4.46-canary.812" "$AMD64_DIGEST" "$ARM64_DIGEST" "${INDEX_REF#*@}"
cp "$OUTPUT" "$TMP/ocir-mismatch.json"
expect_failure 'record-images rejects a divergent OCIR digest' "$TOOL" record-images "$TMP/ocir-mismatch.json" "$INDEX_REF" "$AMD64_DIGEST" "$ARM64_DIGEST" "$AMD64_DIGEST"
jq '.images.ocir_index_digest = "sha256:0000000000000000000000000000000000000000000000000000000000000000"' "$TMP/canary-evidence.json" >"$TMP/tampered-ocir.json"
expect_failure 'OCIR digest differs from Docker Hub index' "$TOOL" validate "$TMP/tampered-ocir.json"
jq '.images.linux_amd64_digest = "sha256:bad"' "$TMP/canary-evidence.json" >"$TMP/tampered-canary.json"
expect_failure 'invalid canary image digest' "$TOOL" validate "$TMP/tampered-canary.json"
printf 'release evidence tests passed\n'

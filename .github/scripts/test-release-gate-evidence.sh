#!/usr/bin/env bash
set -euo pipefail

# Producer and evaluator agreement: every document that
# release-gate-evidence.sh writes must pass release-gates.sh, and a run
# whose image or source differs from the candidate must be refused by the
# writer before it can reach the evaluator.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EVIDENCE_TOOL="$ROOT/.github/scripts/release-evidence.sh"
GATES_TOOL="$ROOT/.github/scripts/release-gates.sh"
TOOL="$ROOT/.github/scripts/release-gate-evidence.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
REPOSITORY=example/repository
PIN=0123456789abcdef0123456789abcdef01234567

SOURCE="$TMP/source"
ARTIFACTS="$TMP/artifacts"
mkdir -p "$SOURCE/node" "$SOURCE/.github" \
	"$ARTIFACTS/artifacts-docker-amd64" "$ARTIFACTS/artifacts-docker-arm64" "$ARTIFACTS/release"
printf '%s\n' '[package]' 'name = "node"' 'version = "0.4.46"' >"$SOURCE/node/Cargo.toml"
printf '%s\n' 'FROM scratch' 'LABEL version="0.4.46"' >"$SOURCE/node/Dockerfile"
printf '%s\n' 'version = 4' '[[package]]' 'name = "node"' 'version = "0.4.46"' >"$SOURCE/Cargo.lock"
printf 'SYSTEM_INTEGRATION_REF=%s\n' "$PIN" >"$SOURCE/.github/oci-validation.env"
git -C "$SOURCE" init -q
git -C "$SOURCE" config user.name 'Gate Evidence Test'
git -C "$SOURCE" config user.email 'test@example.com'
git -C "$SOURCE" add .
git -C "$SOURCE" commit -qm 'test fixture'
git -C "$SOURCE" tag v0.4.45
SOURCE_SHA="$(git -C "$SOURCE" rev-parse HEAD)"
for f in artifacts-docker-amd64/rust-node-docker.tar.gz artifacts-docker-arm64/rust-node-docker.tar.gz release/f1r3node-linux-amd64 release/f1r3node-linux-arm64; do
	printf '%s\n' "$f" >"$ARTIFACTS/$f"
done
run_doc() {
	jq -n --arg path "$1" --argjson id "$2" --argjson attempt "$3" --arg sha "$4" --arg event "${5:-push}" \
		--arg branch "${6:-master}" --arg repo "$REPOSITORY" '{
		id: $id, run_number: ($id % 1000), run_attempt: $attempt, path: $path, event: $event, head_branch: $branch,
		status: "completed", conclusion: "success", head_sha: $sha, repository: {full_name: $repo},
		updated_at: "2026-08-16T00:00:00Z"}'
}
run_doc .github/workflows/ci.yml 123456789 1 "$SOURCE_SHA" >"$TMP/ci-run.json"
"$EVIDENCE_TOOL" required-jobs | jq '[to_entries[] | {id: (.key + 1000), name: .value, status: "completed",
	conclusion: "success", completed_at: "2026-08-16T00:00:00Z"}] | {jobs: .}' >"$TMP/ci-jobs.json"
cat >"$TMP/artifacts.json" <<EOF
{"artifacts": [
  {"id": 201, "name": "artifacts-docker-amd64", "expired": false, "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "workflow_run": {"id": 123456789}},
  {"id": 202, "name": "artifacts-docker-arm64", "expired": false, "digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "workflow_run": {"id": 123456789}}
]}
EOF
GATES="$TMP/gates"
mkdir -p "$GATES"
EVIDENCE="$GATES/release-evidence.json"
"$EVIDENCE_TOOL" generate "$SOURCE" "$REPOSITORY" "$TMP/ci-run.json" "$TMP/ci-jobs.json" "$TMP/artifacts.json" "$ARTIFACTS" "$EVIDENCE"
cp "$EVIDENCE" "$TMP/evidence-only.json"
INDEX_DIGEST="sha256:$(printf 'index' | sha256sum | awk '{print $1}')"
"$EVIDENCE_TOOL" record-images "$EVIDENCE" "docker.io/f1r3flyindustries/f1r3fly-rust@$INDEX_DIGEST" \
	"sha256:$(printf 'amd64' | sha256sum | awk '{print $1}')" "sha256:$(printf 'arm64' | sha256sum | awk '{print $1}')" "$INDEX_DIGEST"
cp "$TMP/ci-run.json" "$TMP/ci-jobs.json" "$GATES/"
run_doc .github/workflows/slashing-tests.yml 555 1 "$SOURCE_SHA" >"$GATES/slashing-run.json"
jq -n '{jobs: ([{name: "Example-based UC tests"}, {name: "Property-based theorem tests"}, {name: "Loom exhaustive interleaving (T-9.2)"},
	{name: "Pre-fix regression backstops (1)"}] | map(. + {status: "completed", conclusion: "success"}))}' >"$GATES/slashing-jobs.json"

expect_failure() {
	local label="$1"
	shift
	if "$@" >"$TMP/out" 2>"$TMP/err"; then
		printf 'expected failure: %s\n' "$label" >&2
		exit 1
	fi
}

# --- OCI validation document ------------------------------------------------
run_doc .github/workflows/oci-validation.yml 777 2 "$SOURCE_SHA" workflow_dispatch master >"$TMP/oci-run.json"
jq -n '{jobs: [
	{name: "Validate Candidate Input", status: "completed", conclusion: "success"},
	{name: "Run OCI Validation / Integration Tests (amd64-docker-1)", status: "completed", conclusion: "success"},
	{name: "Run OCI Validation / Integration Tests (amd64)", status: "completed", conclusion: "success"},
	{name: "Run OCI Validation / Integration Tests (arm64)", status: "completed", conclusion: "success"}]}' >"$TMP/oci-jobs.json"
"$TOOL" oci-validation "$EVIDENCE" "$TMP/oci-run.json" "$TMP/oci-jobs.json" "$GATES/oci-validation-evidence.json"
jq -e --arg sha "$SOURCE_SHA" --arg digest "$INDEX_DIGEST" --arg pin "$PIN" '
	.schema_version == 1 and .gate == "oci_validation" and .source_sha == $sha and .image_index_digest == $digest
	and .workflow_run == {id: 777, attempt: 2, path: ".github/workflows/oci-validation.yml", conclusion: "success"}
	and .mode == "candidate" and .system_integration_sha == $pin and (.required_jobs | length) == 2' "$GATES/oci-validation-evidence.json" >/dev/null
jq '(.jobs[] | select(.name | endswith("(arm64)")) | .conclusion) = "failure"' "$TMP/oci-jobs.json" >"$TMP/oci-jobs-failed.json"
expect_failure 'OCI document with a failed summary job' "$TOOL" oci-validation "$EVIDENCE" "$TMP/oci-run.json" "$TMP/oci-jobs-failed.json" "$TMP/x.json"
jq '.jobs |= map(select(.name | endswith("(arm64)") | not))' "$TMP/oci-jobs.json" >"$TMP/oci-jobs-missing.json"
expect_failure 'OCI document without the arm64 summary job' "$TOOL" oci-validation "$EVIDENCE" "$TMP/oci-run.json" "$TMP/oci-jobs-missing.json" "$TMP/x.json"
jq '.path = ".github/workflows/ci.yml"' "$TMP/oci-run.json" >"$TMP/oci-run-wrong.json"
expect_failure 'OCI document from another workflow' "$TOOL" oci-validation "$EVIDENCE" "$TMP/oci-run-wrong.json" "$TMP/oci-jobs.json" "$TMP/x.json"
expect_failure 'OCI document for an evidence-only candidate' "$TOOL" oci-validation "$TMP/evidence-only.json" "$TMP/oci-run.json" "$TMP/oci-jobs.json" "$TMP/x.json"

# --- Soak document and verdict ------------------------------------------------
run_doc .github/workflows/merge-recovery-soak.yml 888 1 "$SOURCE_SHA" workflow_dispatch master >"$TMP/soak-run.json"
jq -n '{soak_kind: "weekend", requested_duration_seconds: 216000, completed: true, artifact_mode: "candidate",
	retry_attempt: 0, coverage_preserved: true, preflight: {status: "success"}}' >"$TMP/soak-result.json"
"$TOOL" stability-soak "$EVIDENCE" "$TMP/soak-run.json" "$TMP/soak-result.json" "$GATES/soak-evidence.json"
jq -e '.gate == "stability_soak" and .soak_kind == "weekend" and .preflight.status == "success"' "$GATES/soak-evidence.json" >/dev/null
jq '.requested_duration_seconds = 86400' "$TMP/soak-result.json" >"$TMP/soak-short.json"
expect_failure 'soak document with a short duration' "$TOOL" stability-soak "$EVIDENCE" "$TMP/soak-run.json" "$TMP/soak-short.json" "$TMP/x.json"
jq '.artifact_mode = "source"' "$TMP/soak-result.json" >"$TMP/soak-source.json"
expect_failure 'soak document in source mode' "$TOOL" stability-soak "$EVIDENCE" "$TMP/soak-run.json" "$TMP/soak-source.json" "$TMP/x.json"
"$TOOL" verdict "$EVIDENCE" pass "$GATES/verdict.json"
expect_failure 'unknown verdict' "$TOOL" verdict "$EVIDENCE" maybe "$TMP/x.json"

# --- Candidate marker ---------------------------------------------------------
"$TOOL" candidate-marker "$EVIDENCE" "$TMP/release-candidate.json"
jq -e --arg sha "$SOURCE_SHA" '.candidate_tag == "v0.4.46-canary.789" and .source_sha == $sha' "$TMP/release-candidate.json" >/dev/null

# --- The evaluator accepts everything the writer produced ----------------------
"$GATES_TOOL" evaluate "$GATES" "$REPOSITORY" "$TMP/gate-report.json" 2>/dev/null
jq -e '.promotable == true' "$TMP/gate-report.json" >/dev/null

# A regress verdict from the writer holds promotion until review.
"$TOOL" verdict "$EVIDENCE" regress "$GATES/verdict.json"
status=0
"$GATES_TOOL" evaluate "$GATES" "$REPOSITORY" "$TMP/regress-report.json" 2>/dev/null || status=$?
[ "$status" -eq 10 ] || { printf 'regress verdict should hold, got exit %s\n' "$status" >&2; exit 1; }

printf 'release gate evidence tests passed\n'

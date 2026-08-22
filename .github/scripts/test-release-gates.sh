#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EVIDENCE_TOOL="$ROOT/.github/scripts/release-evidence.sh"
TOOL="$ROOT/.github/scripts/release-gates.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
REPOSITORY=example/repository
PIN=0123456789abcdef0123456789abcdef01234567

# --- Candidate evidence fixture, produced by the real evidence tool -------
SOURCE="$TMP/source"
ARTIFACTS="$TMP/artifacts"
mkdir -p "$SOURCE/node" "$SOURCE/.github" \
	"$ARTIFACTS/artifacts-docker-amd64" "$ARTIFACTS/artifacts-docker-arm64" "$ARTIFACTS/release"
printf '%s\n' '[package]' 'name = "node"' 'version = "0.4.46"' >"$SOURCE/node/Cargo.toml"
printf '%s\n' 'FROM scratch' 'LABEL version="0.4.46"' >"$SOURCE/node/Dockerfile"
printf '%s\n' 'version = 4' '[[package]]' 'name = "node"' 'version = "0.4.46"' >"$SOURCE/Cargo.lock"
printf 'SYSTEM_INTEGRATION_REF=%s\n' "$PIN" >"$SOURCE/.github/oci-validation.env"
git -C "$SOURCE" init -q
git -C "$SOURCE" config user.name 'Release Gates Test'
git -C "$SOURCE" config user.email 'test@example.com'
git -C "$SOURCE" add .
git -C "$SOURCE" commit -qm 'test fixture'
git -C "$SOURCE" tag v0.4.45
SOURCE_SHA="$(git -C "$SOURCE" rev-parse HEAD)"
printf 'amd64 archive\n' >"$ARTIFACTS/artifacts-docker-amd64/rust-node-docker.tar.gz"
printf 'arm64 archive\n' >"$ARTIFACTS/artifacts-docker-arm64/rust-node-docker.tar.gz"
printf 'amd64 binary\n' >"$ARTIFACTS/release/f1r3node-linux-amd64"
printf 'arm64 binary\n' >"$ARTIFACTS/release/f1r3node-linux-arm64"
run_doc() {
	local path="$1" id="$2" attempt="$3" sha="$4" event="${5:-push}" branch="${6:-master}" conclusion="${7:-success}"
	jq -n --arg path "$path" --argjson id "$id" --argjson attempt "$attempt" --arg sha "$sha" \
		--arg event "$event" --arg branch "$branch" --arg conclusion "$conclusion" --arg repo "$REPOSITORY" '{
		id: $id, run_number: ($id % 1000), run_attempt: $attempt, path: $path, event: $event,
		head_branch: $branch, status: "completed", conclusion: $conclusion, head_sha: $sha,
		repository: {full_name: $repo}, updated_at: "2026-08-16T00:00:00Z"}'
}
run_doc .github/workflows/ci.yml 123456789 2 "$SOURCE_SHA" >"$TMP/ci-run.json"
"$EVIDENCE_TOOL" required-jobs | jq '[to_entries[] | {
	id: (.key + 1000), name: .value, status: "completed", conclusion: "success",
	completed_at: ("2026-08-16T0" + ((.key % 10) | tostring) + ":00:00Z")}] | {jobs: .}' >"$TMP/ci-jobs.json"
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
"$EVIDENCE_TOOL" record-images "$GATES/release-evidence.json" \
	"docker.io/f1r3flyindustries/f1r3fly-rust@$INDEX_DIGEST" \
	"sha256:$(printf 'amd64' | sha256sum | awk '{print $1}')" \
	"sha256:$(printf 'arm64' | sha256sum | awk '{print $1}')"
CANDIDATE_TAG="$(jq -r '.candidate_tag' "$GATES/release-evidence.json")"

# --- Gate documents -------------------------------------------------------
cp "$TMP/ci-run.json" "$GATES/ci-run.json"
cp "$TMP/ci-jobs.json" "$GATES/ci-jobs.json"
run_doc .github/workflows/slashing-tests.yml 555 1 "$SOURCE_SHA" >"$GATES/slashing-run.json"
jq -n '{jobs: (
	[{name: "Example-based UC tests"}, {name: "Property-based theorem tests"}, {name: "Loom exhaustive interleaving (T-9.2)"}]
	+ [range(1; 12) | {name: ("Pre-fix regression backstops (" + tostring + ")")}]
	+ [{name: "TLA+ invariant check", status: "completed", conclusion: "skipped"}]
	| map(. + {status: "completed", conclusion: (.conclusion // "success")}))}' >"$GATES/slashing-jobs.json"
binding() {
	jq -n --arg gate "$1" --arg sha "$SOURCE_SHA" --arg tag "$CANDIDATE_TAG" --arg digest "$INDEX_DIGEST" \
		--arg path "$2" --argjson id "$3" '{
		schema_version: 1, gate: $gate, source_sha: $sha, candidate_tag: $tag, image_index_digest: $digest,
		workflow_run: {id: $id, attempt: 1, path: $path, conclusion: "success"}}'
}
binding oci_validation .github/workflows/oci-validation.yml 777 | jq --arg pin "$PIN" '. + {
	mode: "candidate", system_integration_sha: $pin,
	required_jobs: [{name: "OCI validation (amd64)", conclusion: "success"}]}' >"$GATES/oci-validation-evidence.json"
binding stability_soak .github/workflows/merge-recovery-soak.yml 888 | jq '. + {
	soak_kind: "weekend", requested_duration_seconds: 216000, completed: true, artifact_mode: "candidate",
	retry_attempt: 0, coverage_preserved: true, preflight: {status: "success"}}' >"$GATES/soak-evidence.json"
jq -n --arg sha "$SOURCE_SHA" '{schema_version: 1, source_sha: $sha, verdict: "pass"}' >"$GATES/verdict.json"

expect_exit() {
	local expected="$1" label="$2"
	shift 2
	local actual=0
	"$@" >"$TMP/out" 2>"$TMP/err" || actual=$?
	if [ "$actual" -ne "$expected" ]; then
		printf 'expected exit %s for %s, got %s\n' "$expected" "$label" "$actual" >&2
		cat "$TMP/err" >&2
		exit 1
	fi
}

# --- Promotable candidate ---------------------------------------------------
REPORT="$TMP/gate-report.json"
expect_exit 0 'all gates pass' "$TOOL" evaluate "$GATES" "$REPOSITORY" "$REPORT"
jq -e --arg tag "$CANDIDATE_TAG" --arg sha "$SOURCE_SHA" '
	.schema_version == 1 and .candidate_tag == $tag and .source_sha == $sha
	and .promotable == true and .held == false and .failed == false
	and (.gates | length) == 8
	and ([.gates[] | select(.status != "pass")] | length) == 0' "$REPORT" >/dev/null
"$TOOL" evaluate "$GATES" "$REPOSITORY" "$TMP/gate-report-repeat.json" >/dev/null 2>&1
cmp "$REPORT" "$TMP/gate-report-repeat.json"
"$TOOL" summarize "$REPORT" | grep -q 'The candidate is promotable'

# --- Held: missing evidence holds, never fails -------------------------------
HELD="$TMP/held"
cp -R "$GATES" "$HELD"
rm "$HELD/soak-evidence.json" "$HELD/oci-validation-evidence.json"
expect_exit 10 'missing soak and OCI evidence' "$TOOL" evaluate "$HELD" "$REPOSITORY" "$TMP/held-report.json"
jq -e '.held == true and .failed == false and .promotable == false
	and ([.gates[] | select(.status == "hold") | .id] | sort) == ["oci_validation", "soak_preflight", "stability_soak"]' "$TMP/held-report.json" >/dev/null
"$TOOL" summarize "$TMP/held-report.json" | grep -q 'Promotion is held'

# --- Regress verdict: held until review, pass with review, fail with rejection
REGRESS="$TMP/regress"
cp -R "$GATES" "$REGRESS"
jq '.verdict = "regress"' "$GATES/verdict.json" >"$REGRESS/verdict.json"
expect_exit 10 'regress verdict without review' "$TOOL" evaluate "$REGRESS" "$REPOSITORY" "$TMP/regress-report.json"
jq -e '[.gates[] | select(.id == "regression_verdict")][0].status == "hold"' "$TMP/regress-report.json" >/dev/null
jq -n --arg sha "$SOURCE_SHA" --arg tag "$CANDIDATE_TAG" '{
	source_sha: $sha, candidate_tag: $tag, verdict_accepted: true, reviewer: "maintainer",
	reference: "https://example.com/review/1", reviewed_at: "2026-08-16T01:00:00Z"}' >"$REGRESS/maintainer-review.json"
expect_exit 0 'regress verdict with accepted review' "$TOOL" evaluate "$REGRESS" "$REPOSITORY" "$TMP/regress-ok.json"
jq '.verdict_accepted = false' "$REGRESS/maintainer-review.json" >"$TMP/rejected.json"
mv "$TMP/rejected.json" "$REGRESS/maintainer-review.json"
expect_exit 20 'regress verdict with rejected review' "$TOOL" evaluate "$REGRESS" "$REPOSITORY" "$TMP/regress-rejected.json"

# --- Fail: evidence that contradicts the candidate --------------------------
fail_case() {
	local label="$1" file="$2" filter="$3" gate="$4"
	local dir="$TMP/fail-$RANDOM"
	cp -R "$GATES" "$dir"
	jq "$filter" "$GATES/$file" >"$dir/$file"
	expect_exit 20 "$label" "$TOOL" evaluate "$dir" "$REPOSITORY" "$dir/report.json"
	jq -e --arg gate "$gate" '[.gates[] | select(.id == $gate)][0].status == "fail" and .failed == true' "$dir/report.json" >/dev/null ||
		{ printf 'gate %s did not fail for %s\n' "$gate" "$label" >&2; exit 1; }
}
OTHER_SHA=fedcba9876543210fedcba9876543210fedcba98
fail_case 'CI run for another commit' ci-run.json ".head_sha = \"$OTHER_SHA\"" full_ci
fail_case 'CI run from a pull request' ci-run.json '.event = "pull_request"' full_ci
fail_case 'CI run attempt differs from evidence' ci-run.json '.run_attempt = 3' full_ci
fail_case 'required CI job failed' ci-jobs.json '(.jobs[] | select(.name == "Lint") | .conclusion) = "failure"' heavy_integration
fail_case 'required CI job id differs from evidence' ci-jobs.json '(.jobs[] | select(.name == "Lint") | .id) = 9999' heavy_integration
fail_case 'slashing run for another commit' slashing-run.json ".head_sha = \"$OTHER_SHA\"" slashing
fail_case 'slashing matrix job failed' slashing-jobs.json '(.jobs[] | select(.name == "Pre-fix regression backstops (7)") | .conclusion) = "failure"' slashing
fail_case 'slashing matrix absent' slashing-jobs.json '.jobs |= map(select(.name | startswith("Pre-fix") | not))' slashing
fail_case 'OCI validation on another image' oci-validation-evidence.json '.image_index_digest = "sha256:0000000000000000000000000000000000000000000000000000000000000000"' oci_validation
fail_case 'OCI validation in pull-request mode' oci-validation-evidence.json '.mode = "pull-request"' oci_validation
fail_case 'OCI validation with another system-integration pin' oci-validation-evidence.json ".system_integration_sha = \"$OTHER_SHA\"" oci_validation
fail_case 'soak on another image' soak-evidence.json '.image_index_digest = "sha256:0000000000000000000000000000000000000000000000000000000000000000"' stability_soak
fail_case 'soak with short duration' soak-evidence.json '.requested_duration_seconds = 86400' stability_soak
fail_case 'soak without completion marker' soak-evidence.json '.completed = false' stability_soak
fail_case 'soak restarted twice' soak-evidence.json '.retry_attempt = 2' stability_soak
fail_case 'soak restart without full coverage' soak-evidence.json '.retry_attempt = 1 | .coverage_preserved = false' stability_soak
fail_case 'soak in source mode' soak-evidence.json '.artifact_mode = "source"' stability_soak
fail_case 'preflight failed' soak-evidence.json '.preflight.status = "failure"' soak_preflight
fail_case 'verdict for another commit' verdict.json ".source_sha = \"$OTHER_SHA\"" regression_verdict

# --- Evidence-only candidates cannot be promoted -----------------------------
NOIMG="$TMP/noimg"
cp -R "$GATES" "$NOIMG"
"$EVIDENCE_TOOL" generate "$SOURCE" "$REPOSITORY" "$TMP/ci-run.json" "$TMP/ci-jobs.json" "$TMP/artifacts.json" "$ARTIFACTS" "$NOIMG/release-evidence.json"
expect_exit 1 'evidence-only candidate' "$TOOL" evaluate "$NOIMG" "$REPOSITORY" "$NOIMG/report.json"

printf 'release gates tests passed\n'

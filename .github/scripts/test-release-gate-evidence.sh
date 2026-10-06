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
unset RELEASE_SHARD_SOAK_IN_GATE
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

# --- Test net candidate marker ------------------------------------------------
"$TOOL" test-net-candidate "$EVIDENCE" "$TMP/soak-run.json" "$GATES/soak-evidence.json" "$GATES/verdict.json" "$TMP/test-net-candidate.json"
jq -e --arg sha "$SOURCE_SHA" --arg digest "$INDEX_DIGEST" --arg soak_sha "$(sha256sum "$GATES/soak-evidence.json" | awk '{print $1}')" '
	.schema_version == 1 and .gate == "test_net_candidate" and .source_sha == $sha and .image_index_digest == $digest
	and .workflow_run == {id: 888, attempt: 1, path: ".github/workflows/merge-recovery-soak.yml", conclusion: "success"}
	and .consensus_model == "cbc-casper" and .soak_evidence_sha256 == $soak_sha
	and .verdict == "pass" and .maintainer_review_reference == null' "$TMP/test-net-candidate.json" >/dev/null
"$TOOL" verdict "$EVIDENCE" regress "$TMP/verdict-regress.json"
expect_failure 'test net candidate with a regress verdict and no review' \
	"$TOOL" test-net-candidate "$EVIDENCE" "$TMP/soak-run.json" "$GATES/soak-evidence.json" "$TMP/verdict-regress.json" "$TMP/x.json"
jq -n --arg sha "$SOURCE_SHA" '{source_sha: $sha, candidate_tag: "v0.4.46-canary.789", verdict_accepted: true,
	reviewer: "maintainer-a", reference: "https://example.com/review/1", reviewed_at: "2026-10-06T00:00:00Z"}' >"$TMP/review.json"
jq -n '{login: "maintainer-a", permission: "maintain"}' >"$TMP/review-permission.json"
"$TOOL" test-net-candidate "$EVIDENCE" "$TMP/soak-run.json" "$GATES/soak-evidence.json" "$TMP/verdict-regress.json" "$TMP/tnc-reviewed.json" \
	"$TMP/review.json" "$TMP/review-permission.json"
jq -e '.verdict == "regress" and .maintainer_review_reference == "https://example.com/review/1"' "$TMP/tnc-reviewed.json" >/dev/null
jq '.permission = "write"' "$TMP/review-permission.json" >"$TMP/review-permission-write.json"
expect_failure 'test net candidate with a reviewer below maintain' \
	"$TOOL" test-net-candidate "$EVIDENCE" "$TMP/soak-run.json" "$GATES/soak-evidence.json" "$TMP/verdict-regress.json" "$TMP/x.json" \
	"$TMP/review.json" "$TMP/review-permission-write.json"
run_doc .github/workflows/merge-recovery-soak.yml 889 1 "$SOURCE_SHA" workflow_dispatch master >"$TMP/other-soak-run.json"
expect_failure 'test net candidate for a soak document from another run' \
	"$TOOL" test-net-candidate "$EVIDENCE" "$TMP/other-soak-run.json" "$GATES/soak-evidence.json" "$GATES/verdict.json" "$TMP/x.json"
jq '.completed = false' "$GATES/soak-evidence.json" >"$TMP/soak-incomplete.json"
expect_failure 'test net candidate for an incomplete soak' \
	"$TOOL" test-net-candidate "$EVIDENCE" "$TMP/soak-run.json" "$TMP/soak-incomplete.json" "$GATES/verdict.json" "$TMP/x.json"
jq '.preflight.status = "failure"' "$GATES/soak-evidence.json" >"$TMP/soak-preflight-failed.json"
expect_failure 'test net candidate after a failed preflight' \
	"$TOOL" test-net-candidate "$EVIDENCE" "$TMP/soak-run.json" "$TMP/soak-preflight-failed.json" "$GATES/verdict.json" "$TMP/x.json"
jq '.source_sha = "0000000000000000000000000000000000000000"' "$GATES/verdict.json" >"$TMP/verdict-other-source.json"
expect_failure 'test net candidate with a verdict for another source' \
	"$TOOL" test-net-candidate "$EVIDENCE" "$TMP/soak-run.json" "$GATES/soak-evidence.json" "$TMP/verdict-other-source.json" "$TMP/x.json"
expect_failure 'test net candidate for an evidence-only candidate' \
	"$TOOL" test-net-candidate "$TMP/evidence-only.json" "$TMP/soak-run.json" "$GATES/soak-evidence.json" "$GATES/verdict.json" "$TMP/x.json"

# --- The evaluator accepts everything the writer produced ----------------------
# The workflow fetches the run each document names; here the run fixtures
# are the same documents the writer consumed.
cp "$TMP/oci-run.json" "$GATES/oci-validation-run.json"
cp "$TMP/soak-run.json" "$GATES/soak-run.json"
jq -e --arg esha "$(sha256sum "$EVIDENCE" | awk '{print $1}')" '.candidate_evidence_sha256 == $esha' "$GATES/oci-validation-evidence.json" >/dev/null
"$GATES_TOOL" evaluate "$GATES" "$REPOSITORY" "$TMP/gate-report.json" 2>/dev/null
jq -e '.promotable == true' "$TMP/gate-report.json" >/dev/null

# --- Test net candidate verification agrees with the writer --------------------
TNC="$TMP/tnc-gates"
cp -R "$GATES" "$TNC"
cp "$TMP/test-net-candidate.json" "$TNC/test-net-candidate.json"
"$GATES_TOOL" verify-test-net-candidate "$TNC" "$REPOSITORY" "$TMP/tnc-report.json" 2>/dev/null
jq -e '.eligible == true and .consensus_model == "cbc-casper"' "$TMP/tnc-report.json" >/dev/null
expect_tnc_status() {
	local expected="$1" label="$2" dir="$3" actual=0
	"$GATES_TOOL" verify-test-net-candidate "$dir" "$REPOSITORY" "$dir/report.json" >/dev/null 2>&1 || actual=$?
	[ "$actual" -eq "$expected" ] || { printf 'expected exit %s for %s, got %s\n' "$expected" "$label" "$actual" >&2; exit 1; }
	jq -e '.eligible == false' "$dir/report.json" >/dev/null
}
tnc_case() {
	local expected="$1" label="$2" dir="$TMP/tnc-$RANDOM"
	cp -R "$TNC" "$dir"
	shift 2
	"$@" "$dir"
	expect_tnc_status "$expected" "$label" "$dir"
}
drop_marker() { rm "$1/test-net-candidate.json"; }
drop_soak_run() { rm "$1/soak-run.json"; }
tamper_soak_doc() { jq '.soak_kind = "daily"' "$TNC/soak-evidence.json" >"$1/soak-evidence.json"; }
regress_verdict() { jq '.verdict = "regress"' "$TNC/verdict.json" >"$1/verdict.json"; }
other_model() { jq '.consensus_model = "other-model"' "$TNC/test-net-candidate.json" >"$1/test-net-candidate.json"; }
other_run() { jq '.workflow_run.id = 889' "$TNC/test-net-candidate.json" >"$1/test-net-candidate.json"; }
tnc_case 10 'canary without a test net candidate marker' drop_marker
tnc_case 10 'marker before API run verification' drop_soak_run
tnc_case 20 'marker whose soak document changed' tamper_soak_doc
tnc_case 10 'marker beside a regress verdict without review' regress_verdict
tnc_case 20 'marker in another consensus model' other_model
tnc_case 20 'marker naming another soak run' other_run

# A document built from different evidence is refused by the evaluator even
# though every other field matches: the run must carry its verified file.
TAMPERED="$TMP/tampered-evidence"
cp -R "$GATES" "$TAMPERED"
jq '.created_at = "2026-08-16T00:00:01Z"' "$EVIDENCE" >"$TAMPERED/release-evidence.json"
status=0
"$GATES_TOOL" evaluate "$TAMPERED" "$REPOSITORY" "$TMP/tampered-report.json" 2>/dev/null || status=$?
[ "$status" -eq 20 ] || { printf 'evidence substitution should fail, got exit %s\n' "$status" >&2; exit 1; }

# A regress verdict from the writer holds promotion until review.
"$TOOL" verdict "$EVIDENCE" regress "$GATES/verdict.json"
status=0
"$GATES_TOOL" evaluate "$GATES" "$REPOSITORY" "$TMP/regress-report.json" 2>/dev/null || status=$?
[ "$status" -eq 10 ] || { printf 'regress verdict should hold, got exit %s\n' "$status" >&2; exit 1; }

printf 'release gate evidence tests passed\n'

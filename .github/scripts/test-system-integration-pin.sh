#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
READER="$ROOT/.github/scripts/read-system-integration-pin.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin"
candidate=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
control=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
revision=cccccccccccccccccccccccccccccccccccccccc
printf 'SYSTEM_INTEGRATION_REF=%s\n' "$candidate" >"$TMP/candidate.env"
printf 'SYSTEM_INTEGRATION_REF=%s\n' "$control" >"$TMP/control.env"
[ "$(bash "$READER" "$TMP/candidate.env")" = "$candidate" ]
[ "$(bash "$READER" "$TMP/control.env")" = "$control" ]
[ "$(bash "$READER" - <"$TMP/candidate.env")" = "$candidate" ]
printf 'touch "%s/EXECUTED"\nSYSTEM_INTEGRATION_REF=%s\n' "$TMP" "$candidate" >"$TMP/data.env"
[ "$(bash "$READER" "$TMP/data.env")" = "$candidate" ]
[ ! -e "$TMP/EXECUTED" ]
printf 'SYSTEM_INTEGRATION_REF=%s' "$candidate" >"$TMP/no-newline.env"
[ "$(bash "$READER" "$TMP/no-newline.env")" = "$candidate" ]
printf 'SYSTEM_INTEGRATION_REF=refs/tags/emergency-pin\n' >"$TMP/tag.env"
[ "$(bash "$READER" "$TMP/tag.env")" = refs/tags/emergency-pin ]

reject() {
	if bash "$READER" "$@" >"$TMP/output" 2>"$TMP/error"; then
		printf 'FAIL: accepted invalid pin input\n' >&2
		exit 1
	fi
	[ ! -s "$TMP/output" ]
}

printf -v payload '\044(touch SENTINEL)'
for value in '' main master deadbeef AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA 'refs/heads/dev' "$payload"; do
	printf 'SYSTEM_INTEGRATION_REF=%s\n' "$value" >"$TMP/invalid.env"
	(cd "$TMP" && reject "$TMP/invalid.env")
done
[ ! -e "$TMP/SENTINEL" ]
cat "$TMP/candidate.env" "$TMP/control.env" >"$TMP/duplicate.env"
reject "$TMP/duplicate.env"
printf '\nSYSTEM_INTEGRATION_REF=\n' >>"$TMP/candidate.env"
reject "$TMP/candidate.env"
reject "$TMP/missing.env"
: >"$TMP/empty.env"
reject "$TMP/empty.env"

cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[ "$*" = "api --method GET -H Accept: application/vnd.github.raw+json repos/example/node/contents/.github/oci-validation.env?ref=cccccccccccccccccccccccccccccccccccccccc" ]
[ "${GITHUB_TOKEN-unset}" = unset ]
[ "$GH_TOKEN" = fixture-read-token ]
[ "${FAIL_API:-0}" != 1 ] || exit 1
printf 'SYSTEM_INTEGRATION_REF=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n'
SH
chmod +x "$TMP/bin/gh"
export PATH="$TMP/bin:$PATH" GH_TOKEN=fixture-read-token GITHUB_TOKEN=fixture-inherited-token
[ "$(bash "$READER" --repository example/node "$revision")" = "$candidate" ]
FAIL_API=1 reject --repository example/node "$revision"
reject --repository example/node master
reject --repository example/node 'cccccccccccccccccccccccccccccccccccccccc?other=1'
reject --repository ../node "$revision"

mkdir -p "$TMP/work/.github/scripts"
cp "$READER" "$TMP/work/.github/scripts/"
cp "$TMP/control.env" "$TMP/work/.github/oci-validation.env"
ruby -ryaml - "$ROOT" "$TMP" <<'RUBY'
root, temporary = ARGV
{
  "resolve" => ["_integration-pipeline.yml", "resolve_harness", "pin"],
  "pr" => ["oci-validation.yml", "validate_pr", "pr"],
  "launcher" => ["oci-validation.yml", "validate_pr", "launcher"],
  "soak" => ["merge-recovery-soak.yml", "soak", "pin"]
}.each do |name, (file, job, id)|
  workflow = YAML.load_file(File.join(root, ".github/workflows", file))
  step = workflow.fetch("jobs").fetch(job).fetch("steps").find { |item| item["id"] == id }
  File.write(File.join(temporary, "#{name}.sh"), step.fetch("run"))
end
RUBY
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
case "$*" in
	'api repos/example/node/collaborators/maintainer/permission --jq .permission') printf 'maintain\n' ;;
	'api repos/example/node/pulls/7')
		printf '{"state":"open","head":{"repo":{"full_name":"example/node"},"sha":"cccccccccccccccccccccccccccccccccccccccc","ref":"ci/test-pin"},"base":{"ref":"dev"}}\n' ;;
	'api repos/example/node/issues/comments/9')
		printf '{"issue_url":"https://api.github.com/repos/example/node/issues/7","user":{"login":"maintainer"},"body":"/full-oci-validate 7 cccccccccccccccccccccccccccccccccccccccc"}\n' ;;
	'api --method GET -H Accept: application/vnd.github.raw+json repos/example/node/contents/.github/oci-validation.env?ref=cccccccccccccccccccccccccccccccccccccccc')
		[ "${FAIL_API:-0}" != 1 ] || exit 1
		printf 'SYSTEM_INTEGRATION_REF=%s\n' "${FAKE_PIN:-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}" ;;
	*) printf 'Unexpected GitHub request: %s\n' "$*" >&2; exit 1 ;;
esac
SH
workflow_case() {
	: >"$TMP/resolved"
	(cd "$TMP/work" && env -u GITHUB_TOKEN \
		SOURCE_REPOSITORY=example/node SOURCE_SHA="$revision" \
		GH_REPO=example/node PR_NUMBER=7 EXPECTED_HEAD_SHA="$revision" \
		APPROVAL_COMMENT_ID=9 ACTOR=maintainer GITHUB_OUTPUT="$TMP/resolved" \
		bash -euo pipefail "$TMP/$1.sh")
}
workflow_case launcher
grep -Fxq "ref=$control" "$TMP/resolved"
cp "$TMP/data.env" "$TMP/work/.github/oci-validation.env"
(cd "$TMP/work" && WORKFLOW_PIN="$control" GITHUB_OUTPUT="$TMP/soak-output" bash -euo pipefail "$TMP/soak.sh")
grep -Fxq "system_integration_ref=$candidate" "$TMP/soak-output"
[ ! -e "$TMP/EXECUTED" ]
cp "$TMP/control.env" "$TMP/work/.github/oci-validation.env"
for workflow in resolve pr; do
	workflow_case "$workflow"
	grep -Fxq "system_integration_ref=$candidate" "$TMP/resolved"
	FAKE_PIN="$control" workflow_case "$workflow"
	grep -Fxq "system_integration_ref=$control" "$TMP/resolved"
	for mode in missing malformed; do
		if (export FAIL_API=0 FAKE_PIN="$candidate"
			if [ "$mode" = missing ]; then export FAIL_API=1; else export FAKE_PIN=master; fi
			workflow_case "$workflow"); then
			printf 'FAIL: %s accepted a %s candidate pin\n' "$workflow" "$mode" >&2
			exit 1
		fi
		[ ! -s "$TMP/resolved" ]
	done
done
printf 'PASS: candidate pins are read as data from exact revisions, with no control-pin fallback.\n'

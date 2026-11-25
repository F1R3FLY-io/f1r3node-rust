#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

ruby -ryaml -e '
  doc = YAML.load_file(ARGV[0])
  env_step = doc.dig("jobs", "build_base", "steps").find { |item| item["id"] == "env" }
  abort "build_base env step not found" unless env_step && env_step["run"].is_a?(String)
  branch = env_step["run"][/# Derive BRANCH.*?echo "BRANCH=\$BRANCH" >> "\$GITHUB_OUTPUT"\n/m]
  abort "BRANCH derivation not found" unless branch
  File.write(File.join(ARGV[1], "branch.sh"), branch)
  steps = doc.dig("jobs", "release_docker_image", "steps")
  { "Publish Docker Image" => "dockerhub.sh", "Publish to OCIR" => "ocir.sh" }.each do |name, file|
    step = steps.find { |item| item["name"] == name }
    abort "#{name} step not found" unless step && step["run"].is_a?(String)
    run = step["run"].gsub(/\$\{\{\s*secrets\.(\w+)\s*\}\}/) { "fake-#{$1.downcase}" }
    File.write(File.join(ARGV[1], file), run)
  end
' "$ROOT/.github/workflows/ci.yml" "$TMP"

mkdir -p "$TMP/bin"
cat >"$TMP/bin/docker" <<'FAKE'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$DOCKER_LOG"
FAKE
chmod +x "$TMP/bin/docker"

cat >"$TMP/branch-tip-guard.sh" <<'GUARD'
publish_mutable() { "$@"; }
GUARD

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

# publish LABEL REF VERSION: derives BRANCH from REF as build_base does, then
# runs both publish steps and leaves the pushed references in $TMP/pushed.
publish() {
	local label="$1" ref="$2" version="$3" branch
	branch="$(GITHUB_REF="$ref" GITHUB_OUTPUT=/dev/stdout bash "$TMP/branch.sh" | sed -n 's/^BRANCH=//p')"
	: >"$TMP/docker.log"
	for step in dockerhub ocir; do
		PATH="$TMP/bin:$PATH" DOCKER_LOG="$TMP/docker.log" RUNNER_TEMP="$TMP" \
			DOCKER_IMAGE_NAME=f1r3flyindustries/f1r3fly-rust GITHUB_REF="$ref" GITHUB_RUN_NUMBER=7 \
			VERSION="$version" BRANCH="$branch" GH_TOKEN=test \
			bash -eu -o pipefail "$TMP/$step.sh" >/dev/null 2>&1 || fail "$label: the $step step failed"
	done
	sed -n 's/^manifest push //p' "$TMP/docker.log" >"$TMP/pushed"
}

expect_pushed() {
	grep -Fxq "$2" "$TMP/pushed" || fail "$1: $2 was not pushed: $(tr '\n' ' ' <"$TMP/pushed")"
}

expect_not_pushed() {
	if grep -Eq "$2" "$TMP/pushed"; then
		fail "$1: a reference matching $2 was pushed: $(tr '\n' ' ' <"$TMP/pushed")"
	fi
}

HUB=f1r3flyindustries/f1r3fly-rust
OCIR=fake-ocir_registry/fake-ocir_namespace/fake-ocir_repo

publish dev-push refs/heads/dev v0.4.23-5-gabc1234
expect_pushed dev-push "$HUB:dev-v0.4.23-5-gabc1234"
expect_pushed dev-push "$HUB:dev"
expect_pushed dev-push "$OCIR:dev"
expect_not_pushed dev-push ':latest$'

publish master-push refs/heads/master v0.4.23-5-gabc1234
expect_pushed master-push "$HUB:v0.4.23-5-gabc1234"
expect_pushed master-push "$HUB:latest"

publish version-tag refs/tags/v0.4.24 v0.4.24
expect_pushed version-tag "$HUB:v0.4.24"
expect_pushed version-tag "$HUB:latest"
expect_pushed version-tag "$OCIR:latest"

publish ca-release-push refs/heads/ca-release v0.4.23-9-gdef5678
expect_pushed ca-release-push "$HUB:ca-release-v0.4.23-9-gdef5678"
expect_pushed ca-release-push "$HUB:ca-release"
expect_pushed ca-release-push "$OCIR:ca-release-v0.4.23-9-gdef5678"
expect_pushed ca-release-push "$OCIR:ca-release"
expect_not_pushed ca-release-push ':(latest|dev)$'

publish ca-tag refs/tags/ca-v0.5.0-rc.1 ca-v0.5.0-rc.1
expect_pushed ca-tag "$HUB:ca-v0.5.0-rc.1"
expect_pushed ca-tag "$HUB:ca-latest"
expect_pushed ca-tag "$OCIR:ca-v0.5.0-rc.1"
expect_pushed ca-tag "$OCIR:ca-latest"
expect_not_pushed ca-tag ':(latest|dev)$'

printf 'PASS: the image publish steps tag dev, master, and v* as before, tag a ca-release push as ca-release-<version> with the :ca-release pointer, tag a ca-v* tag push as :<tag> with the :ca-latest pointer, and never move :latest or :dev for a ca-* ref.\n'

#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/check-ca-daily-provenance.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
export GIT_AUTHOR_NAME=Test GIT_AUTHOR_EMAIL=test@example.com
export GIT_COMMITTER_NAME=Test GIT_COMMITTER_EMAIL=test@example.com

git init --quiet "$TMP/repo"
cd "$TMP/repo"

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

tree() {
	local path content blob
	while [ "$#" -gt 0 ]; do
		path="$1" content="$2"
		shift 2
		blob="$(printf '%s\n' "$content" | git hash-object -w --stdin)"
		printf '100644 blob %s\t%s\n' "$blob" "$path"
	done | git mktree
}

commit() {
	local tree="$1"
	shift
	printf 'fixture\n' | git -c commit.gpgsign=false commit-tree "$tree" "$@"
}

root="$(commit "$(tree common.txt initial)")"
source="$(commit "$(tree common.txt initial release.txt release)" -p "$root")"
base="$(commit "$(tree common.txt initial base.txt first)" -p "$root")"
base2="$(commit "$(tree common.txt initial base.txt second)" -p "$base")"
merge="$(commit "$(git merge-tree --write-tree "$source" "$base")" -p "$source" -p "$base")"
merge2="$(commit "$(git merge-tree --write-tree "$merge" "$base2")" -p "$merge" -p "$base2")"
release2="$(commit "$(tree common.txt initial release.txt newer)" -p "$source")"
foreign="$(commit "$(tree common.txt initial foreign.txt foreign)" -p "$root")"
extra="$(commit "$(tree common.txt initial release.txt changed)" -p "$source")"
forged="$(commit "$(tree common.txt initial release.txt forged)" -p "$source" -p "$base")"
foreign_merge="$(commit "$(git merge-tree --write-tree "$source" "$foreign")" -p "$source" -p "$foreign")"
octopus="$(commit "$(git rev-parse "$merge^{tree}")" -p "$source" -p "$base" -p "$foreign")"
conflict_base="$(commit "$(tree common.txt canary)" -p "$root")"
conflict_source="$(commit "$(tree common.txt release)" -p "$root")"
conflict_merge="$(commit "$(tree common.txt resolved)" -p "$conflict_source" -p "$conflict_base")"

run() {
	set +e
	out="$(HEAD_REF="${TEST_HEAD_REF:-backmerge/ca-release-${source:0:12}}" HEAD_SHA="$1" \
		BASE_SHA="${TEST_BASE_SHA:-$base2}" RELEASE_SHA="${TEST_RELEASE_SHA:-$release2}" \
		HEAD_REPO="${TEST_HEAD_REPO:-example/repo}" REPOSITORY=example/repo \
		PR_AUTHOR="${TEST_PR_AUTHOR:-f1r3node-promotion[bot]}" bash "$TOOL" 2>&1)"
	status=$?
	set -e
}

accept() {
	run "$1"
	[ "$status" -eq 0 ] || fail "$2: $out"
}

reject() {
	run "$1"
	[ "$status" -ne 0 ] || fail "$2 was accepted"
	[[ "$out" == *'::error::'* ]] || fail "$2 has no diagnostic: $out"
}

accept "$source" 'the named source'
accept "$merge" 'one clean base merge'
accept "$merge2" 'repeated clean base merges'
TEST_RELEASE_SHA="$source" accept "$source" 'the current release head'
reject "$extra" 'an extra single-parent commit'
reject "$forged" 'an altered merge tree'
reject "$foreign_merge" 'an unrelated merge parent'
reject "$octopus" 'an octopus merge'
TEST_RELEASE_SHA="$root" reject "$source" 'a source outside ca-release'
TEST_HEAD_REPO=other/fork reject "$source" 'a fork head'
TEST_PR_AUTHOR=other-user reject "$source" 'a different author'
TEST_HEAD_REF=backmerge/ca-release-feature reject "$source" 'an invalid source name'
TEST_HEAD_REF=backmerge/ca-release-ffffffffffff reject "$source" 'a missing source'
TEST_BASE_SHA=invalid reject "$merge" 'an invalid base SHA'
TEST_BASE_SHA=ffffffffffffffffffffffffffffffffffffffff reject "$merge" 'a missing base object'
TEST_RELEASE_SHA=invalid reject "$source" 'an invalid release SHA'
reject invalid 'an invalid head SHA'
TEST_HEAD_REF="backmerge/ca-release-${conflict_source:0:12}" TEST_RELEASE_SHA="$conflict_source" \
	TEST_BASE_SHA="$conflict_base" reject "$conflict_merge" 'a manual conflict resolution'

head="$source"
for ((i = 0; i < 129; i++)); do
	head="$(commit "$(git merge-tree --write-tree "$head" "$base")" -p "$head" -p "$base")"
	if [ "$i" -eq 127 ]; then
		accept "$head" 'the 128-merge boundary'
	fi
done
reject "$head" 'an overlong merge chain'

WORKFLOW="$ROOT/.github/workflows/ca-daily-provenance.yml"
dollar='$'
for text in \
	'pull_request_target:' \
	"ref: ${dollar}{{ github.event.pull_request.base.sha }}" \
	"HEAD_SHA: ${dollar}{{ github.event.pull_request.head.sha }}" \
	'contents: read' \
	'statuses: write' \
	'persist-credentials: false' \
	'if: always()' \
	'-f context="CA Daily Provenance"'; do
	grep -Fq -- "$text" "$WORKFLOW" || fail "the trusted workflow has no $text"
done
if grep -Eq 'ref:.*(head.sha|head.ref)|refs/pull/.*/merge|secrets\.' "$WORKFLOW"; then
	fail 'the trusted workflow checks out pull request code or reads a repository secret'
fi
awk '
  /^  - name: ca-daily$/ { daily = 1 }
  /^pull_request_rules:/ { daily = 0 }
  daily && /check-success=CA Daily Provenance/ { found = 1 }
  END { exit !found }
' "$ROOT/.mergify.yml" || fail 'the daily queue does not require the trusted status'

extract_step() {
	awk -v step="$1" '
	  /^      - name:/ { selected = ($0 == "      - name: " step); body = 0 }
	  selected && /^        run: \|$/ { body = 1; next }
	  body && /^          / { print substr($0, 11) }
	' "$WORKFLOW"
}

extract_step 'Mark the head pending' > "$TMP/pending.sh"
extract_step 'Publish the head-bound result' > "$TMP/result.sh"
[[ -s "$TMP/pending.sh" && -s "$TMP/result.sh" ]] || fail 'the status steps are missing'
mkdir "$TMP/bin"
cat > "$TMP/bin/gh" <<'FAKE'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$GH_LOG"
exit "${GH_FAIL_STATUS:-0}"
FAKE
chmod +x "$TMP/bin/gh"

post() {
	: > "$TMP/status.log"
	PATH="$TMP/bin:$PATH" GH_LOG="$TMP/status.log" HEAD_SHA="$source" \
		GITHUB_SHA="$base" REPOSITORY=example/repo SERVER_URL=https://github.com \
		RUN_ID=123 VALIDATION="$1" JOB_STATUS="$2" bash -euo pipefail "$TMP/$3.sh" || return $?
	grep -Fq "api repos/example/repo/statuses/$source " "$TMP/status.log" ||
		fail 'the status does not name the event head SHA'
	grep -Fq -- '-f context=CA Daily Provenance' "$TMP/status.log" ||
		fail 'the trusted status context changed'
}

post '' '' pending
grep -Fq -- '-f state=pending' "$TMP/status.log" || fail 'the head was not marked pending'
for outcomes in success:success success:cancelled failure:success skipped:success :success; do
	post "${outcomes%%:*}" "${outcomes#*:}" result
	state=failure
	[ "$outcomes" != success:success ] || state=success
	grep -Fq -- "-f state=$state" "$TMP/status.log" || fail "wrong status for $outcomes"
done
if GH_FAIL_STATUS=1 post success success result; then
	fail 'a status API error was ignored'
fi

printf 'PASS: daily provenance accepts release sources and clean base merge chains, rejects unverified changes and metadata, and requires a trusted head-bound status.\n'

#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }

awk '
	/^          unshallow=\(\)/ { capture = 1 }
	capture && /^  [^ ]/ { exit }
	capture { sub(/^          /, ""); print }
' "$ROOT/.github/workflows/ci.yml" >"$TMP/check.sh"
[ -s "$TMP/check.sh" ] || fail 'the workflow check is missing'
mkdir -p "$TMP/bin" "$TMP/runner" "$TMP/repo"
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[ "${FAIL_API:-0}" = 0 ] || exit 19
shift
if [ "${1:-}" = --paginate ]; then shift; fi
endpoint="$1"
shift
[ "$1" = --jq ]
case "$endpoint" in
    repos/example/repo/pulls/20) jq -r "$2" "$FIXTURE_DIR/pull.json" ;;
    repos/example/repo/pulls/20/commits\?per_page=100) printf '[]' | jq -r "$2" ;;
    repos/example/repo/branches/*)
        [ "${FAIL_SOURCE:-0}" = 0 ] || exit 19
        [ "${endpoint##*/}" = "$EXPECTED_SOURCE" ]
        jq -r "$2" "$FIXTURE_DIR/source.json"
        ;;
    *) exit 18 ;;
esac
SH
chmod +x "$TMP/bin/gh"
git init -q --bare "$TMP/origin.git"
git -C "$TMP/origin.git" config uploadpack.allowFilter true
git init -q "$TMP/repo"
git -C "$TMP/repo" config user.name 'Policy Test'
git -C "$TMP/repo" config user.email 'base@example.com'
git -C "$TMP/repo" config core.hooksPath /dev/null
git -C "$TMP/repo" remote add origin "$TMP/origin.git"
mkdir -p "$TMP/repo/.github/scripts"
cp "$ROOT/.github/scripts/commit-policy-exemptions.sh" "$TMP/repo/.github/scripts/"
printf '%s\n' 'echo legacy-target-checker >&2' 'exit 97' >"$TMP/repo/.github/scripts/check-commit-trailers.sh"
printf '%s\n' 'target-denied@example.com' >"$TMP/repo/.github/denied-identities.txt"
git -C "$TMP/repo" add .github
git -C "$TMP/repo" commit -q -m legacy-target
BASE="$(git -C "$TMP/repo" rev-parse HEAD)"
for file in commit-policy-source.sh check-commit-trailers.sh; do
	cp "$ROOT/.github/scripts/$file" "$TMP/repo/.github/scripts/$file"
done
printf '%s\n' 'source-denied@example.com' >"$TMP/repo/.github/denied-identities.txt"
git -C "$TMP/repo" add .github
git -C "$TMP/repo" commit -q --author='Sync Bot <sync-bot@example.com>' -m accepted-source-policy
ACCEPTED="$(git -C "$TMP/repo" rev-parse HEAD)"
printf '%s\n' 'exit 91' >"$TMP/repo/.github/scripts/commit-policy-source.sh"
git -C "$TMP/repo" add .github
git -C "$TMP/repo" commit -q -m later-source-policy
TIP="$(git -C "$TMP/repo" rev-parse HEAD)"
git -C "$TMP/repo" push -q origin HEAD:refs/heads/dev

BASE_SHA="$BASE"
HEAD_SHA="$ACCEPTED"
BASE_REF=ca-canary
HEAD_REF=ca/sync-dev-snapshot
PR_AUTHOR='f1r3node-promotion[bot]'
PR_AUTHOR_TYPE=Bot
SOURCE_TIP="$TIP"

fixture() {
	jq -n --arg base "$BASE_REF" --arg head "$HEAD_REF" --arg base_sha "$BASE_SHA" \
		--arg head_sha "$HEAD_SHA" --arg author "$PR_AUTHOR" --arg type "$PR_AUTHOR_TYPE" \
		'{base: {repo: {full_name: "example/repo"}, ref: $base, sha: $base_sha},
		  head: {repo: {full_name: "example/repo"}, ref: $head, sha: $head_sha},
		  user: {login: $author, type: $type}, body: ""}' >"$TMP/pull.json"
	jq -n --arg sha "$SOURCE_TIP" '{protected: true, commit: {sha: $sha}}' >"$TMP/source.json"
}

run() {
	local merge_sha
	merge_sha="$(git -C "$TMP/repo" commit-tree "$HEAD_SHA^{tree}" \
		-p "${MERGE_BASE_SHA:-$BASE_SHA}" -p "${MERGE_HEAD_SHA:-$HEAD_SHA}" -m proposed-merge)"
	git -C "$TMP/repo" push -q --force origin "$merge_sha:refs/pull/20/merge"
	set +e
	(cd "${CHECKOUT_REPO:-$TMP/repo}" && env PATH="$TMP/bin:$PATH" FIXTURE_DIR="$TMP" \
		GITHUB_REPOSITORY=example/repo PR_NUMBER=20 BASE_SHA="$BASE_SHA" HEAD_SHA="$HEAD_SHA" \
		MERGE_SHA="$merge_sha" \
		BASE_REF="$BASE_REF" HEAD_REF="$HEAD_REF" PR_AUTHOR="$PR_AUTHOR" PR_AUTHOR_TYPE="$PR_AUTHOR_TYPE" \
		RUNNER_TEMP="$TMP/runner" EXPECTED_SOURCE=dev VALIDATOR_MARKER="$TMP/untrusted-ran" \
		"$@" bash -euo pipefail "$TMP/check.sh" >"$TMP/out" 2>"$TMP/err")
	status=$?
	set -e
	[ ! -e "$TMP/untrusted-ran" ] || fail 'a PR-only policy file executed'
}

untrusted_copy() {
	local files=(commit-policy-source.sh commit-policy-exemptions.sh check-commit-trailers.sh)
	if [ -f "$TMP/repo/.github/scripts/commit-policy-team.sh" ]; then files+=(commit-policy-team.sh); fi
	for file in "${files[@]}"; do
		cat >"$TMP/repo/.github/scripts/$file" <<'SH'
printf ran >"$VALIDATOR_MARKER"
exit 90
SH
	done
	: >"$TMP/repo/.github/denied-identities.txt"
}

fixture
untrusted_copy
run
[ "$status" -eq 0 ] || { cat "$TMP/out" "$TMP/err"; fail 'a workflow sync could not bootstrap a legacy target'; }
grep -Fq "policy at $ACCEPTED" "$TMP/out" || fail 'policy came from a later source revision'

for route in 'master promote/snapshot dev' 'ca-release promote/ca-release-snapshot ca-canary' \
	'dev backmerge/master-snapshot master' 'ca-canary backmerge/ca-release-snapshot ca-release'; do
	read -r BASE_REF HEAD_REF expected_source <<<"$route"
	fixture
	run EXPECTED_SOURCE="$expected_source"
	[ "$status" -eq 0 ] || fail "an approved workflow route failed: $route"
done
BASE_REF=ca-canary
HEAD_REF=ca/sync-dev-snapshot

PR_AUTHOR=maintainer PR_AUTHOR_TYPE=User fixture
run PR_AUTHOR=maintainer PR_AUTHOR_TYPE=User
[ "$status" -eq 97 ] || fail 'a human PR used source policy'
PR_AUTHOR_TYPE=User fixture
run PR_AUTHOR_TYPE=User
[ "$status" -eq 97 ] || fail 'a non-bot account used source policy'
HEAD_REF=fix/ordinary fixture
run HEAD_REF=fix/ordinary
[ "$status" -eq 97 ] || fail 'an unsupported route used source policy'

for mutation in '.head.repo.full_name = "fork/repo"' '.base.repo.full_name = "other/repo"' \
	'.head.sha = "changed"' '.base.sha = "changed"' '.head.ref = "changed"' '.base.ref = "changed"' \
	'.user.login = "other[bot]"' '.user.type = "User"'; do
	fixture
	jq "$mutation" "$TMP/pull.json" >"$TMP/changed.json"
	mv "$TMP/changed.json" "$TMP/pull.json"
	run
	[ "$status" -eq 1 ] || fail "changed transfer metadata passed: $mutation"
done
fixture
jq '.protected = false' "$TMP/source.json" >"$TMP/changed.json"
mv "$TMP/changed.json" "$TMP/source.json"
run
[ "$status" -eq 1 ] || fail 'an unprotected source passed'
fixture
run FAIL_API=1
[ "$status" -eq 19 ] || fail 'a failed PR metadata read passed'
run FAIL_SOURCE=1
[ "$status" -eq 19 ] || fail 'a failed source metadata read passed'

git -C "$TMP/repo" reset -q --hard "$BASE"
git -C "$TMP/repo" commit -q --allow-empty --author='Target Maintainer <target@example.com>' -m target-update
TARGET="$(git -C "$TMP/repo" rev-parse HEAD)"
git -C "$TMP/repo" reset -q --hard "$ACCEPTED"
GIT_AUTHOR_NAME='Target Maintainer' GIT_AUTHOR_EMAIL='target@example.com' \
	git -C "$TMP/repo" merge -q --no-ff --no-edit "$TARGET"
HEAD_SHA="$(git -C "$TMP/repo" rev-parse HEAD)"
BASE_SHA="$TARGET"
git -C "$TMP/repo" push -q origin HEAD:refs/heads/snapshot
fixture
untrusted_copy
run
[ "$status" -eq 0 ] || fail 'a clean target merge lost trusted source policy'
BASE_SHA="$BASE"

for message in unknown-author unknown-coauthor forbidden-trailer; do
	git -C "$TMP/repo" reset -q --hard "$ACCEPTED"
	case "$message" in
		unknown-author) git -C "$TMP/repo" commit -q --allow-empty --author='Unknown <unknown@example.com>' -m "$message" ;;
		unknown-coauthor) git -C "$TMP/repo" commit -q --allow-empty -m $'coauthor\n\nCo-authored-by: Unknown <unknown@example.com>' ;;
		forbidden-trailer) git -C "$TMP/repo" commit -q --allow-empty -m $'session\n\nClaude-Session: example' ;;
	esac
	HEAD_SHA="$(git -C "$TMP/repo" rev-parse HEAD)"
	git -C "$TMP/repo" push -q --force origin HEAD:refs/heads/snapshot
	fixture
	untrusted_copy
	run
	[ "$status" -ne 0 ] || fail "source policy accepted $message"
	if [ "$message" = forbidden-trailer ]; then
		grep -q 'forbidden trailer:' "$TMP/err" || fail 'the trailer check did not run'
	else
		grep -q 'unratified identity:.*unknown@example.com' "$TMP/err" || fail 'the identity check did not run'
	fi
done

git -C "$TMP/repo" reset -q --hard "$ACCEPTED"
untrusted_copy
git -C "$TMP/repo" add .github
git -C "$TMP/repo" commit -q -m unmerged-policy-edits
HEAD_SHA="$(git -C "$TMP/repo" rev-parse HEAD)"
git -C "$TMP/repo" push -q --force origin HEAD:refs/heads/snapshot
fixture
run
[ "$status" -eq 0 ] || fail 'unmerged policy edits replaced accepted source policy'
grep -Fq "policy at $ACCEPTED" "$TMP/out" || fail 'policy came from an unmerged head revision'

git -C "$TMP/repo" reset -q --hard "$BASE"
printf '%s\n' 'sync-bot@example.com' >"$TMP/repo/.github/denied-identities.txt"
git -C "$TMP/repo" add .github
git -C "$TMP/repo" commit -q -m target-denial
BASE_SHA="$(git -C "$TMP/repo" rev-parse HEAD)"
git -C "$TMP/repo" push -q --force origin HEAD:refs/heads/target
HEAD_SHA="$ACCEPTED"
fixture
untrusted_copy
run
[ "$status" -ne 0 ] || fail 'source policy removed a target denial'
grep -q 'denied identity:.*sync-bot@example.com' "$TMP/err" || fail 'the target denial did not run'
BASE_SHA="$BASE"

git -C "$TMP/repo" reset -q --hard "$ACCEPTED"
printf '%s\n' 'sync-bot@example.com' >"$TMP/repo/.github/denied-identities.txt"
git -C "$TMP/repo" add .github
git -C "$TMP/repo" commit -q -m source-denial
SOURCE_TIP="$(git -C "$TMP/repo" rev-parse HEAD)"
HEAD_SHA="$SOURCE_TIP"
git -C "$TMP/repo" push -q --force origin HEAD:refs/heads/dev
fixture
untrusted_copy
run
[ "$status" -ne 0 ] || fail 'target policy removed a source denial'
grep -q 'denied identity:.*sync-bot@example.com' "$TMP/err" || fail 'the source denial did not run'

for file in .github/scripts/commit-policy-source.sh .github/scripts/commit-policy-exemptions.sh \
	.github/scripts/check-commit-trailers.sh .github/denied-identities.txt; do
	git -C "$TMP/repo" reset -q --hard "$ACCEPTED"
	git -C "$TMP/repo" rm -q "$file"
	git -C "$TMP/repo" commit -q -m missing-source-file
	SOURCE_TIP="$(git -C "$TMP/repo" rev-parse HEAD)"
	HEAD_SHA="$SOURCE_TIP"
	git -C "$TMP/repo" push -q --force origin HEAD:refs/heads/dev
	fixture
	untrusted_copy
	run
	[ "$status" -ne 0 ] || fail "a missing source file passed: $file"
done
git -C "$TMP/repo" reset -q --hard "$TIP"
SOURCE_TIP="$TIP"
HEAD_SHA="$TIP"
fixture
untrusted_copy
run
[ "$status" -eq 91 ] || fail 'a trusted source validator failure was ignored'

BASE_SHA="$ACCEPTED"
fixture
run
[ "$status" -eq 0 ] || fail 'an established target policy was replaced by source policy'

BASE_SHA="$BASE"
HEAD_REF=fix/ordinary
PR_AUTHOR=maintainer
PR_AUTHOR_TYPE=User
fixture
MERGE_BASE_SHA="$ACCEPTED" run
[ "$status" -eq 0 ] || { cat "$TMP/out" "$TMP/err"; fail 'stale PR base metadata replaced the accepted merge target policy'; }
grep -Fq "merge target $ACCEPTED" "$TMP/out" || fail 'the actual policy base was not reported'

merge_sha="$(git -C "$TMP/origin.git" rev-parse refs/pull/20/merge)"
git init -q "$TMP/shallow"
git -C "$TMP/shallow" remote add origin "$TMP/origin.git"
git -C "$TMP/shallow" fetch -q --depth=1 origin "$merge_sha"
git -C "$TMP/shallow" -c core.hooksPath=/dev/null checkout -q --detach FETCH_HEAD
[ "$(git -C "$TMP/shallow" rev-parse --is-shallow-repository)" = true ] || fail 'the shallow fixture has full history'
MERGE_BASE_SHA="$ACCEPTED" CHECKOUT_REPO="$TMP/shallow" run
[ "$status" -eq 0 ] || { cat "$TMP/out" "$TMP/err"; fail 'a shallow checkout did not resolve the accepted merge target'; }

MERGE_BASE_SHA="$ACCEPTED" MERGE_HEAD_SHA="$BASE" run
[ "$status" -ne 0 ] || fail 'a merge revision for another PR head passed'
run MERGE_SHA="$HEAD_SHA"
[ "$status" -ne 0 ] || fail 'a PR head commit was accepted as its proposed merge'

git -C "$TMP/repo" reset -q --hard "$ACCEPTED"
printf '%s\n' 'printf '\''%s\n'\'' "${TEAM_EXEMPT:-}"' >"$TMP/repo/.github/scripts/commit-policy-team.sh"
git -C "$TMP/repo" add .github
git -C "$TMP/repo" commit -q -m accepted-team-policy
TEAM_SOURCE="$(git -C "$TMP/repo" rev-parse HEAD)"
SOURCE_TIP="$TEAM_SOURCE"
git -C "$TMP/repo" push -q --force origin HEAD:refs/heads/dev
git -C "$TMP/repo" commit -q --allow-empty --author='Team Member <member@example.com>' -m member-contribution
HEAD_SHA="$(git -C "$TMP/repo" rev-parse HEAD)"
BASE_SHA="$BASE"
HEAD_REF=ca/sync-dev-team
PR_AUTHOR='f1r3node-promotion[bot]'
PR_AUTHOR_TYPE=Bot
fixture
untrusted_copy
run TEAM_EXEMPT="$HEAD_SHA"
[ "$status" -eq 0 ] || { cat "$TMP/out" "$TMP/err"; fail 'accepted source team policy did not exempt its member'; }

BASE_SHA="$TEAM_SOURCE"
HEAD_REF=fix/ordinary
PR_AUTHOR=maintainer
PR_AUTHOR_TYPE=User
fixture
run TEAM_EXEMPT="$HEAD_SHA"
[ "$status" -eq 0 ] || fail 'accepted target team policy did not exempt its member'

BASE_SHA="$ACCEPTED"
fixture
run TEAM_EXEMPT="$HEAD_SHA"
[ "$status" -ne 0 ] || fail 'a PR-only team helper ratified its author'
grep -q 'unratified identity:.*member@example.com' "$TMP/err" || fail 'the member author check did not run'

printf 'PASS: workflow transfers use verified shared source policy and preserve identity restrictions.\n'

#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/open-promotion-pr.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

git_quiet() {
	git -c user.name=Test -c user.email=test@example.com -c commit.gpgsign=false "$@" >/dev/null 2>&1
}

git_id() {
	git -c user.name=Test -c user.email=test@example.com -c commit.gpgsign=false "$@"
}

merge_into_seed() {
	local parent side merged
	parent="$(git -C "$TMP/seed" rev-parse HEAD)"
	side="$(git_id -C "$TMP/seed" commit-tree "$(git -C "$TMP/seed" rev-parse 'HEAD^{tree}')" -p "$parent" -m "side of $1")"
	merged="$(git_id -C "$TMP/seed" commit-tree "$(git -C "$TMP/seed" rev-parse 'HEAD^{tree}')" -p "$parent" -p "$side" -m "$1")"
	git_quiet -C "$TMP/seed" reset --hard "$merged"
}

git init --quiet --bare "$TMP/remote.git"
git_quiet clone --quiet "$TMP/remote.git" "$TMP/seed"
git_quiet -C "$TMP/seed" commit --allow-empty -m base
git_quiet -C "$TMP/seed" push --quiet origin HEAD:refs/heads/master HEAD:refs/heads/dev
git_quiet clone --quiet "$TMP/remote.git" "$TMP/work"

mkdir -p "$TMP/bin"
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$GH_LOG"
if [ "${GH_FAIL_PR_CREATE:-0}" = 1 ] && [ "$1 $2" = "pr create" ]; then exit 1; fi
if [ "$1 $2" = "pr view" ]; then
	[ -f "$GH_CLOSING/$3" ] || exit 0
	[ "$(cat "$GH_CLOSING/$3")" != fail ] || exit 1
	cat "$GH_CLOSING/$3"
fi
SH
chmod +x "$TMP/bin/gh"
mkdir -p "$TMP/closing"
export GH_CLOSING="$TMP/closing"

run_tool() {
	(cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" PROMOTE_REPOSITORY=example/repo \
		PROMOTE_TIMESTAMP=20261009-0945 bash "$TOOL")
}

: >"$TMP/gh.log"
out="$(run_tool)"
[[ "$out" == *"Nothing to promote"* ]] || fail "dev equal to master did not report nothing to promote: $out"
[[ ! -s "$TMP/gh.log" ]] || fail "dev equal to master still called gh: $(cat "$TMP/gh.log")"

git_quiet -C "$TMP/seed" commit --allow-empty -m feature
git_quiet -C "$TMP/seed" push --quiet origin HEAD:refs/heads/dev
dev_sha="$(git -C "$TMP/seed" rev-parse HEAD)"

: >"$TMP/gh.log"
out="$(run_tool)"
[[ "$out" == *"Created promote/20261009-0945 at ${dev_sha:0:9}"* ]] || fail "the promote branch was not reported: $out"
grep -Fxq "api repos/example/repo/git/refs -f ref=refs/heads/promote/20261009-0945 -f sha=$dev_sha" "$TMP/gh.log" ||
	fail "the promote branch was not created at the dev commit: $(cat "$TMP/gh.log")"
grep -q '^pr create --repo example/repo --base master --head promote/20261009-0945 ' "$TMP/gh.log" ||
	fail "the pull request was not opened from the promote branch to master: $(cat "$TMP/gh.log")"
grep -Fq '@Mergifyio queue' "$TMP/gh.log" || fail 'the promotion does not explain the explicit merge request'
if grep -q -- '--head dev' "$TMP/gh.log"; then
	fail "the pull request used dev as its head branch"
fi

grep -Fq 'created by `just promote`.' "$TMP/gh.log" ||
	fail "the just promote body changed: $(cat "$TMP/gh.log")"
if grep -q 'A different member' "$TMP/gh.log"; then
	fail "the just promote body asked for a different approver"
fi
if grep -qE '^pr view |Closes #|could not be read' "$TMP/gh.log"; then
	fail "a promotion without merged pull requests read or listed closing issues: $(cat "$TMP/gh.log")"
fi

: >"$TMP/gh.log"
(cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" PROMOTE_REPOSITORY=example/repo \
	PROMOTE_TIMESTAMP=20261009-094502 PROMOTE_TRIGGERED_BY=octo-cat bash "$TOOL") >/dev/null
grep -Fq 'created by the Promote and Back-merge workflow for @octo-cat.' "$TMP/gh.log" ||
	fail "the workflow body does not name the user who started it: $(cat "$TMP/gh.log")"
grep -Fxq '@octo-cat started this promotion. A different member of @F1R3FLY-io/ci-maintainers approves it.' "$TMP/gh.log" ||
	fail "the workflow body does not ask a different maintainer to approve: $(cat "$TMP/gh.log")"

# GitHub logins have at most 39 characters.
for login in 'x; rm' octo- -octo a--b "$(printf 'a%.0s' {1..40})"; do
	: >"$TMP/gh.log"
	# A missing remote makes git fetch fail. The login error must come first.
	if (cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" PROMOTE_REPOSITORY=example/repo \
		PROMOTE_REMOTE=no-such-remote PROMOTE_TIMESTAMP=20261009-094503 PROMOTE_TRIGGERED_BY="$login" \
		bash "$TOOL") >/dev/null 2>"$TMP/err"; then
		fail "the invalid login '$login' did not fail the command"
	fi
	grep -q 'PROMOTE_TRIGGERED_BY is not a GitHub login' "$TMP/err" ||
		fail "the invalid login '$login' was not rejected before git fetch: $(cat "$TMP/err")"
	[[ ! -s "$TMP/gh.log" ]] || fail "the invalid login '$login' still called gh: $(cat "$TMP/gh.log")"
done

: >"$TMP/gh.log"
if (cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" GH_FAIL_PR_CREATE=1 PROMOTE_REPOSITORY=example/repo \
	PROMOTE_TIMESTAMP=20261009-094501 bash "$TOOL") >/dev/null 2>"$TMP/err"; then
	fail "a failed pull request creation did not fail the command"
fi
grep -Fxq "api -X DELETE repos/example/repo/git/refs/heads/promote/20261009-094501" "$TMP/gh.log" ||
	fail "a failed pull request creation did not delete the promote branch: $(cat "$TMP/gh.log")"

merge_into_seed "Merge pull request #11 from example/topic-11"
merge_into_seed "Merge branch 'dev' into topic-12"
merge_into_seed "Merge pull request #12 from example/topic-12"
merge_into_seed "Merge pull request #13 from example/topic-13"
merge_into_seed "Merge pull request #14 from example/topic-14"
merge_into_seed "Merge pull request #11 from example/topic-11-again"
merge_into_seed "Merge pull request #15 from example/topic-15"
git_quiet -C "$TMP/seed" push --quiet origin HEAD:refs/heads/dev
cat >"$TMP/closing/15" <<'BODY'
````markdown
```
Closes #901
```
Closes #902
````
<!-- Closes #903 -->
<!--
Fixes #904
-->
Use `Closes #905` to link an issue, or ``Fixes #906``.
Closes #0
Resolves #107
BODY
printf 'Closes #101 and fixes: #102. Refs #105.\n' >"$TMP/closing/11"
cat >"$TMP/closing/12" <<'BODY'
Resolves #102
See #200, closes other/repo#300, and fixes https://github.com/other/repo/issues/301.
```
Closes #999
```
Fixed https://github.com/example/repo/issues/103
BODY
printf 'fail\n' >"$TMP/closing/14"

: >"$TMP/gh.log"
(cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" PROMOTE_REPOSITORY=example/repo \
	PROMOTE_TIMESTAMP=20261009-094504 bash "$TOOL") >/dev/null 2>"$TMP/err" ||
	fail "an unreadable pull request stopped the promotion: $(cat "$TMP/err")"
for pr in 11 12 13 14 15; do
	[ "$(grep -c "^pr view $pr --repo example/repo --json body " "$TMP/gh.log")" = 1 ] ||
		fail "PR #$pr was not read exactly once: $(cat "$TMP/gh.log")"
done
[ "$(grep -c '^pr view ' "$TMP/gh.log")" = 5 ] ||
	fail "a merge that is not a pull request was read: $(cat "$TMP/gh.log")"
for issue in 101 102 103 107; do
	[ "$(grep -cFx "Closes #$issue" "$TMP/gh.log")" = 1 ] ||
		fail "issue #$issue is not listed exactly once: $(cat "$TMP/gh.log")"
done
for issue in 0 901 902 903 904 905 906; do
	if grep -qFx "Closes #$issue" "$TMP/gh.log"; then
		fail "issue #$issue from a code block, a comment, inline code, or a zero number is listed: $(cat "$TMP/gh.log")"
	fi
done
[ "$(grep -c '^Closes #' "$TMP/gh.log")" = 4 ] || fail "the body lists issues that no pull request closes: $(cat "$TMP/gh.log")"
grep -Fq 'The descriptions of the promoted pull requests close these issues.' "$TMP/gh.log" ||
	fail "the body does not explain the closing issues: $(cat "$TMP/gh.log")"
grep -Fq 'Close their issues by hand after the merge: PR #14.' "$TMP/gh.log" ||
	fail "the body does not name the unreadable pull request: $(cat "$TMP/gh.log")"
grep -Fq 'the closing issues of PR #14 could not be read' "$TMP/err" ||
	fail "the unreadable pull request was not reported: $(cat "$TMP/err")"
grep -q '^pr create --repo example/repo --base master --head promote/20261009-094504 ' "$TMP/gh.log" ||
	fail "the promotion with closing issues was not opened: $(cat "$TMP/gh.log")"

run_ca_tool() {
	(cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" PROMOTE_REPOSITORY=example/repo \
		PROMOTE_SOURCE=ca-canary PROMOTE_TARGET=ca-release PROMOTE_TIMESTAMP="$1" bash "$TOOL")
}

git_quiet -C "$TMP/seed" push --quiet origin HEAD:refs/heads/ca-canary HEAD:refs/heads/ca-release

: >"$TMP/gh.log"
out="$(run_ca_tool 20261020-1200)"
[[ "$out" == *"Nothing to promote: origin/ca-release already contains origin/ca-canary"* ]] ||
	fail "ca-canary equal to ca-release did not report nothing to promote: $out"
[[ ! -s "$TMP/gh.log" ]] || fail "ca-canary equal to ca-release still called gh: $(cat "$TMP/gh.log")"

git_quiet -C "$TMP/seed" commit --allow-empty -m ca-item
git_quiet -C "$TMP/seed" push --quiet origin HEAD:refs/heads/ca-canary
canary_sha="$(git -C "$TMP/seed" rev-parse HEAD)"

: >"$TMP/gh.log"
out="$(run_ca_tool 20261020-1201)"
[[ "$out" == *"Created promote/ca-release-20261020-1201 at ${canary_sha:0:9} (origin/ca-canary, 1 commits not on ca-release)"* ]] ||
	fail "the ca-release promote branch was not reported: $out"
grep -Fxq "api repos/example/repo/git/refs -f ref=refs/heads/promote/ca-release-20261020-1201 -f sha=$canary_sha" "$TMP/gh.log" ||
	fail "the ca-release promote branch was not created at the ca-canary commit: $(cat "$TMP/gh.log")"
grep -q '^pr create --repo example/repo --base ca-release --head promote/ca-release-20261020-1201 --title Promote ca-canary to ca-release (20261020-1201) ' "$TMP/gh.log" ||
	fail "the pull request was not opened from the promote branch to ca-release: $(cat "$TMP/gh.log")"
grep -Fq 'created by `just ca-promote`.' "$TMP/gh.log" ||
	fail "the just ca-promote body does not name just ca-promote: $(cat "$TMP/gh.log")"
grep -Fq '@Mergifyio queue' "$TMP/gh.log" || fail 'the release promotion does not explain the explicit merge request'
if grep -q -- '--head ca-canary' "$TMP/gh.log"; then
	fail "the pull request used ca-canary as its head branch"
fi

merge_into_seed "Merge pull request #16 from example/topic-16"
git_quiet -C "$TMP/seed" push --quiet origin HEAD:refs/heads/ca-canary
printf 'Closes #106\n' >"$TMP/closing/16"
: >"$TMP/gh.log"
run_ca_tool 20261020-1203 >/dev/null
if grep -qE '^pr view |Closes #' "$TMP/gh.log"; then
	fail "the ca-release promotion read or listed closing issues: $(cat "$TMP/gh.log")"
fi

for pair in master:dev dev:ca-release ca-canary:master ca-release:ca-canary; do
	: >"$TMP/gh.log"
	if (cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" PROMOTE_REPOSITORY=example/repo \
		PROMOTE_REMOTE=no-such-remote PROMOTE_SOURCE="${pair%%:*}" PROMOTE_TARGET="${pair#*:}" \
		PROMOTE_TIMESTAMP=20261020-1202 bash "$TOOL") >/dev/null 2>"$TMP/err"; then
		fail "the promotion pair $pair did not fail the command"
	fi
	grep -q "no promotion from '${pair%%:*}' to '${pair#*:}'" "$TMP/err" ||
		fail "the promotion pair $pair was not refused before git fetch: $(cat "$TMP/err")"
	[[ ! -s "$TMP/gh.log" ]] || fail "the promotion pair $pair still called gh: $(cat "$TMP/gh.log")"
done

printf 'PASS: just promote opens the master pull request from a promote/* branch at the dev commit, just ca-promote opens the ca-release pull request from a promote/ca-release-* branch at the ca-canary commit, other branch pairs are refused, names the user who started the workflow, deletes the branch when the pull request fails, lists each issue that the promoted pull requests close once in the master promotion, ignores closing keywords in code blocks, HTML comments, and inline code, names a pull request whose issues it cannot read, and lists no issues in the ca-release promotion.\n'

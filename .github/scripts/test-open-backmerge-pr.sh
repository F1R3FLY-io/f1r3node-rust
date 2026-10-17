#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/open-backmerge-pr.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

git_quiet() {
	git -c user.name=Test -c user.email=test@example.com -c commit.gpgsign=false "$@" >/dev/null 2>&1
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
if [ "$1 $2" = "pr list" ]; then printf '%s\n' "${GH_OPEN_BACKMERGE:-}"; exit "${GH_FAIL_PR_LIST:-0}"; fi
if [ "${GH_FAIL_REF:-0}" = 1 ] && [ "$1" = api ] && [ "$2" != -X ]; then exit 1; fi
if [ "${GH_FAIL_PR_CREATE:-0}" = 1 ] && [ "$1 $2" = "pr create" ]; then exit 1; fi
SH
chmod +x "$TMP/bin/gh"

run_tool() {
	(cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" BACKMERGE_REPOSITORY=example/repo \
		BACKMERGE_TIMESTAMP="$1" bash "$TOOL")
}

: >"$TMP/gh.log"
out="$(run_tool 20261016-1600)"
[[ "$out" == *"Nothing to back-merge"* ]] || fail "master equal to dev did not report nothing to back-merge: $out"
[[ ! -s "$TMP/gh.log" ]] || fail "master equal to dev still called gh: $(cat "$TMP/gh.log")"

git_quiet -C "$TMP/seed" commit --allow-empty -m feature
git_quiet -C "$TMP/seed" push --quiet origin HEAD:refs/heads/dev

: >"$TMP/gh.log"
out="$(run_tool 20261016-1601)"
[[ "$out" == *"Nothing to back-merge"* ]] || fail "dev ahead of master did not report nothing to back-merge: $out"
[[ ! -s "$TMP/gh.log" ]] || fail "dev ahead of master still called gh: $(cat "$TMP/gh.log")"

git_quiet -C "$TMP/seed" checkout --quiet -b hotfix origin/master
git_quiet -C "$TMP/seed" commit --allow-empty -m hotfix
git_quiet -C "$TMP/seed" push --quiet origin HEAD:refs/heads/master
master_sha="$(git -C "$TMP/seed" rev-parse HEAD)"
branch="backmerge/master-${master_sha:0:12}"

: >"$TMP/gh.log"
out="$(run_tool 20261016-1602)"
[[ "$out" == *"Created $branch at ${master_sha:0:9}"* ]] || fail "the backmerge branch was not reported: $out"
grep -Fxq "api repos/example/repo/git/refs -f ref=refs/heads/$branch -f sha=$master_sha" "$TMP/gh.log" ||
	fail "the backmerge branch was not created at the master commit: $(cat "$TMP/gh.log")"
grep -q "^pr create --repo example/repo --base dev --head $branch --title Back-merge master into dev (20261016-1602) " "$TMP/gh.log" ||
	fail "the pull request was not opened from the backmerge branch to dev: $(cat "$TMP/gh.log")"
if grep -q -- '--head master' "$TMP/gh.log"; then
	fail "the pull request used master as its head branch"
fi
grep -Fq 'created by `just backmerge`.' "$TMP/gh.log" ||
	fail "the just backmerge body changed: $(cat "$TMP/gh.log")"
if grep -q 'A different member' "$TMP/gh.log"; then
	fail "the just backmerge body asked for a different approver"
fi

: >"$TMP/gh.log"
(cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" BACKMERGE_REPOSITORY=example/repo \
	BACKMERGE_TIMESTAMP=20261016-1607 BACKMERGE_TRIGGERED_BY=octo-cat bash "$TOOL") >/dev/null
grep -Fq 'created by the Promote and Back-merge workflow for @octo-cat.' "$TMP/gh.log" ||
	fail "the workflow body does not name the user who started it: $(cat "$TMP/gh.log")"
grep -Fxq '@octo-cat started this back-merge. A different member of @F1R3FLY-io/ci-maintainers approves it.' "$TMP/gh.log" ||
	fail "the workflow body does not ask a different maintainer to approve: $(cat "$TMP/gh.log")"

for login in 'x; rm' octo- -octo a--b; do
	: >"$TMP/gh.log"
	# A missing remote makes git fetch fail. The login error must come first.
	if (cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" BACKMERGE_REPOSITORY=example/repo \
		BACKMERGE_REMOTE=no-such-remote BACKMERGE_TIMESTAMP=20261016-1608 BACKMERGE_TRIGGERED_BY="$login" \
		bash "$TOOL") >/dev/null 2>"$TMP/err"; then
		fail "the invalid login '$login' did not fail the command"
	fi
	grep -q 'BACKMERGE_TRIGGERED_BY is not a GitHub login' "$TMP/err" ||
		fail "the invalid login '$login' was not rejected before git fetch: $(cat "$TMP/err")"
	[[ ! -s "$TMP/gh.log" ]] || fail "the invalid login '$login' still called gh: $(cat "$TMP/gh.log")"
done

: >"$TMP/gh.log"
if (cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" GH_FAIL_PR_CREATE=1 BACKMERGE_REPOSITORY=example/repo \
	BACKMERGE_TIMESTAMP=20261016-1603 bash "$TOOL") >/dev/null 2>"$TMP/err"; then
	fail "a failed pull request creation did not fail the command"
fi
grep -Fxq "api -X DELETE repos/example/repo/git/refs/heads/$branch" "$TMP/gh.log" ||
	fail "a failed pull request creation did not delete the backmerge branch: $(cat "$TMP/gh.log")"

grep -q '^pr list --repo example/repo --base dev --state open ' "$TMP/gh.log" ||
	fail "the open back-merge pull requests were not listed: $(cat "$TMP/gh.log")"

: >"$TMP/gh.log"
out="$(cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" GH_OPEN_BACKMERGE='#747' BACKMERGE_REPOSITORY=example/repo \
	BACKMERGE_TIMESTAMP=20261016-1604 bash "$TOOL")"
[[ "$out" == *"already open: #747"* ]] || fail "an open back-merge pull request was not reported: $out"
if grep -qE '^(api|pr create)' "$TMP/gh.log"; then
	fail "an open back-merge pull request still created a branch or a pull request: $(cat "$TMP/gh.log")"
fi

: >"$TMP/gh.log"
if (cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" GH_FAIL_PR_LIST=1 BACKMERGE_REPOSITORY=example/repo \
	BACKMERGE_TIMESTAMP=20261016-1605 bash "$TOOL") >/dev/null 2>"$TMP/err"; then
	fail "a failed pull request list did not fail the command"
fi
grep -q 'could not be listed' "$TMP/err" || fail "a failed pull request list was not reported: $(cat "$TMP/err")"
if grep -qE '^(api|pr create)' "$TMP/gh.log"; then
	fail "a failed pull request list still created a branch or a pull request: $(cat "$TMP/gh.log")"
fi

: >"$TMP/gh.log"
if (cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" GH_FAIL_REF=1 BACKMERGE_REPOSITORY=example/repo \
	BACKMERGE_TIMESTAMP=20261016-1606 bash "$TOOL") >/dev/null 2>"$TMP/err"; then
	fail "a failed branch creation did not fail the command"
fi
grep -q "$branch was not created" "$TMP/err" || fail "a failed branch creation was not reported: $(cat "$TMP/err")"
if grep -q '^pr create' "$TMP/gh.log"; then
	fail "a failed branch creation still opened a pull request: $(cat "$TMP/gh.log")"
fi

printf 'PASS: just backmerge opens the dev pull request from backmerge/master-<sha> at the master commit, skips when dev contains master, refuses a second open back-merge, reports a failed branch creation for the same master commit, names the user who started the workflow, and deletes the branch when the pull request fails.\n'

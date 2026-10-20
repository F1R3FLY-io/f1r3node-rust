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

# GitHub logins have at most 39 characters.
for login in 'x; rm' octo- -octo a--b "$(printf 'a%.0s' {1..40})"; do
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

run_ca_tool() {
	(cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" BACKMERGE_REPOSITORY=example/repo \
		BACKMERGE_SOURCE=ca-release BACKMERGE_TARGET=ca-canary BACKMERGE_TIMESTAMP="$1" bash "$TOOL")
}

git_quiet -C "$TMP/seed" push --quiet origin HEAD:refs/heads/ca-canary HEAD:refs/heads/ca-release

: >"$TMP/gh.log"
out="$(run_ca_tool 20261020-1300)"
[[ "$out" == *"Nothing to back-merge: origin/ca-canary already contains origin/ca-release"* ]] ||
	fail "ca-release equal to ca-canary did not report nothing to back-merge: $out"
[[ ! -s "$TMP/gh.log" ]] || fail "ca-release equal to ca-canary still called gh: $(cat "$TMP/gh.log")"

git_quiet -C "$TMP/seed" commit --allow-empty -m promotion
git_quiet -C "$TMP/seed" push --quiet origin HEAD:refs/heads/ca-release
release_sha="$(git -C "$TMP/seed" rev-parse HEAD)"
ca_branch="backmerge/ca-release-${release_sha:0:12}"

: >"$TMP/gh.log"
out="$(run_ca_tool 20261020-1301)"
[[ "$out" == *"Created $ca_branch at ${release_sha:0:9} (origin/ca-release, 1 commits not on ca-canary)"* ]] ||
	fail "the ca-canary backmerge branch was not reported: $out"
grep -q '^pr list --repo example/repo --base ca-canary --state open ' "$TMP/gh.log" ||
	fail "the open back-merge pull requests to ca-canary were not listed: $(cat "$TMP/gh.log")"
grep -Fxq "api repos/example/repo/git/refs -f ref=refs/heads/$ca_branch -f sha=$release_sha" "$TMP/gh.log" ||
	fail "the ca-canary backmerge branch was not created at the ca-release commit: $(cat "$TMP/gh.log")"
grep -q "^pr create --repo example/repo --base ca-canary --head $ca_branch --title Back-merge ca-release into ca-canary (20261020-1301) " "$TMP/gh.log" ||
	fail "the pull request was not opened from the backmerge branch to ca-canary: $(cat "$TMP/gh.log")"
grep -Fq 'created by `just ca-backmerge`.' "$TMP/gh.log" ||
	fail "the just ca-backmerge body does not name just ca-backmerge: $(cat "$TMP/gh.log")"
grep -Fq 'so that `ca-canary` keeps the `ca-release` history.' "$TMP/gh.log" ||
	fail "the just ca-backmerge body does not ask for a merge commit into ca-canary: $(cat "$TMP/gh.log")"
if grep -q -- '--head ca-release' "$TMP/gh.log"; then
	fail "the pull request used ca-release as its head branch"
fi

: >"$TMP/gh.log"
out="$(cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" GH_OPEN_BACKMERGE='#792' BACKMERGE_REPOSITORY=example/repo \
	BACKMERGE_SOURCE=ca-release BACKMERGE_TARGET=ca-canary BACKMERGE_TIMESTAMP=20261020-1302 bash "$TOOL")"
[[ "$out" == *"already open: #792. Merge or close it, then run just ca-backmerge again."* ]] ||
	fail "an open ca-canary back-merge pull request was not reported: $out"

for pair in dev:master master:ca-canary ca-release:dev ca-canary:ca-release dev:ca-release; do
	: >"$TMP/gh.log"
	if (cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" BACKMERGE_REPOSITORY=example/repo \
		BACKMERGE_REMOTE=no-such-remote BACKMERGE_SOURCE="${pair%%:*}" BACKMERGE_TARGET="${pair#*:}" \
		BACKMERGE_TIMESTAMP=20261020-1303 bash "$TOOL") >/dev/null 2>"$TMP/err"; then
		fail "the back-merge pair $pair did not fail the command"
	fi
	grep -q "no back-merge from '${pair%%:*}' to '${pair#*:}'" "$TMP/err" ||
		fail "the back-merge pair $pair was not refused before git fetch: $(cat "$TMP/err")"
	[[ ! -s "$TMP/gh.log" ]] || fail "the back-merge pair $pair still called gh: $(cat "$TMP/gh.log")"
done

run_sync_tool() {
	(cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" BACKMERGE_REPOSITORY=example/repo \
		BACKMERGE_SOURCE=dev BACKMERGE_TARGET=ca-canary BACKMERGE_TIMESTAMP="$1" bash "$TOOL")
}

git_quiet -C "$TMP/seed" fetch --quiet origin
git_quiet -C "$TMP/seed" push --quiet origin origin/ca-canary:refs/heads/dev --force

: >"$TMP/gh.log"
out="$(run_sync_tool 20261020-1400)"
[[ "$out" == *"Nothing to sync: origin/ca-canary already contains origin/dev"* ]] ||
	fail "dev contained in ca-canary did not report nothing to sync: $out"
[[ ! -s "$TMP/gh.log" ]] || fail "dev contained in ca-canary still called gh: $(cat "$TMP/gh.log")"

git_quiet -C "$TMP/seed" checkout --quiet -B dev-work origin/dev
git_quiet -C "$TMP/seed" commit --allow-empty -m dev-change
git_quiet -C "$TMP/seed" push --quiet origin HEAD:refs/heads/dev
sync_sha="$(git -C "$TMP/seed" rev-parse HEAD)"
sync_branch="ca/sync-dev-${sync_sha:0:12}"

: >"$TMP/gh.log"
out="$(run_sync_tool 20261020-1401)"
[[ "$out" == *"Created $sync_branch at ${sync_sha:0:9} (origin/dev, 1 commits not on ca-canary)"* ]] ||
	fail "the sync branch was not reported: $out"
grep -q '^pr list --repo example/repo --base ca-canary --state open ' "$TMP/gh.log" ||
	fail "the open sync pull requests to ca-canary were not listed: $(cat "$TMP/gh.log")"
grep -q 'startswith("ca/sync-")' "$TMP/gh.log" ||
	fail "the open pull request check does not filter on ca/sync-: $(cat "$TMP/gh.log")"
grep -q "^pr create --repo example/repo --base ca-canary --head $sync_branch --label ci-heavy --title Sync dev into ca-canary (20261020-1401) " "$TMP/gh.log" ||
	fail "the sync pull request was not opened from the sync branch to ca-canary with ci-heavy: $(cat "$TMP/gh.log")"
grep -Fq 'created by `just ca-sync`.' "$TMP/gh.log" ||
	fail "the sync body does not name just ca-sync: $(cat "$TMP/gh.log")"
grep -Fq 'Add the `ca-soak` label to start the 22-hour soak.' "$TMP/gh.log" ||
	fail "the sync body does not ask for the ca-soak label: $(cat "$TMP/gh.log")"
if grep -q -- '--label ca-soak' "$TMP/gh.log"; then
	fail "the sync pull request started a soak: $(cat "$TMP/gh.log")"
fi

: >"$TMP/gh.log"
run_ca_tool 20261020-1402 >/dev/null
if grep -q -- '--label' "$TMP/gh.log"; then
	fail "the ca-canary back-merge got a label: $(cat "$TMP/gh.log")"
fi

: >"$TMP/gh.log"
out="$(cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" GH_OPEN_BACKMERGE='#793' BACKMERGE_REPOSITORY=example/repo \
	BACKMERGE_SOURCE=dev BACKMERGE_TARGET=ca-canary BACKMERGE_TIMESTAMP=20261020-1403 bash "$TOOL")"
[[ "$out" == *"A sync pull request is already open: #793. Merge or close it, then run just ca-sync again."* ]] ||
	fail "an open sync pull request was not reported: $out"

: >"$TMP/gh.log"
(cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" BACKMERGE_REPOSITORY=example/repo \
	BACKMERGE_SOURCE=ca-release BACKMERGE_TARGET=ca-canary BACKMERGE_TIMESTAMP=20261020-1404 BACKMERGE_SCHEDULED=1 \
	bash "$TOOL") >/dev/null
grep -Fq 'created by the scheduled Release Train Daily Back-merge workflow.' "$TMP/gh.log" ||
	fail "the scheduled back-merge body does not name the daily back-merge workflow: $(cat "$TMP/gh.log")"
grep -Fxq 'Mergify merges this back-merge when its CI passes. It needs no review.' "$TMP/gh.log" ||
	fail "the scheduled back-merge body does not say that Mergify merges it: $(cat "$TMP/gh.log")"

for scheduled in master:dev dev:ca-canary; do
	: >"$TMP/gh.log"
	if (cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" BACKMERGE_REPOSITORY=example/repo \
		BACKMERGE_REMOTE=no-such-remote BACKMERGE_SOURCE="${scheduled%%:*}" BACKMERGE_TARGET="${scheduled#*:}" \
		BACKMERGE_TIMESTAMP=20261020-1405 BACKMERGE_SCHEDULED=1 bash "$TOOL") >/dev/null 2>"$TMP/err"; then
		fail "a scheduled $scheduled run did not fail the command"
	fi
	grep -q 'BACKMERGE_SCHEDULED applies only to the ca-release to ca-canary back-merge' "$TMP/err" ||
		fail "a scheduled $scheduled run was not refused before git fetch: $(cat "$TMP/err")"
	[[ ! -s "$TMP/gh.log" ]] || fail "a scheduled $scheduled run still called gh: $(cat "$TMP/gh.log")"
done

printf 'PASS: just backmerge opens the dev pull request from backmerge/master-<sha> at the master commit, just ca-backmerge opens the ca-canary pull request from backmerge/ca-release-<sha> at the ca-release commit, just ca-sync opens the ca-canary pull request from ca/sync-dev-<sha> at the dev commit with ci-heavy and without ca-soak, marks the scheduled ca-release back-merge for Mergify and refuses any other scheduled pair, other branch pairs are refused, skips when the target contains the source, refuses a second open pull request of the same kind, reports a failed branch creation for the same master commit, names the user who started the workflow, and deletes the branch when the pull request fails.\n'

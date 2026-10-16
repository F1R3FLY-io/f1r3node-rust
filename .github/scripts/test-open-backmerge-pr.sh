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

: >"$TMP/gh.log"
out="$(run_tool 20261016-1602)"
[[ "$out" == *"Created backmerge/20261016-1602 at ${master_sha:0:9}"* ]] || fail "the backmerge branch was not reported: $out"
grep -Fxq "api repos/example/repo/git/refs -f ref=refs/heads/backmerge/20261016-1602 -f sha=$master_sha" "$TMP/gh.log" ||
	fail "the backmerge branch was not created at the master commit: $(cat "$TMP/gh.log")"
grep -q '^pr create --repo example/repo --base dev --head backmerge/20261016-1602 ' "$TMP/gh.log" ||
	fail "the pull request was not opened from the backmerge branch to dev: $(cat "$TMP/gh.log")"
if grep -q -- '--head master' "$TMP/gh.log"; then
	fail "the pull request used master as its head branch"
fi
grep -Fq 'created by `just backmerge`.' "$TMP/gh.log" ||
	fail "the just backmerge body changed: $(cat "$TMP/gh.log")"

: >"$TMP/gh.log"
if (cd "$TMP/work" && PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" GH_FAIL_PR_CREATE=1 BACKMERGE_REPOSITORY=example/repo \
	BACKMERGE_TIMESTAMP=20261016-1603 bash "$TOOL") >/dev/null 2>"$TMP/err"; then
	fail "a failed pull request creation did not fail the command"
fi
grep -Fxq "api -X DELETE repos/example/repo/git/refs/heads/backmerge/20261016-1603" "$TMP/gh.log" ||
	fail "a failed pull request creation did not delete the backmerge branch: $(cat "$TMP/gh.log")"

printf 'PASS: just backmerge opens the dev pull request from a backmerge/* branch at the master commit, skips when dev contains master, and deletes the branch when the pull request fails.\n'

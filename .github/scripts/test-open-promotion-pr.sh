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

git init --quiet --bare "$TMP/remote.git"
git_quiet clone --quiet "$TMP/remote.git" "$TMP/seed"
git_quiet -C "$TMP/seed" commit --allow-empty -m base
git_quiet -C "$TMP/seed" push --quiet origin HEAD:refs/heads/master HEAD:refs/heads/dev
git_quiet clone --quiet "$TMP/remote.git" "$TMP/work"

mkdir -p "$TMP/bin"
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$GH_LOG"
SH
chmod +x "$TMP/bin/gh"

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
if grep -q -- '--head dev' "$TMP/gh.log"; then
	fail "the pull request used dev as its head branch"
fi

printf 'PASS: just promote opens the master pull request from a promote/* branch at the dev commit.\n'

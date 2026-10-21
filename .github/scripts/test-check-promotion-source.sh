#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/check-promotion-source.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

mkdir -p "$TMP/bin"
cat >"$TMP/bin/gh" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$GH_LOG"
if [ "${GH_FAIL_CLOSE:-0}" = 1 ]; then exit 1; fi
SH
chmod +x "$TMP/bin/gh"

# run EVENT BASE HEAD HEAD_REPO [PR_NUMBER]: sets status, out, and gh.log.
run() {
	: >"$TMP/gh.log"
	set +e
	out="$(PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" EVENT_NAME="$1" BASE_REF="$2" HEAD_REF="$3" \
		HEAD_REPO="$4" REPOSITORY=example/repo PR_NUMBER="${5:-}" bash "$TOOL" 2>&1)"
	status=$?
	set -e
}

expect_reject() {
	run "$@"
	[ "$status" -ne 0 ] || fail "accepted event=$1 base=$2 head=$3 repo=$4: $out"
	[[ "$out" == *"::error::"* ]] || fail "no error annotation for base=$2 head=$3: $out"
}

expect_accept() {
	run "$@"
	[ "$status" -eq 0 ] || fail "rejected event=$1 base=$2 head=$3 repo=$4: $out"
	if grep -q '^pr close' "$TMP/gh.log"; then fail "an accepted pull request was closed: $(cat "$TMP/gh.log")"; fi
}

expect_reject pull_request master dev example/repo
[[ "$out" == *"just promote"* ]] || fail "a promotion with head dev does not name just promote: $out"
expect_reject pull_request dev master example/repo
[[ "$out" == *"just backmerge"* ]] || fail "a back-merge with head master does not name just backmerge: $out"
expect_reject pull_request master master example/repo
expect_reject pull_request dev dev example/repo
expect_reject pull_request feature dev example/repo
expect_reject pull_request feature master example/repo

expect_accept pull_request dev master other/fork 12
expect_accept pull_request master dev other/fork 12
expect_accept pull_request dev feature example/repo 12
expect_accept pull_request master promote/20261016-1600 example/repo 12
expect_accept pull_request dev backmerge/20261016-1600 example/repo 12
expect_accept pull_request master hotfix/x example/repo 12
expect_accept push "" "" ""
expect_accept merge_group "" "" ""
expect_reject pull_request_target master dev example/repo
expect_accept pull_request_target dev master other/fork 12
expect_accept pull_request_target dev feature example/repo 12

expect_reject pull_request_target dev master example/repo 747
grep -q '^pr close 747 --repo example/repo --comment Closed by the protected head guard\. Do not open a back-merge with head master\..* Reason: ' "$TMP/gh.log" ||
	fail "a rejected pull request was not closed with the remedy and the reason: $(cat "$TMP/gh.log")"
[[ "$out" == *"Closed pull request #747."* ]] || fail "the close was not reported: $out"

expect_reject pull_request dev master example/repo
[[ ! -s "$TMP/gh.log" ]] || fail "a rejection without PR_NUMBER called gh: $(cat "$TMP/gh.log")"

: >"$TMP/gh.log"
set +e
out="$(PATH="$TMP/bin:$PATH" GH_LOG="$TMP/gh.log" GH_FAIL_CLOSE=1 EVENT_NAME=pull_request BASE_REF=dev HEAD_REF=master \
	HEAD_REPO=example/repo REPOSITORY=example/repo PR_NUMBER=748 bash "$TOOL" 2>&1)"
status=$?
set -e
[ "$status" -ne 0 ] || fail "a failed close passed the check: $out"
[[ "$out" == *"::warning::Pull request #748 was not closed"* ]] || fail "a failed close was not reported: $out"

expect_reject pull_request dev master example/repo '1; rm'
[[ ! -s "$TMP/gh.log" ]] || fail "a non-numeric PR_NUMBER called gh: $(cat "$TMP/gh.log")"

expect_reject pull_request ca-release ca-canary example/repo
[[ "$out" == *"just ca-promote"* ]] || fail "a promotion with head ca-canary does not name just ca-promote: $out"
expect_reject pull_request ca-canary ca-release example/repo
[[ "$out" == *"just ca-backmerge"* ]] || fail "a back-merge with head ca-release does not name just ca-backmerge: $out"
expect_reject pull_request dev ca-canary example/repo
[[ "$out" == *"temporary branch at the ca-canary commit"* ]] || fail "head ca-canary to dev does not name a temporary branch: $out"
expect_reject pull_request ca-canary ca-canary example/repo
expect_reject pull_request master ca-release example/repo
expect_reject pull_request_target ca-release ca-canary example/repo

expect_accept pull_request ca-canary ca/a1a-coalescing example/repo 12
expect_accept pull_request ca-release promote/ca-release-20261020-1200 example/repo 12
expect_accept pull_request ca-canary backmerge/ca-release-0123456789ab example/repo 12
expect_accept pull_request dev backmerge/ca-release-0123456789ab example/repo 12
expect_accept pull_request ca-release ca-canary other/fork 12
expect_accept pull_request_target ca-canary ca-release other/fork 12

expect_reject pull_request_target ca-canary ca-release example/repo 791
grep -q '^pr close 791 --repo example/repo --comment Closed by the protected head guard\. Do not open a back-merge with head ca-release\..* Reason: ' "$TMP/gh.log" ||
	fail "a rejected ca-release pull request was not closed with the remedy and the reason: $(cat "$TMP/gh.log")"

printf 'PASS: the protected-head check rejects a pull request with head master, dev, ca-canary, or ca-release from this repository under pull_request and pull_request_target, closes it with a reason when PR_NUMBER is set, and accepts fork, temporary, and non-pull-request heads. The separate daily provenance check validates back-merge content.\n'

#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/require-repository-commit.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

g() {
	git -C "$TMP/origin" -c user.name=Test -c user.email=test@example.com \
		-c commit.gpgsign=false -c tag.gpgsign=false -c core.hooksPath=/dev/null "$@"
}

commit() {
	g commit --quiet --allow-empty -m "$1"
	g rev-parse HEAD
}

git init --quiet -b main "$TMP/origin"
g config uploadpack.allowFilter true
g config uploadpack.allowAnySHA1InWant true
base="$(commit base)"
head="$(commit head)"
g switch --quiet -c feature
feature="$(commit feature)"
g switch --quiet --detach "$base"
tagged="$(commit tagged)"
g tag -a v1 -m v1
pull="$(commit pull)"
g update-ref refs/pull/1/head "$pull"
dangling="$(commit dangling)"
g switch --quiet main
URL="file://$TMP/origin"

run() {
	set +e
	out="$(bash "$TOOL" "$@" 2>&1)"
	status=$?
	set -e
}

expect_accept() {
	run "$URL" "$1"
	[ "$status" -eq 0 ] || fail "rejected $2 ($1): $out"
	[[ "$out" == "Benchmark target accepted: "* ]] || fail "no acceptance line for $2: $out"
}

expect_reject() {
	run "$URL" "$1"
	[ "$status" -ne 0 ] || fail "accepted $2 ($1): $out"
	[[ "$out" == *"::error::"* ]] || fail "no error annotation for $2: $out"
}

expect_accept "$head" "a branch head"
expect_accept "$base" "an ancestor of a branch"
expect_accept "$feature" "a second branch"
expect_accept "$tagged" "a commit that only an annotated tag holds"
expect_reject "$pull" "a commit that only a pull request ref holds"
[[ "$out" == *"holds $pull"* ]] || fail "a pull request commit was fetched on demand: $out"
expect_reject "$dangling" "a commit that no ref holds"
[[ "$out" == *"holds $dangling"* ]] || fail "a dangling commit was fetched on demand: $out"
expect_reject "0000000000000000000000000000000000000000" "an unknown commit"
expect_reject "${head:0:12}" "an abbreviated SHA"
expect_reject "main" "a branch name"

run "file://$TMP/missing" "$head"
[ "$status" -ne 0 ] || fail "accepted a commit when the fetch failed: $out"
[[ "$out" == *"could not be fetched"* ]] || fail "a fetch failure does not name the fetch: $out"

echo "Benchmark target check: 10 cases passed"

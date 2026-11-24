#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CHECK="$ROOT/.github/scripts/check-ci-parity.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
	echo "FAIL: $*" >&2
	exit 1
}

repo="$TMP/repo"
git init -q "$repo"
cd "$repo"
git config user.name "Parity Test"
git config user.email "parity@example.com"
git config commit.gpgsign false

commit() {
	git add -A
	git commit -q --allow-empty -m "$1"
	git rev-parse HEAD
}

mkdir -p .github/workflows .github/scripts src
printf 'name: ci\n' >.github/workflows/ci.yml
printf 'name: soak\n' >.github/workflows/soak.yml
printf '#!/bin/sh\n' >.github/scripts/run.sh
printf 'queue_rules: []\n' >.mergify.yml
printf 'fn main() {}\n' >src/main.rs
master="$(commit master)"

printf 'name: soak v2\n' >.github/workflows/soak.yml
git rm -q .github/scripts/run.sh
printf 'name: new\n' >.github/workflows/new.yml
dev="$(commit dev)"

run() {
	git checkout -q --detach "$master"
	"$@"
	candidate="$(commit candidate)"
	status=0
	out="$(bash "$CHECK" "$candidate" "$master" "$dev" 2>&1)" || status=$?
	git checkout -q --detach "$master"
}

expect_pass() {
	run "${@:2}"
	[ "$status" -eq 0 ] || fail "$1: expected a pass, got $status: $out"
}

expect_drift() {
	local name="$1" path="$2"
	run "${@:3}"
	[ "$status" -eq 1 ] || fail "$name: expected drift, got $status: $out"
	grep -Fq "DRIFT: $path " <<<"$out" || fail "$name: the drift report does not name $path: $out"
}

noop() { :; }
expect_pass "the candidate equals master" noop

dev_soak() { printf 'name: soak v2\n' >.github/workflows/soak.yml; }
expect_pass "a file comes from dev" dev_soak

dev_delete() { git rm -q .github/scripts/run.sh; }
expect_pass "a file is deleted on dev" dev_delete

dev_add() { printf 'name: new\n' >.github/workflows/new.yml; }
expect_pass "a file is added on dev" dev_add

mixed() { dev_soak; }
expect_pass "files come from master and from dev" mixed

source_change() { printf 'fn main() { println!("ca"); }\n' >src/main.rs; }
expect_pass "a file outside the CI control files changes" source_change

ca_edit() { printf 'name: ci on ca\n' >.github/workflows/ci.yml; }
expect_drift "a workflow changes only on the ca line" .github/workflows/ci.yml ca_edit

ca_add() { printf 'name: ca only\n' >.github/workflows/ca-only.yml; }
expect_drift "a workflow exists only on the ca line" .github/workflows/ca-only.yml ca_add

ca_delete() { git rm -q .github/workflows/ci.yml; }
expect_drift "a workflow is deleted only on the ca line" .github/workflows/ci.yml ca_delete

ca_mode() { chmod +x .github/workflows/ci.yml; git update-index --chmod=+x .github/workflows/ci.yml; }
expect_drift "a file mode changes only on the ca line" .github/workflows/ci.yml ca_mode

ca_mergify() { printf 'queue_rules: [ca]\n' >.mergify.yml; }
expect_drift "the Mergify configuration changes only on the ca line" .mergify.yml ca_mergify

ca_soak() { printf 'name: soak ca\n' >.github/workflows/soak.yml; }
expect_drift "a file differs from both master and dev" .github/workflows/soak.yml ca_soak

two() { ca_edit; ca_add; }
run two
[ "$status" -eq 1 ] || fail "two drifts: expected drift, got $status: $out"
[ "$(grep -c '^DRIFT: ' <<<"$out")" -eq 2 ] || fail "two drifts: the report does not name both paths: $out"

status=0
out="$(bash "$CHECK" "$master" "$master" 2>&1)" || status=$?
[ "$status" -eq 2 ] || fail "two arguments: expected exit 2, got $status: $out"
status=0
out="$(bash "$CHECK" "$master" "$master" no-such-revision 2>&1)" || status=$?
[ "$status" -eq 2 ] || fail "a missing revision: expected exit 2, got $status: $out"

printf 'PASS: a ca-release candidate passes only when each CI control file under .github/ and .mergify.yml matches master or dev in content, mode, and presence.\n'

#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL="$ROOT/.github/scripts/check-dockerfile-workspace.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}

cat >"$TMP/Cargo.toml" <<'TOML'
[workspace]
resolver = "2"
members = [
	"casper",
	"rspace++",
	"rholang",
	"rholang/fixtures",
	"scripts/tool",
]
TOML
cat >"$TMP/Dockerfile" <<'DOCKER'
FROM rust AS builder
COPY --from=xx / /
COPY Cargo.toml Cargo.lock ./
COPY casper/ ./casper/
COPY rspace++/ ./rspace++/
COPY rholang/ ./rholang/
COPY scripts/tool/ ./scripts/tool/
# cargo fetch reads every member manifest.
RUN xx-cargo --setup-target-triple && \
    cargo fetch --locked
COPY late/ ./late/
FROM debian
COPY runtime/ ./runtime/
COPY --from=builder --chmod=755 /build/node_binary /opt/docker/bin/node
DOCKER

# check CARGO DOCKERFILE: sets status and out.
check() {
	set +e
	out="$(bash "$TOOL" "$1" "$2" 2>&1)"
	status=$?
	set -e
}

check "$TMP/Cargo.toml" "$TMP/Dockerfile"
[ "$status" -eq 0 ] || fail "a fully copied workspace failed: $out"

sed '/scripts\/tool/d' "$TMP/Dockerfile" >"$TMP/missing.Dockerfile"
check "$TMP/Cargo.toml" "$TMP/missing.Dockerfile"
[ "$status" -eq 1 ] || fail "a member without a COPY passed: $out"
[[ "$out" == *"scripts/tool has no COPY"* ]] || fail "the missing member is not named: $out"
[[ "$out" == *"COPY scripts/tool/ ./scripts/tool/"* ]] || fail "the fix is not given: $out"

sed '/rholang/d' "$TMP/Dockerfile" >"$TMP/no-parent.Dockerfile"
check "$TMP/Cargo.toml" "$TMP/no-parent.Dockerfile"
[ "$status" -eq 1 ] && [[ "$out" == *"rholang/fixtures has no COPY"* ]] ||
	fail "a nested member passed without its parent COPY: $out"

printf '[workspace]\nmembers = ["casper", "casper-extra"]\n' >"$TMP/prefix.toml"
check "$TMP/prefix.toml" "$TMP/Dockerfile"
[ "$status" -eq 1 ] && [[ "$out" == *"casper-extra has no COPY"* ]] ||
	fail "a COPY of casper/ covered the member casper-extra: $out"

sed 's|^COPY casper/ ./casper/$|COPY --from=other casper/ ./casper/|' "$TMP/Dockerfile" >"$TMP/from.Dockerfile"
check "$TMP/Cargo.toml" "$TMP/from.Dockerfile"
[ "$status" -eq 1 ] && [[ "$out" == *"casper has no COPY"* ]] ||
	fail "a COPY from another stage covered a member: $out"

printf '[workspace]\nmembers = ["casper", "runtime"]\n' >"$TMP/runtime.toml"
check "$TMP/runtime.toml" "$TMP/Dockerfile"
[ "$status" -eq 1 ] && [[ "$out" == *"runtime has no COPY"* ]] ||
	fail "a COPY in the runtime stage covered a member: $out"
printf '[workspace]\nmembers = ["casper", "late"]\n' >"$TMP/late.toml"
check "$TMP/late.toml" "$TMP/Dockerfile"
[ "$status" -eq 1 ] && [[ "$out" == *"late has no COPY"* ]] ||
	fail "a COPY after cargo fetch covered a member: $out"
grep -v 'cargo fetch --locked' "$TMP/Dockerfile" | sed 's/ && \\$//' >"$TMP/no-fetch.Dockerfile"
check "$TMP/Cargo.toml" "$TMP/no-fetch.Dockerfile"
[ "$status" -eq 2 ] || fail "a Dockerfile without cargo fetch passed: $out"
{ printf 'FROM rust AS early\nCOPY scripts/tool/ ./scripts/tool/\n'; grep -v '^COPY scripts/tool/' "$TMP/Dockerfile"; } >"$TMP/early.Dockerfile"
check "$TMP/Cargo.toml" "$TMP/early.Dockerfile"
[ "$status" -eq 1 ] && [[ "$out" == *"scripts/tool has no COPY"* ]] ||
	fail "a COPY in an earlier stage covered a member: $out"

printf '[workspace]\nresolver = "2"\n' >"$TMP/empty.toml"
check "$TMP/empty.toml" "$TMP/Dockerfile"
[ "$status" -eq 2 ] || fail "a manifest without members passed: $out"

printf '[workspace]\nmembers = ["casper", "scripts/tool"]\n' >"$TMP/inline.toml"
check "$TMP/inline.toml" "$TMP/Dockerfile"
[ "$status" -eq 0 ] || fail "an inline member list failed: $out"
printf '[workspace]\nmembers = ["casper", "scripts/other"]\n' >"$TMP/inline-missing.toml"
check "$TMP/inline-missing.toml" "$TMP/Dockerfile"
[ "$status" -eq 1 ] && [[ "$out" == *"scripts/other has no COPY"* ]] ||
	fail "an inline member without a COPY passed: $out"

check "$ROOT/Cargo.toml" "$ROOT/node/Dockerfile"
[ "$status" -eq 0 ] || fail "the repository workspace has a member without a COPY: $out"

ruby -ryaml -e '
  steps = YAML.load_file(ARGV[0]).dig("jobs", "static_checks", "steps") || []
  abort "Static Checks does not run check-dockerfile-workspace.sh" unless
    steps.any? { |s| s["run"].to_s.strip == "bash .github/scripts/check-dockerfile-workspace.sh" }
' "$ROOT/.github/workflows/ci.yml" || fail "ci.yml wiring"

printf 'PASS: every workspace member needs a COPY of its directory or a parent directory into node/Dockerfile. A COPY from another stage, into another stage, after cargo fetch, or of a directory whose name is only a prefix of the member name does not count. A Dockerfile without cargo fetch fails. The repository workspace passes, and Static Checks runs the check.\n'

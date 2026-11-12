#!/usr/bin/env bash
# Fixture test for coverage-summary.sh.
set -euo pipefail

SCRIPT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/coverage-summary.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail=0

check() {
	if ! grep -qF "$1" <<<"$2"; then
		printf '::error::expected output to contain: %s\n%s\n' "$1" "$2"
		fail=1
	fi
}

check_absent() {
	if grep -qF "$1" <<<"$2"; then
		printf '::error::expected output to omit: %s\n%s\n' "$1" "$2"
		fail=1
	fi
}

mklcov() { # file lines covered [first-line]: lines first..first+lines-1, the first `covered` of them hit
	local first="${4:-1}"
	{
		echo "SF:/src/lib.rs"
		for ((line = first; line < first + $2; line++)); do
			if [ $((line - first)) -lt "$3" ]; then echo "DA:$line,3"; else echo "DA:$line,0"; fi
		done
		echo "end_of_record"
	} >"$tmp/$1"
}

mkjson() { # crate lines covered
	mklcov "coverage-$1.lcov" "$2" "$3"
}

mkjson crypto 100 80
mkjson models 100 50

if out="$("$SCRIPT" "$tmp" crypto models graphz 2>&1)"; then
	printf '::error::expected low or missing coverage to fail\n%s\n' "$out"
	fail=1
fi
check "| Crate | Lines | Covered | Line coverage |" "$out"
check "| crypto | 100 | 80 | 80.0% |" "$out"
check "| models | 100 | 50 | 50.0% |" "$out"
check "| graphz | — | — | missing |" "$out"
check "| **All** | 200 | 130 | 65.0% |" "$out"
check "Some crates did not report coverage" "$out"
check "models line coverage is below 80%" "$out"
check "workspace line coverage is below 80%" "$out"

mkjson models 100 90
mkjson graphz 50 40
if ! out="$("$SCRIPT" "$tmp" crypto models graphz 2>&1)"; then
	printf '::error::expected sufficient coverage to pass\n%s\n' "$out"
	fail=1
fi
check "| **All** | 250 | 210 | 84.0% |" "$out"
check "Coverage gate passed" "$out"
check_absent "missing" "$out"

mkjson crypto 100 100
mkjson models 100 70
mkjson graphz 50 50
if out="$("$SCRIPT" "$tmp" crypto models graphz 2>&1)"; then
	printf '::error::expected one low crate to fail\n%s\n' "$out"
	fail=1
fi
check "| **All** | 250 | 220 | 88.0% |" "$out"
check "models line coverage is below 80%" "$out"
check_absent "workspace line coverage is below 80%" "$out"

# Two shards report the same 100 lines. Shard 1 ran lines 1-60, shard 2 ran
# lines 41-90: the crate has 100 lines, 90 of them covered.
mklcov coverage-casper-1.lcov 100 60
{
	echo "SF:/src/lib.rs"
	for ((line = 1; line <= 100; line++)); do
		if [ "$line" -ge 41 ] && [ "$line" -le 90 ]; then echo "DA:$line,1"; else echo "DA:$line,0"; fi
	done
	echo "end_of_record"
} >"$tmp/coverage-casper-2.lcov"
if ! out="$("$SCRIPT" "$tmp" casper 2>&1)"; then
	printf '::error::expected merged shard coverage to pass\n%s\n' "$out"
	fail=1
fi
check "| casper | 100 | 90 | 90.0% |" "$out"

echo "not lcov" >"$tmp/coverage-shared.lcov"
if out="$("$SCRIPT" "$tmp" shared 2>&1)"; then
	printf '::error::expected malformed coverage to fail\n%s\n' "$out"
	fail=1
fi
check "| shared | — | — | missing |" "$out"
check "| **All** | 0 | 0 | n/a |" "$out"

printf 'SF:/src/lib.rs\nDA:1,x\nend_of_record\n' >"$tmp/coverage-node.lcov"
if out="$("$SCRIPT" "$tmp" node 2>&1)"; then
	printf '::error::expected incomplete coverage to fail\n%s\n' "$out"
	fail=1
fi
check "| node | — | — | missing |" "$out"
check "| **All** | 0 | 0 | n/a |" "$out"

if out="$(COVERAGE_MINIMUM=invalid "$SCRIPT" "$tmp" crypto 2>&1)"; then
	printf '::error::expected an invalid minimum to fail\n%s\n' "$out"
	fail=1
fi
check "Coverage minimum must be a number from 0 through 100." "$out"

if out="$("$SCRIPT" "$tmp" 2>&1)"; then
	printf '::error::expected incomplete default coverage to fail\n%s\n' "$out"
	fail=1
fi
for crate in rspace_plus_plus rholang shared node models crypto block-storage comm graphz casper; do
	check "| $crate |" "$out"
done

if [ "$fail" -ne 0 ]; then
	exit 1
fi
echo "ok: coverage-summary.sh enforces per-crate and workspace coverage"

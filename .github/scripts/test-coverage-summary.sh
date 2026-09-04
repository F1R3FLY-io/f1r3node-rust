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

mkjson() { # crate lines covered
	jq -n --argjson count "$2" --argjson covered "$3" \
		'{data:[{totals:{lines:{count:$count,covered:$covered,percent:(100 * $covered / $count)}}}]}' \
		>"$tmp/coverage-$1.json"
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

echo "not json" >"$tmp/coverage-shared.json"
if out="$("$SCRIPT" "$tmp" shared 2>&1)"; then
	printf '::error::expected malformed coverage to fail\n%s\n' "$out"
	fail=1
fi
check "| shared | — | — | missing |" "$out"
check "| **All** | 0 | 0 | n/a |" "$out"

jq -n '{data:[{totals:{lines:{count:10,covered:null,percent:null}}}]}' \
	>"$tmp/coverage-node.json"
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

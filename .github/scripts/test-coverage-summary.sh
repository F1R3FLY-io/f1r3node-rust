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

# Two crates report, one does not.
mkjson crypto 100 80
mkjson models 100 50

out="$("$SCRIPT" "$tmp" crypto models graphz)"
check "| Crate | Lines | Covered | Line coverage |" "$out"
check "| crypto | 100 | 80 | 80.0% |" "$out"
check "| models | 100 | 50 | 50.0% |" "$out"
check "| graphz | — | — | missing |" "$out"
check "| **All** | 200 | 130 | 65.0% |" "$out"
check "Some crates did not report coverage" "$out"

# Every crate reports: weighted overall, no missing note.
mkjson graphz 50 50
out="$("$SCRIPT" "$tmp" crypto models graphz)"
check "| **All** | 250 | 180 | 72.0% |" "$out"
check_absent "missing" "$out"

# Malformed and incomplete files read as missing instead of aborting the report.
echo "not json" >"$tmp/coverage-shared.json"
out="$("$SCRIPT" "$tmp" shared)"
check "| shared | — | — | missing |" "$out"
check "| **All** | 0 | 0 | n/a |" "$out"

jq -n '{data:[{totals:{lines:{count:10,covered:null,percent:null}}}]}' \
	>"$tmp/coverage-node.json"
out="$("$SCRIPT" "$tmp" node)"
check "| node | — | — | missing |" "$out"
check "| **All** | 0 | 0 | n/a |" "$out"

# The default crate list covers every crate in the ci.yml coverage matrix.
out="$("$SCRIPT" "$tmp")"
for crate in rspace_plus_plus rholang shared node models crypto block-storage comm graphz casper; do
	check "| $crate |" "$out"
done

if [ "$fail" -ne 0 ]; then
	exit 1
fi
echo "ok: coverage-summary.sh renders rows, overall, and missing states"

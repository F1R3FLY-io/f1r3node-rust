#!/usr/bin/env bash
# Build a per-crate line-coverage table from cargo-llvm-cov summary files.
#
# Input: a directory with coverage-<crate>.json files, each produced by
# `cargo llvm-cov report --json --summary-only`. Output: a Markdown table on
# stdout. A missing or unreadable file becomes a "missing" row so one failed
# matrix leg cannot hide the other crates' numbers. The overall row is the
# line-weighted total across the crates that reported.
set -euo pipefail
export LC_ALL=C

dir="${1:?usage: coverage-summary.sh <json-dir> [crate ...]}"
minimum="${COVERAGE_MINIMUM:-80}"
[[ "$minimum" =~ ^([0-9]+)(\.[0-9]+)?$ ]] || {
	echo "Coverage minimum must be a number from 0 through 100." >&2
	exit 2
}
awk -v minimum="$minimum" 'BEGIN { exit !(minimum >= 0 && minimum <= 100) }' || {
	echo "Coverage minimum must be a number from 0 through 100." >&2
	exit 2
}
shift
if [ "$#" -gt 0 ]; then
	crates=("$@")
else
	# Same set as the coverage matrix in ci.yml.
	crates=(rspace_plus_plus rholang shared node models crypto block-storage comm graphz casper)
fi

echo "| Crate | Lines | Covered | Line coverage |"
echo "| --- | ---: | ---: | ---: |"

total=0
covered=0
missing=0
gate_failed=0
failures=()
for crate in "${crates[@]}"; do
	file="$dir/coverage-$crate.json"
	stats=""
	if [ -s "$file" ]; then
		stats="$(jq -er '
			.data[0].totals.lines
			| select([.count, .covered, .percent] | all(.[]; type == "number"))
			| select(.count >= 0 and .covered >= 0 and .covered <= .count and .percent >= 0 and .percent <= 100)
			| "\(.count) \(.covered) \(.percent)"
		' "$file" 2>/dev/null || true)"
	fi
	if [ -z "$stats" ]; then
		echo "| $crate | — | — | missing |"
		missing=1
		gate_failed=1
		failures+=("$crate did not report coverage")
		continue
	fi
	read -r line_count covered_count percent <<<"$stats"
	printf '| %s | %s | %s | %.1f%% |\n' "$crate" "$line_count" "$covered_count" "$percent"
	if ! awk -v covered="$covered_count" -v count="$line_count" -v minimum="$minimum" \
		'BEGIN { exit !(count > 0 && 100 * covered >= minimum * count) }'; then
		gate_failed=1
		failures+=("$crate line coverage is below ${minimum}%")
	fi
	total=$((total + line_count))
	covered=$((covered + covered_count))
done

if [ "$total" -eq 0 ]; then
	echo "| **All** | 0 | 0 | n/a |"
	gate_failed=1
	failures+=("workspace did not report coverage")
else
	overall="$(awk -v c="$covered" -v t="$total" 'BEGIN { printf "%.1f", 100 * c / t }')"
	echo "| **All** | $total | $covered | ${overall}% |"
	if ! awk -v covered="$covered" -v count="$total" -v minimum="$minimum" \
		'BEGIN { exit !(100 * covered >= minimum * count) }'; then
		gate_failed=1
		failures+=("workspace line coverage is below ${minimum}%")
	fi
fi

if [ "$missing" -eq 1 ]; then
	echo ""
	echo "Some crates did not report coverage. Check the Coverage jobs for failures."
fi

if [ "$gate_failed" -eq 0 ]; then
	echo ""
	echo "Coverage gate passed. Each crate and the workspace have at least ${minimum}% line coverage."
else
	echo ""
	echo "Coverage gate failed:"
	printf -- '- %s\n' "${failures[@]}"
	exit 1
fi

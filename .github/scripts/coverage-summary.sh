#!/usr/bin/env bash
# Build a per-crate line-coverage table from cargo-llvm-cov summary files.
#
# Input: a directory with coverage-<crate>.json files, each produced by
# `cargo llvm-cov report --json --summary-only`. Output: a Markdown table on
# stdout. A missing or unreadable file becomes a "missing" row so one failed
# matrix leg cannot hide the other crates' numbers. The overall row is the
# line-weighted total across the crates that reported. This report never fails
# on a low percentage; thresholds are a separate, ratified decision.
set -euo pipefail
export LC_ALL=C

dir="${1:?usage: coverage-summary.sh <json-dir> [crate ...]}"
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
		continue
	fi
	read -r line_count covered_count percent <<<"$stats"
	printf '| %s | %s | %s | %.1f%% |\n' "$crate" "$line_count" "$covered_count" "$percent"
	total=$((total + line_count))
	covered=$((covered + covered_count))
done

if [ "$total" -eq 0 ]; then
	echo "| **All** | 0 | 0 | n/a |"
else
	overall="$(awk -v c="$covered" -v t="$total" 'BEGIN { printf "%.1f", 100 * c / t }')"
	echo "| **All** | $total | $covered | ${overall}% |"
fi

if [ "$missing" -eq 1 ]; then
	echo ""
	echo "Some crates did not report coverage. Check the Coverage jobs for failures."
fi

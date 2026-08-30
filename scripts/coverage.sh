#!/usr/bin/env bash
# Measure per-crate line coverage with cargo-llvm-cov and nextest.
#
# Usage: scripts/coverage.sh [crate ...]
# With no arguments it measures every crate in the ci.yml coverage matrix.
# Results land in target/coverage/: one coverage-<crate>.json and one
# coverage-<crate>.lcov per crate, followed by the rendered summary table.
#
# Requires cargo-llvm-cov (cargo install cargo-llvm-cov --locked) and the
# llvm-tools-preview component.
set -euo pipefail
cd "$(dirname "$0")/.."

if [ "$#" -gt 0 ]; then
	crates=("$@")
else
	crates=(rspace_plus_plus rholang shared node models crypto block-storage comm graphz casper)
fi

if ! cargo llvm-cov --version >/dev/null 2>&1; then
	echo "error: cargo-llvm-cov not installed; run: cargo install cargo-llvm-cov --locked" >&2
	exit 1
fi

# `target/llvm-cov` is llvm-cov's own report dir, which `clean` removes, so
# outputs live in target/coverage instead.
out="target/coverage"
mkdir -p "$out"
ulimit -n 65536 2>/dev/null || true

for crate in "${crates[@]}"; do
	echo "=== $crate ==="
	cargo llvm-cov clean
	cargo llvm-cov --release -p "$crate" --lib --bins --no-report
	cargo llvm-cov report --release -p "$crate" \
		--json --summary-only --output-path "$out/coverage-$crate.json"
	cargo llvm-cov report --release -p "$crate" \
		--lcov --output-path "$out/coverage-$crate.lcov"
done

.github/scripts/coverage-summary.sh "$out" "${crates[@]}"

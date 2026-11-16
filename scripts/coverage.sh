#!/usr/bin/env bash
# Measure per-crate line coverage with cargo-llvm-cov and nextest.
#
# Usage: scripts/coverage.sh [crate ...]
# With no arguments it measures every crate that the ci.yml test jobs measure.
# Results land in target/coverage/: one coverage-<crate>.lcov per crate,
# followed by the rendered summary table.
#
# Requires cargo-llvm-cov (cargo install cargo-llvm-cov --locked) and the
# llvm-tools-preview component. Doctests do not count toward coverage, same as
# the CI test jobs.
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
# Two classes leave the measured denominator, for the same reason: their
# lines cannot be exercised by the crate's own test run.
# 1. Src-shipped test scaffolding (rholang's interpreter/test_utils,
#    block-storage's test-internals helpers): compiled for cross-crate test
#    consumers only.
# 2. node's process bootstrap and wiring (main.rs, runtime/, the proposer
#    instance driver): they assemble and run the
#    live server processes, reachable only from a booted node, which the
#    integration suites in system-integration cover. heartbeat_proposer.rs
#    stays measured — it is logic, not wiring, and its unit tests prove it.
# Must stay identical to the regex in .github/scripts/run-crate-tests.sh.
ignore_regex='(/test_utils/|block-storage/src/rust/test/|node/src/main\.rs|node/src/rust/runtime/|node/src/rust/instances/proposer_instance\.rs)'

out="target/coverage"
mkdir -p "$out"
ulimit -n 65536 2>/dev/null || true

for crate in "${crates[@]}"; do
	echo "=== $crate ==="
	cargo llvm-cov clean
	cargo llvm-cov nextest --release -p "$crate" --no-tests=pass
	cargo llvm-cov report --release -p "$crate" \
		--ignore-filename-regex "$ignore_regex" \
		--lcov --output-path "$out/coverage-$crate.lcov"
done

.github/scripts/coverage-summary.sh "$out" "${crates[@]}"

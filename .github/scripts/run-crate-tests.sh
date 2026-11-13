#!/usr/bin/env bash
# Run one crate's tests once under cargo-llvm-cov and write the line coverage
# of that run to target/coverage/coverage-<crate>[-<shard>].lcov.
#
# CRATE names the package. PARTITION, as `<n>/<total>`, runs one nextest
# count-partition shard. DOCTESTS=false skips the doctests, which cannot be
# partitioned and so run in one shard only. The report is written before the
# doctests run, so doctests do not count toward coverage.
set -euo pipefail

crate="${CRATE:?CRATE is required}"
partition="${PARTITION:-}"
doctests="${DOCTESTS:-true}"
name="$crate${partition:+-${partition%%/*}}"

# Must stay identical to the regex in scripts/coverage.sh, which explains it.
ignore_regex='(/test_utils/|block-storage/src/rust/test/|node/src/main\.rs|node/src/rust/runtime/|node/src/rust/consensus/casper/instances/proposer_instance\.rs)'

ulimit -n 65536
mkdir -p /tmp/test-logs target/coverage
# A restored build cache can carry profiles from an earlier run.
cargo llvm-cov clean --profraw-only

args=(--locked --release -p "$crate" --no-tests=pass --no-report)
[ -z "$partition" ] || args+=(--partition "count:$partition")
nt=0
cargo llvm-cov nextest "${args[@]}" 2>&1 | tee "/tmp/test-logs/$name.txt" || nt=$?

cargo llvm-cov report --release -p "$crate" --ignore-filename-regex "$ignore_regex" \
	--lcov --output-path "target/coverage/coverage-$name.lcov"

dt=0
if [ "$doctests" = true ]; then
	cargo llvm-cov test --locked --release -p "$crate" --doc --no-report 2>&1 \
		| tee "/tmp/test-logs/$name-doc.txt" || dt=$?
fi

echo "nextest exit=$nt doctest exit=$dt"
[ "$nt" -eq 0 ] && [ "$dt" -eq 0 ]

#!/usr/bin/env bash
# scripts/check-cost-accounted-rho-coverage.sh
#
# Local-only line-coverage report for the multi-sig + LL-rich algebra
# substrate. Uses cargo-llvm-cov (already available in the dev
# environment per `cargo install --list`). Per team policy, this stays
# local — NOT a CI gate.
#
# Targets these critical files with thresholds:
#   - crypto/src/rust/signatures/signed.rs                   (>= 95%)
#   - models/src/rust/casper/protocol/casper_message.rs      (>= 90%)
#   - rholang/src/rust/interpreter/accounting/mod.rs         (>= 85%)
#   - casper/src/rust/rholang/runtime.rs                     (>= 80%)
#
# NOTE (2026-06-01, per user direction): these LINE-coverage thresholds are an
# UNVALIDATED, superficial proxy. Line coverage is not a proper completeness
# measure (BRANCH coverage is), and the numbers above were aspirational guesses
# (introduced in 9474d987; never validated, because this gate was blocked by
# failing tests until 2026-06-01). The OPERATIVE quality bar for the
# cost-accounting substrate is thorough testing + multi-prover formal
# verification (Rocq + Lean + TLA+/TLAPS + Sage), NOT this LoC number.
# Threshold recalibration (and a move to branch coverage) is DEFERRED to a
# later user-led testing pass; until then a sub-threshold result here is
# informational, not a completion blocker.
#
# Output:
#   - target/llvm-cov/html/index.html (browsable line-level coverage)
#   - target/llvm-cov/cost-accounted-rho-summary.txt
#
# Usage:
#   bash scripts/check-cost-accounted-rho-coverage.sh
#   bash scripts/check-cost-accounted-rho-coverage.sh --html-only

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="$REPO_ROOT/target/llvm-cov"
SUMMARY="$OUT_DIR/cost-accounted-rho-summary.txt"

mkdir -p "$OUT_DIR"

# Run llvm-cov on the multi-sig-touching test surfaces. The
# --workspace + --release combination minimizes runtime; we narrow
# to the relevant test runners via env (the suites themselves drive
# the cosigned + LL paths).
echo "Running cargo llvm-cov (this can take 10+ minutes for full workspace)..."
cargo llvm-cov clean --workspace
# cargo-llvm-cov rejects --html together with --summary-only ("--summary-only
# may not be used together with --html"). Use the idiomatic two-pass pattern:
# run the instrumented tests ONCE with --no-report to collect the profdata,
# then render the two reports separately from that same data — the textual
# per-file summary (the threshold gate below parses it) and the browsable HTML.
# This changes only HOW the reports are emitted, not WHAT is measured, so the
# critical-file thresholds are unaffected by the fix itself.
cargo llvm-cov \
    --workspace \
    --no-fail-fast \
    --release \
    --no-report \
    -- \
    --skip schnorr_secp256k1_experimental
cargo llvm-cov report --release --summary-only 2>&1 | tee "$SUMMARY"
cargo llvm-cov report --release --html --output-dir "$OUT_DIR/html"

echo
echo "Coverage HTML: $OUT_DIR/html/index.html"
echo "Coverage summary: $SUMMARY"

# Extract per-file coverage and check thresholds.
echo
echo "Critical-file coverage check:"
declare -A THRESHOLDS
THRESHOLDS["crypto/src/rust/signatures/signed.rs"]=95
THRESHOLDS["models/src/rust/casper/protocol/casper_message.rs"]=90
THRESHOLDS["rholang/src/rust/interpreter/accounting/mod.rs"]=85
THRESHOLDS["casper/src/rust/rholang/runtime.rs"]=80

failures=0
for file in "${!THRESHOLDS[@]}"; do
    threshold="${THRESHOLDS[$file]}"
    # Extract LINE coverage from the summary. The `cargo llvm-cov
    # --summary-only` columns are (whitespace-separated, filename has no
    # spaces): $1 Filename, $2 Regions, $3 MissedRegions, $4 Cover(region%),
    # $5 Functions, $6 MissedFunctions, $7 Executed(function%), $8 Lines,
    # $9 MissedLines, $10 Cover(LINE%), $11 Branches, ... So line coverage —
    # the metric this gate documents — is field $10 (NOT $7, which is function
    # coverage; this gate previously read $7 by mistake and so gated on the
    # wrong metric).
    actual=$(grep "$file" "$SUMMARY" 2>/dev/null | awk '{print $10}' | sed 's/%//' || echo "0")
    if [[ -z "$actual" ]]; then
        actual=0
    fi
    actual_int="${actual%.*}"
    if [[ -z "$actual_int" ]]; then
        actual_int=0
    fi
    status="PASS"
    if (( actual_int < threshold )); then
        status="BELOW THRESHOLD"
        failures=$((failures + 1))
    fi
    printf "  %-60s actual=%s%%  required=%d%%  [%s]\n" \
        "$file" "$actual" "$threshold" "$status"
done

echo
if (( failures > 0 )); then
    echo "$failures file(s) below threshold — see HTML report for uncovered lines."
    exit 1
fi
echo "All critical files meet their coverage thresholds."

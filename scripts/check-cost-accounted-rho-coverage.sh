#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$ROOT/target/llvm-cov"
SUMMARY_JSON="$OUT_DIR/cost-accounted-rho-summary.json"
SUMMARY="$OUT_DIR/cost-accounted-rho-summary.txt"
HTML_DIR="$OUT_DIR/html"
mkdir -p "$OUT_DIR"
cd "$ROOT"

active_llvm_bin="$(dirname "$(rustc --print target-libdir)")/bin"
export LLVM_PROFDATA="${LLVM_PROFDATA:-$active_llvm_bin/llvm-profdata}"
if [[ -z "${LLVM_COV:-}" ]]; then
    llvm_cov="$active_llvm_bin/llvm-cov"
    if newer_libdir="$(rustup run nightly rustc --print target-libdir 2>/dev/null)"; then
        newer_cov="$(dirname "$newer_libdir")/bin/llvm-cov"
        if [[ -x "$newer_cov" ]]; then
            llvm_cov="$newer_cov"
        fi
    fi
    export LLVM_COV="$llvm_cov"
fi

if [[ $# -gt 1 || ( $# -eq 1 && "$1" != "--html-only" && "$1" != "--report-only" ) ]]; then
    echo "usage: $0 [--html-only|--report-only]" >&2
    exit 2
fi

if [[ "${1:-}" == "--html-only" ]]; then
    cargo llvm-cov report --release --html --output-dir "$OUT_DIR"
    echo "Coverage HTML: $HTML_DIR/index.html"
    exit 0
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "error: jq is required to inspect branch-coverage JSON" >&2
    exit 2
fi
if ! cargo nextest --version >/dev/null 2>&1; then
    echo "error: cargo-nextest is required for scoped cost-accounting coverage" >&2
    exit 2
fi

coverage_filter='(package(crypto) & test(/signatures::signed::/)) | (package(models) & test(/casper_message::tests::|mixed_algorithm_cosigned/)) | (package(rholang) & test(/accounting|runtime_budget/)) | (package(casper) & test(/multi_sig::|runtime_manager_test::(client_fuel|close_block_supply|fee_|gate_decision|mintphlogiston|redeem_|replay_on_independent_runtime|slash_zeros|system_settlement|wallet_channel|mixed_success)/))'

test_rc=0
if [[ "${1:-}" != "--report-only" ]]; then
    cargo llvm-cov clean --workspace
    if cargo llvm-cov nextest \
        --branch \
        --release \
        --no-report \
        -p crypto -p models -p rholang -p casper \
        -E "$coverage_filter" \
        --ignore-default-filter \
        --no-fail-fast \
        --test-threads=4; then
        :
    else
        test_rc=$?
    fi
fi

cargo llvm-cov report --release --json --summary-only --output-path "$SUMMARY_JSON"
cargo llvm-cov report --release --html --output-dir "$OUT_DIR"

files=(
    crypto/src/rust/signatures/signed.rs
    models/src/rust/casper/protocol/casper_message.rs
    rholang/src/rust/interpreter/accounting/mod.rs
    casper/src/rust/rholang/runtime.rs
)
line_refs=(95 90 85 80)

printf 'Critical-file cost-accounting coverage\n' >"$SUMMARY"
for i in "${!files[@]}"; do
    file="${files[$i]}"
    metrics="$(jq -er --arg filename "$ROOT/$file" '
        [.data[].files[] | select(.filename == $filename) | .summary]
        | if length == 1 then
            .[0] | [.lines.percent, .branches.count, .branches.percent] | @tsv
          else
            error("expected exactly one coverage summary for " + $filename)
          end
    ' "$SUMMARY_JSON")"
    IFS=$'\t' read -r line_pct branch_count branch_pct <<<"$metrics"
    if [[ "$branch_count" -eq 0 ]]; then
        echo "error: no instrumented branches for $file" >&2
        exit 1
    fi
    status="at or above line reference"
    if (( ${line_pct%.*} < line_refs[i] )); then
        status="below line reference"
    fi
    printf '%-60s lines=%7.2f%% branches=%7.2f%% (%s instrumented) reference=%d%% [%s]\n' \
        "$file" "$line_pct" "$branch_pct" "$branch_count" "${line_refs[$i]}" "$status" \
        | tee -a "$SUMMARY"
done

echo "Coverage JSON: $SUMMARY_JSON"
echo "Coverage summary: $SUMMARY"
echo "Coverage HTML: $HTML_DIR/index.html"
if [[ "$test_rc" -ne 0 ]]; then
    echo "error: scoped cost-accounting tests failed with exit $test_rc" >&2
    exit "$test_rc"
fi

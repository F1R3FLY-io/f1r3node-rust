#!/usr/bin/env bash
set -euo pipefail

SEARCH_TIER="${SLASHING_SEARCH_TIER:-${SEARCH_TIER:-smoke}}"
SEARCH_OUTPUT_DIR="${SEARCH_OUTPUT_DIR:-$PWD/target/slashing-search-horizon}"
SAGE_OBJECTIVES="${SAGE_OBJECTIVES:-all}"
case "$SEARCH_TIER" in
    smoke)
        DEFAULT_FUZZ_RUNS=10000
        ;;
    frontier)
        DEFAULT_FUZZ_RUNS=100000
        ;;
    nightly)
        DEFAULT_FUZZ_RUNS=1000000
        ;;
    exhaustive)
        DEFAULT_FUZZ_RUNS=1000000
        ;;
    *)
        echo "Unknown slashing search tier: $SEARCH_TIER" >&2
        exit 2
        ;;
esac
FUZZ_RUNS="${FUZZ_RUNS:-$DEFAULT_FUZZ_RUNS}"
FUZZ_MAX_TOTAL_TIME="${FUZZ_MAX_TOTAL_TIME:-}"
ASAN_OPTIONS="${ASAN_OPTIONS:-detect_leaks=0}"
FUZZ_RUSTFLAGS="${FUZZ_RUSTFLAGS:--C target-feature=+aes,+sse2}"
KANI_RUSTFLAGS="${KANI_RUSTFLAGS:--Aexplicit-builtin-cfgs-in-flags --cfg target_feature=\"aes\" --cfg target_feature=\"sse2\"}"
MIRI_RUSTFLAGS="${MIRI_RUSTFLAGS:-$FUZZ_RUSTFLAGS}"
MIRI_CACHE_HOME="${MIRI_CACHE_HOME:-$PWD/target/miri-cache}"
SYSTEMD_MEMORY_MAX="${SYSTEMD_MEMORY_MAX:-}"
SYSTEMD_CPU_QUOTA="${SYSTEMD_CPU_QUOTA:-}"
KANI_HARNESSES=(
    checked_base_seq_matches_i32_predecessor
    checked_next_seq_matches_i32_successor
    epoch_for_block_number_rejects_invalid_domain
    epoch_for_block_number_matches_bounded_floor_division
    slash_target_epoch_is_current_matches_epoch_projection
    slash_evidence_epoch_matches_target_matches_epoch_projection
    received_slash_deploy_authorized_rejects_invalid_domain
    received_slash_deploy_authorized_is_conjunction_on_bounded_domain
    slash_target_has_positive_bond_matches_positive
    received_authorization_requires_positive_bond_on_bounded_domain
    received_authorization_requires_invalid_evidence_on_bounded_domain
    received_authorization_requires_current_epoch_on_bounded_domain
    received_authorization_requires_evidence_epoch_on_bounded_domain
    slash_target_key_collides_matches_pair_equality
)
FUZZ_TARGETS=(
    "slashing_arithmetic:64"
    "slash_deploy_roundtrip:512"
    "block_message_roundtrip:4096"
    "slash_authorization_paths:2048"
    "equivocation_detector_paths:2048"
    "slash_lifecycle_trace:4096"
)

run_limited() {
    if [[ -n "$SYSTEMD_MEMORY_MAX" ]] && command -v systemd-run >/dev/null 2>&1; then
        local args=(--user --scope -p "MemoryMax=$SYSTEMD_MEMORY_MAX")
        if [[ -n "$SYSTEMD_CPU_QUOTA" ]]; then
            args+=(-p "CPUQuota=$SYSTEMD_CPU_QUOTA")
        fi
        systemd-run "${args[@]}" "$@"
    else
        "$@"
    fi
}

write_run_metadata() {
    mkdir -p "$SEARCH_OUTPUT_DIR"
    {
        printf '{\n'
        printf '  "tier": "%s",\n' "$SEARCH_TIER"
        printf '  "fuzz_runs": %s,\n' "$FUZZ_RUNS"
        printf '  "sage_objectives": "%s",\n' "$SAGE_OBJECTIVES"
        printf '  "run_coverage": "%s",\n' "${RUN_COVERAGE:-0}"
        printf '  "run_mutants": "%s",\n' "${RUN_MUTANTS:-0}"
        printf '  "run_deny": "%s",\n' "${RUN_DENY:-0}"
        printf '  "run_apalache": "%s"\n' "${RUN_APALACHE:-0}"
        printf '}\n'
    } > "$SEARCH_OUTPUT_DIR/run-metadata.json"
}

triage_fuzz_artifacts() {
    mkdir -p "$SEARCH_OUTPUT_DIR"
    local report="$SEARCH_OUTPUT_DIR/fuzz-artifact-triage.txt"
    if [[ ! -d fuzz/artifacts ]]; then
        printf 'no fuzz/artifacts directory\n' > "$report"
        return
    fi
    {
        printf 'empty_artifacts\n'
        find fuzz/artifacts -type f -size 0 -print | sort
        printf '\nnonempty_artifacts\n'
        find fuzz/artifacts -type f ! -size 0 -print | sort
    } > "$report"
}

run_fuzz_target() {
    local target="$1"
    local max_len="$2"
    local args=("-runs=${FUZZ_RUNS}" "-max_len=${max_len}")
    if [[ -n "$FUZZ_MAX_TOTAL_TIME" ]]; then
        args+=("-max_total_time=${FUZZ_MAX_TOTAL_TIME}")
    fi
    run_limited env ASAN_OPTIONS="$ASAN_OPTIONS" RUSTFLAGS="$FUZZ_RUSTFLAGS" cargo fuzz run "$target" -- "${args[@]}"
}

run_hypothesis_sage_replay() {
    local profile="$1"
    local mode="$2"
    if ! command -v sage >/dev/null 2>&1; then
        echo "SKIP Sage/Hypothesis replay: sage not found"
        return
    fi
    mkdir -p "$SEARCH_OUTPUT_DIR"
    local json_out="$SEARCH_OUTPUT_DIR/hypothesis-${profile}-${mode}.json"
    local fixture_out="$SEARCH_OUTPUT_DIR/hypothesis-${profile}-${mode}-fixtures.json"
    local coverage_out="$SEARCH_OUTPUT_DIR/hypothesis-${profile}-${mode}-coverage.json"
    local rust_fixtures_out="$SEARCH_OUTPUT_DIR/hypothesis-${profile}-${mode}-rust-fixtures.json"
    DOT_SAGE="${DOT_SAGE:-/tmp/codex-sage}" sage formal/sage/slashing/hypothesis_search/hypothesis_scenario_search.sage -- \
        --profile "$profile" \
        --search-mode "$mode" \
        --objectives "$SAGE_OBJECTIVES" \
        --json-out "$json_out" \
        --fixture-out "$fixture_out" \
        --coverage-out "$coverage_out" \
        --rust-fixtures-out "$rust_fixtures_out"
    SLASHING_REPLAY_JSON="$fixture_out" \
        SLASHING_RUST_FIXTURES_JSON="$rust_fixtures_out" \
        cargo test -p casper generated_
}

run_coverage() {
    if [[ "${RUN_COVERAGE:-0}" != "1" ]]; then
        return
    fi
    if ! cargo llvm-cov --version >/dev/null 2>&1; then
        echo "SKIP coverage: install cargo-llvm-cov"
        return
    fi
    mkdir -p "$SEARCH_OUTPUT_DIR"
    cargo llvm-cov --json --summary-only --output-path "$SEARCH_OUTPUT_DIR/slashing-coverage-summary.json" -p casper -- slashing
    cargo llvm-cov --lcov --output-path "$SEARCH_OUTPUT_DIR/slashing.lcov" --no-run -p casper
}

run_mutants() {
    if [[ "${RUN_MUTANTS:-0}" != "1" ]]; then
        return
    fi
    if ! cargo mutants --version >/dev/null 2>&1; then
        echo "SKIP cargo-mutants: install with 'cargo install cargo-mutants'"
        return
    fi
    run_limited cargo mutants --in-place --no-shuffle --timeout 120 --baseline=skip
}

run_deny() {
    if [[ "${RUN_DENY:-0}" != "1" ]]; then
        return
    fi
    if ! cargo deny --version >/dev/null 2>&1; then
        echo "SKIP cargo-deny: install with 'cargo install cargo-deny'"
        return
    fi
    cargo deny check
}

run_apalache() {
    if [[ "${RUN_APALACHE:-0}" != "1" ]]; then
        return
    fi
    if ! command -v apalache-mc >/dev/null 2>&1; then
        echo "SKIP Apalache: apalache-mc not found"
        return
    fi
    apalache-mc check formal/tlaplus/slashing/MC_AuthorizedSlashFlow.tla
    apalache-mc check formal/tlaplus/slashing/MC_TwoLevelSlashing.tla
}

write_run_metadata
triage_fuzz_artifacts
cargo test -p casper slash_authorization_regressions

if cargo fuzz --help >/dev/null 2>&1; then
    for target in "${FUZZ_TARGETS[@]}"; do
        run_fuzz_target "${target%%:*}" "${target##*:}"
    done
else
    echo "SKIP cargo-fuzz: install with 'cargo install cargo-fuzz'"
fi

if cargo kani --version >/dev/null 2>&1; then
    for harness in "${KANI_HARNESSES[@]}"; do
        RUSTFLAGS="$KANI_RUSTFLAGS" cargo kani -p casper --harness "$harness"
    done
else
    echo "SKIP Kani: install with 'cargo install kani-verifier && cargo kani setup'"
fi

run_coverage
run_mutants
run_deny
run_apalache

if [[ "${RUN_MIRI:-0}" == "1" ]]; then
    if cargo miri --version >/dev/null 2>&1; then
        XDG_CACHE_HOME="$MIRI_CACHE_HOME" RUSTFLAGS="$MIRI_RUSTFLAGS" cargo miri test -p casper checked_sequence_arithmetic_rejects_boundaries
    else
        echo "SKIP Miri: install with 'rustup component add miri'"
    fi
fi

case "$SEARCH_TIER" in
    smoke)
        ;;
    frontier)
        run_hypothesis_sage_replay quick frontier
        ;;
    nightly)
        run_hypothesis_sage_replay corpus all
        ;;
    exhaustive)
        run_hypothesis_sage_replay deep all
        if [[ "${RUN_ROCQ:-0}" == "1" ]]; then
            make -C formal/rocq/slashing
        fi
        if [[ "${RUN_TLA:-0}" == "1" ]]; then
            bash scripts/ci/check-tla-invariants.sh
        fi
        ;;
esac

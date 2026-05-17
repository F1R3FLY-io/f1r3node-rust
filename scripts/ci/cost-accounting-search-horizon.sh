#!/usr/bin/env bash
set -euo pipefail

SEARCH_TIER="${COST_ACCOUNTING_SEARCH_TIER:-${SEARCH_TIER:-smoke}}"
SEARCH_OUTPUT_DIR="${SEARCH_OUTPUT_DIR:-$PWD/target/cost-accounting-search-horizon}"
FORMAL_REPO="${COST_ACCOUNTING_FORMAL_REPO:-../f1r3node-cost-accounted-rho-calc}"
SEARCH_FAMILIES="${SEARCH_FAMILIES:-all}"
SEARCH_PROFILE="${SEARCH_PROFILE:-}"
SAGE_OBJECTIVES="${SAGE_OBJECTIVES:-all}"
SEARCH_RSS_LIMIT="${SEARCH_RSS_LIMIT:-32G}"
ALLOW_UNBOUNDED_SEARCH="${ALLOW_UNBOUNDED_SEARCH:-0}"
TLC_MAX_HEAP="${TLC_MAX_HEAP:-28g}"

case "$SEARCH_TIER" in
    smoke)
        DEFAULT_FUZZ_RUNS=10000
        ;;
    frontier)
        DEFAULT_FUZZ_RUNS=100000
        ;;
    nightly | exhaustive)
        DEFAULT_FUZZ_RUNS=1000000
        ;;
    *)
        echo "Unknown cost-accounting search tier: $SEARCH_TIER" >&2
        exit 2
        ;;
esac

FUZZ_RUNS="${FUZZ_RUNS:-$DEFAULT_FUZZ_RUNS}"
FUZZ_MAX_TOTAL_TIME="${FUZZ_MAX_TOTAL_TIME:-}"
ASAN_OPTIONS="${ASAN_OPTIONS:-detect_leaks=0}"
FUZZ_RUSTFLAGS="${FUZZ_RUSTFLAGS:--C target-feature=+aes,+sse2}"
KANI_RUSTFLAGS="${KANI_RUSTFLAGS:--Aexplicit-builtin-cfgs-in-flags --cfg kani --cfg target_feature=\"aes\" --cfg target_feature=\"sse2\"}"
KANI_TIMEOUT_SECONDS="${KANI_TIMEOUT_SECONDS:-120}"
MIRI_RUSTFLAGS="${MIRI_RUSTFLAGS:-$FUZZ_RUSTFLAGS}"
MIRI_CACHE_HOME="${MIRI_CACHE_HOME:-$PWD/target/miri-cache}"
SYSTEMD_MEMORY_MAX="${SYSTEMD_MEMORY_MAX:-$SEARCH_RSS_LIMIT}"
SYSTEMD_CPU_QUOTA="${SYSTEMD_CPU_QUOTA:-}"
SYSTEMD_DISABLE_SWAP="${SYSTEMD_DISABLE_SWAP:-1}"
export DOT_SAGE="${DOT_SAGE:-$SEARCH_OUTPUT_DIR/sage}"

FUZZ_TARGETS=(
    "runtime_budget_admission:2048"
    "cost_trace_roundtrip:2048"
    "processed_deploy_settlement:512"
    "replay_payload_cost_fields:2048"
    "cost_accounting_lifecycle_trace:4096"
    "cost_accounting_stateful_campaign:4096"
    "replay_settlement_differential:2048"
    "source_descriptor_resource_campaign:2048"
    "block_replay_auth_mutation:2048"
)

KANI_TARGETS=(
    "models:checked_total_phlo_charge_rejects_negative_inputs"
    "models:checked_total_phlo_charge_matches_product_on_small_valid_domain"
    "models:refund_amount_is_bounded_on_small_valid_domain"
    "rholang:cost_value_to_token_count_rejects_negative_values"
    "rholang:token_remaining_units_i64_saturates_to_i64_max"
)

run_limited() {
    if [[ -n "$SYSTEMD_MEMORY_MAX" ]] && command -v systemd-run >/dev/null 2>&1; then
        local args=(--user --scope -p "MemoryMax=$SYSTEMD_MEMORY_MAX")
        if [[ "$SYSTEMD_DISABLE_SWAP" == "1" ]]; then
            args+=(-p "MemorySwapMax=0")
        fi
        if [[ -n "$SYSTEMD_CPU_QUOTA" ]]; then
            args+=(-p "CPUQuota=$SYSTEMD_CPU_QUOTA")
        fi
        systemd-run "${args[@]}" "$@"
    elif [[ -n "$SYSTEMD_MEMORY_MAX" && "$ALLOW_UNBOUNDED_SEARCH" != "1" ]]; then
        echo "Refusing to run unbounded cost-accounting search command without systemd-run; set ALLOW_UNBOUNDED_SEARCH=1 to override." >&2
        return 125
    else
        "$@"
    fi
}

write_run_metadata() {
    mkdir -p "$SEARCH_OUTPUT_DIR"
    {
        printf '{\n'
        printf '  "tier": "%s",\n' "$SEARCH_TIER"
        printf '  "families": "%s",\n' "$SEARCH_FAMILIES"
        printf '  "profile": "%s",\n' "${SEARCH_PROFILE:-auto}"
        printf '  "sage_objectives": "%s",\n' "$SAGE_OBJECTIVES"
        printf '  "search_rss_limit": "%s",\n' "$SEARCH_RSS_LIMIT"
        printf '  "allow_unbounded_search": "%s",\n' "$ALLOW_UNBOUNDED_SEARCH"
        printf '  "tlc_max_heap": "%s",\n' "$TLC_MAX_HEAP"
        printf '  "fuzz_runs": %s,\n' "$FUZZ_RUNS"
        printf '  "formal_repo": "%s",\n' "$FORMAL_REPO"
        printf '  "kani_timeout_seconds": %s,\n' "$KANI_TIMEOUT_SECONDS"
        printf '  "run_coverage": "%s",\n' "${RUN_COVERAGE:-0}"
        printf '  "run_mutants": "%s",\n' "${RUN_MUTANTS:-0}"
        printf '  "run_miri": "%s",\n' "${RUN_MIRI:-0}"
        printf '  "run_deny": "%s",\n' "${RUN_DENY:-0}"
        printf '  "run_apalache": "%s",\n' "${RUN_APALACHE:-0}"
        printf '  "run_rocq": "%s",\n' "${RUN_ROCQ:-0}"
        printf '  "run_tla": "%s"\n' "${RUN_TLA:-0}"
        printf '}\n'
    } > "$SEARCH_OUTPUT_DIR/run-metadata.json"
}

family_enabled() {
    local family="$1"
    local item
    if [[ "$SEARCH_FAMILIES" == "all" ]]; then
        return 0
    fi
    IFS=',' read -ra requested_families <<< "$SEARCH_FAMILIES"
    for item in "${requested_families[@]}"; do
        if [[ "$item" == "$family" ]]; then
            return 0
        fi
    done
    return 1
}

profile_for_tier() {
    if [[ -n "$SEARCH_PROFILE" ]]; then
        printf '%s\n' "$SEARCH_PROFILE"
        return
    fi
    case "$SEARCH_TIER" in
        frontier)
            printf 'quick\n'
            ;;
        nightly)
            printf 'corpus\n'
            ;;
        exhaustive)
            printf 'deep\n'
            ;;
        *)
            printf 'quick\n'
            ;;
    esac
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

run_sage_models() {
    local mode="$1"
    if ! command -v sage >/dev/null 2>&1; then
        echo "SKIP Sage cost-accounting frontier: sage not found"
        return
    fi
    if [[ ! -d "$FORMAL_REPO/formal/sage/cost_accounting" ]]; then
        echo "SKIP Sage cost-accounting frontier: formal repo not found at $FORMAL_REPO"
        return
    fi
    mkdir -p "$SEARCH_OUTPUT_DIR"
    mkdir -p "$DOT_SAGE"
    if [[ "$mode" == "frontier" ]] || family_enabled objective; then
        run_limited sage "$FORMAL_REPO/formal/sage/cost_accounting/objective_frontier_model.sage" -- \
            --json-out "$SEARCH_OUTPUT_DIR/objective-frontier.json"
    fi
    if [[ "$mode" == "all" ]]; then
        if family_enabled budget; then
            run_limited sage "$FORMAL_REPO/formal/sage/cost_accounting/budget_admission_model.sage" -- \
                --json-out "$SEARCH_OUTPUT_DIR/budget-admission.json"
        fi
        if family_enabled producer; then
            run_limited sage "$FORMAL_REPO/formal/sage/cost_accounting/producer_routing_model.sage" -- \
                --json-out "$SEARCH_OUTPUT_DIR/producer-routing.json"
        fi
        if family_enabled concurrency; then
            run_limited sage "$FORMAL_REPO/formal/sage/cost_accounting/concurrency_schedule_model.sage" -- \
                --json-out "$SEARCH_OUTPUT_DIR/concurrency-schedule.json"
        fi
        if family_enabled settlement; then
            run_limited sage "$FORMAL_REPO/formal/sage/cost_accounting/settlement_model.sage" -- \
                --json-out "$SEARCH_OUTPUT_DIR/settlement.json"
        fi
        if family_enabled replay; then
            run_limited sage "$FORMAL_REPO/formal/sage/cost_accounting/replay_auth_model.sage" -- \
                --json-out "$SEARCH_OUTPUT_DIR/replay-auth.json"
        fi
        if family_enabled slashing; then
            run_limited sage "$FORMAL_REPO/formal/sage/cost_accounting/slashing_composition_model.sage" -- \
                --json-out "$SEARCH_OUTPUT_DIR/slashing-composition.json"
        fi
        if family_enabled resource; then
            run_limited sage "$FORMAL_REPO/formal/sage/cost_accounting/resource_exhaustion_model.sage" -- \
                --json-out "$SEARCH_OUTPUT_DIR/resource-exhaustion.json"
        fi
    fi
}

run_hypothesis_sage_replay() {
    local profile="$1"
    local mode="$2"
    if ! command -v sage >/dev/null 2>&1; then
        echo "SKIP Sage/Hypothesis cost-accounting frontier: sage not found"
        return
    fi
    if [[ ! -f "$FORMAL_REPO/formal/sage/cost_accounting/hypothesis_search/hypothesis_scenario_search.sage" ]]; then
        echo "SKIP Sage/Hypothesis cost-accounting frontier: search script not found at $FORMAL_REPO"
        return
    fi
    mkdir -p "$SEARCH_OUTPUT_DIR"
    mkdir -p "$DOT_SAGE"
    local json_out="$SEARCH_OUTPUT_DIR/hypothesis-${profile}-${mode}.json"
    local fixture_out="$SEARCH_OUTPUT_DIR/hypothesis-${profile}-${mode}-fixtures.json"
    local coverage_out="$SEARCH_OUTPUT_DIR/hypothesis-${profile}-${mode}-coverage.json"
    local rust_fixtures_out="$SEARCH_OUTPUT_DIR/hypothesis-${profile}-${mode}-rust-fixtures.json"
    run_limited env DOT_SAGE="$DOT_SAGE" sage "$FORMAL_REPO/formal/sage/cost_accounting/hypothesis_search/hypothesis_scenario_search.sage" -- \
        --profile "$profile" \
        --search-mode "$mode" \
        --objectives "$SAGE_OBJECTIVES" \
        --json-out "$json_out" \
        --fixture-out "$fixture_out" \
        --coverage-out "$coverage_out" \
        --rust-fixtures-out "$rust_fixtures_out"
    COST_ACCOUNTING_FRONTIER_FIXTURES_JSON="$rust_fixtures_out" \
        cargo nextest run -p rholang accounting::cost_accounting_frontier::generated_frontier_replay_fixtures_hold
}

run_horizon_v2_sage_replay() {
    local profile="$1"
    local mode="$2"
    if ! command -v sage >/dev/null 2>&1; then
        echo "SKIP Sage/Hypothesis cost-accounting v2 frontier: sage not found"
        return
    fi
    if [[ ! -f "$FORMAL_REPO/formal/sage/cost_accounting/hypothesis_search/horizon_v2_search.sage" ]]; then
        echo "SKIP Sage/Hypothesis cost-accounting v2 frontier: search script not found at $FORMAL_REPO"
        return
    fi
    mkdir -p "$SEARCH_OUTPUT_DIR"
    mkdir -p "$DOT_SAGE"
    local json_out="$SEARCH_OUTPUT_DIR/horizon-v2-${profile}-${mode}.json"
    local fixture_out="$SEARCH_OUTPUT_DIR/horizon-v2-${profile}-${mode}-fixtures.json"
    local coverage_out="$SEARCH_OUTPUT_DIR/horizon-v2-${profile}-${mode}-coverage.json"
    local rust_fixtures_out="$SEARCH_OUTPUT_DIR/horizon-v2-${profile}-${mode}-rust-fixtures.json"
    run_limited env DOT_SAGE="$DOT_SAGE" sage "$FORMAL_REPO/formal/sage/cost_accounting/hypothesis_search/horizon_v2_search.sage" -- \
        --profile "$profile" \
        --search-mode "$mode" \
        --objectives "$SAGE_OBJECTIVES" \
        --source-root "$PWD/rholang/examples" \
        --source-root "$PWD/casper/tests/resources" \
        --json-out "$json_out" \
        --fixture-out "$fixture_out" \
        --coverage-out "$coverage_out" \
        --rust-fixtures-out "$rust_fixtures_out"
    COST_ACCOUNTING_FRONTIER_FIXTURES_JSON="$rust_fixtures_out" \
        cargo nextest run -p rholang accounting::cost_accounting_frontier::generated_frontier_differential_fixtures_hold
}

run_horizon_v3_stateful_replay() {
    local profile="$1"
    local mode="$2"
    if ! command -v sage >/dev/null 2>&1; then
        echo "SKIP Sage/Hypothesis cost-accounting v3 frontier: sage not found"
        return
    fi
    if [[ ! -f "$FORMAL_REPO/formal/sage/cost_accounting/hypothesis_search/horizon_v3_stateful_search.sage" ]]; then
        echo "SKIP Sage/Hypothesis cost-accounting v3 frontier: search script not found at $FORMAL_REPO"
        return
    fi
    mkdir -p "$SEARCH_OUTPUT_DIR"
    mkdir -p "$DOT_SAGE"
    local json_out="$SEARCH_OUTPUT_DIR/horizon-v3-${profile}-${mode}.json"
    local fixture_out="$SEARCH_OUTPUT_DIR/horizon-v3-${profile}-${mode}-fixtures.json"
    local coverage_out="$SEARCH_OUTPUT_DIR/horizon-v3-${profile}-${mode}-coverage.json"
    local rust_fixtures_out="$SEARCH_OUTPUT_DIR/horizon-v3-${profile}-${mode}-rust-fixtures.json"
    run_limited env DOT_SAGE="$DOT_SAGE" sage "$FORMAL_REPO/formal/sage/cost_accounting/hypothesis_search/horizon_v3_stateful_search.sage" -- \
        --profile "$profile" \
        --search-mode "$mode" \
        --objectives "$SAGE_OBJECTIVES" \
        --source-root "$PWD/rholang/examples" \
        --source-root "$PWD/casper/tests/resources" \
        --json-out "$json_out" \
        --fixture-out "$fixture_out" \
        --coverage-out "$coverage_out" \
        --rust-fixtures-out "$rust_fixtures_out"
    COST_ACCOUNTING_FRONTIER_FIXTURES_JSON="$rust_fixtures_out" \
        cargo nextest run -p rholang accounting::cost_accounting_frontier::generated_frontier_stateful_campaign_fixtures_hold
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

run_coverage() {
    if [[ "${RUN_COVERAGE:-0}" != "1" ]]; then
        return
    fi
    if ! cargo llvm-cov --version >/dev/null 2>&1; then
        echo "SKIP coverage: install cargo-llvm-cov"
        return
    fi
    mkdir -p "$SEARCH_OUTPUT_DIR"
    run_limited cargo llvm-cov --json --summary-only --output-path "$SEARCH_OUTPUT_DIR/cost-accounting-coverage-summary.json" -p rholang -- accounting::
}

run_mutants() {
    if [[ "${RUN_MUTANTS:-0}" != "1" ]]; then
        return
    fi
    if ! cargo mutants --version >/dev/null 2>&1; then
        echo "SKIP cargo-mutants: install with 'cargo install cargo-mutants'"
        return
    fi
    run_limited cargo mutants -p rholang --no-shuffle --timeout 120 --baseline=skip
}

run_deny() {
    if [[ "${RUN_DENY:-0}" != "1" ]]; then
        return
    fi
    if ! cargo deny --version >/dev/null 2>&1; then
        echo "SKIP cargo-deny: install with 'cargo install cargo-deny'"
        return
    fi
    run_limited cargo deny check
}

run_miri() {
    if [[ "${RUN_MIRI:-0}" != "1" ]]; then
        return
    fi
    if ! cargo miri --version >/dev/null 2>&1; then
        echo "SKIP Miri: install with 'rustup component add miri'"
        return
    fi
    run_limited env XDG_CACHE_HOME="$MIRI_CACHE_HOME" RUSTFLAGS="$MIRI_RUSTFLAGS" \
        cargo miri test -p rholang generated_frontier_metamorphic_fixtures_hold
}

run_apalache() {
    if [[ "${RUN_APALACHE:-0}" != "1" ]]; then
        return
    fi
    if ! command -v apalache-mc >/dev/null 2>&1; then
        echo "SKIP Apalache: apalache-mc not found"
        return
    fi
    run_limited apalache-mc check "$FORMAL_REPO/formal/tlaplus/cost_accounted_rho/MCRuntimeBudgetReplay.tla"
    run_limited apalache-mc check "$FORMAL_REPO/formal/tlaplus/cost_accounted_rho/MCCostAccountingThreats.tla"
}

run_tla() {
    if [[ "${RUN_TLA:-0}" != "1" ]]; then
        return
    fi
    local tla_tools="${TLA2TOOLS:-/usr/share/java/tla2tools.jar}"
    if [[ ! -f "$tla_tools" ]]; then
        echo "SKIP TLA+: set TLA2TOOLS to tla2tools.jar"
        return
    fi
    (
        cd "$FORMAL_REPO/formal/tlaplus/cost_accounted_rho"
        run_limited java -Xmx"$TLC_MAX_HEAP" -XX:+UseParallelGC -cp "$tla_tools" \
            tlc2.TLC MCRuntimeBudgetReplay.tla -config RuntimeBudgetReplay.cfg -workers auto -nowarning \
            -metadir "$SEARCH_OUTPUT_DIR/tla-runtime-budget-replay"
        run_limited java -Xmx"$TLC_MAX_HEAP" -XX:+UseParallelGC -cp "$tla_tools" \
            tlc2.TLC MCCostAccountingThreats.tla -config CostAccountingThreats.cfg -workers auto -nowarning \
            -metadir "$SEARCH_OUTPUT_DIR/tla-cost-accounting-threats"
        run_limited java -Xmx"$TLC_MAX_HEAP" -XX:+UseParallelGC -cp "$tla_tools" \
            tlc2.TLC MCCostAccountingSearchFrontier.tla -config CostAccountingSearchFrontier.cfg -workers auto -nowarning \
            -metadir "$SEARCH_OUTPUT_DIR/tla-cost-accounting-search-frontier"
    )
}

run_rocq() {
    if [[ "${RUN_ROCQ:-0}" != "1" ]]; then
        return
    fi
    run_limited make -C "$FORMAL_REPO/formal/rocq/cost_accounted_rho"
    (cd "$FORMAL_REPO" && ./scripts/check-cost-accounted-rho-proofs.sh)
}

write_run_metadata
triage_fuzz_artifacts

if ! cargo nextest --version >/dev/null 2>&1; then
    echo "cargo-nextest is required for cost-accounting search smoke tests" >&2
    exit 1
fi

bash scripts/check-cost-accounting-legacy-guard.sh
bash scripts/check-cost-accounting-frontier-guard.sh
cargo nextest run -p rholang accounting::cost_accounting_frontier
cargo nextest run -p rholang --test loom_metering_ownership
cargo nextest run -p rholang --test loom_cost_trace_slots
cargo nextest run -p models refund_amount_is_bounded_by_valid_escrow
cargo nextest run -p models settlement_edge_cases_are_total_and_deterministic

if cargo fuzz --help >/dev/null 2>&1; then
    for target in "${FUZZ_TARGETS[@]}"; do
        run_fuzz_target "${target%%:*}" "${target##*:}"
    done
else
    echo "SKIP cargo-fuzz: install with 'cargo install cargo-fuzz'"
fi

if cargo kani --version >/dev/null 2>&1; then
    for target in "${KANI_TARGETS[@]}"; do
        crate="${target%%:*}"
        harness="${target##*:}"
        run_limited env RUSTFLAGS="$KANI_RUSTFLAGS" timeout "$KANI_TIMEOUT_SECONDS" cargo kani -p "$crate" --harness "$harness"
    done
else
    echo "SKIP Kani: install with 'cargo install kani-verifier && cargo kani setup'"
fi

run_coverage
run_mutants
run_deny
run_miri
run_apalache

case "$SEARCH_TIER" in
    smoke)
        ;;
    frontier)
        run_hypothesis_sage_replay "$(profile_for_tier)" frontier
        run_horizon_v2_sage_replay "$(profile_for_tier)" frontier
        run_horizon_v3_stateful_replay "$(profile_for_tier)" frontier
        run_sage_models frontier
        ;;
    nightly)
        run_hypothesis_sage_replay "$(profile_for_tier)" all
        run_horizon_v2_sage_replay "$(profile_for_tier)" all
        run_horizon_v3_stateful_replay "$(profile_for_tier)" all
        run_sage_models all
        ;;
    exhaustive)
        run_hypothesis_sage_replay "$(profile_for_tier)" all
        run_horizon_v2_sage_replay "$(profile_for_tier)" all
        run_horizon_v3_stateful_replay "$(profile_for_tier)" all
        run_sage_models all
        run_tla
        run_rocq
        ;;
esac

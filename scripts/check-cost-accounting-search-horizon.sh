#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${SEARCH_OUT_DIR:-$ROOT/target/cost-accounting-search-horizon}"
SAGE_DIR="$ROOT/formal/sage/cost_accounting"
TLA_DIR="$ROOT/formal/tlaplus/cost_accounted_rho"
SEARCH_TIER="${SEARCH_TIER:-smoke}"
if [[ -z "${SEARCH_PROFILE+x}" ]]; then
  case "$SEARCH_TIER" in
    smoke|frontier) SEARCH_PROFILE="quick" ;;
    nightly) SEARCH_PROFILE="corpus" ;;
    exhaustive) SEARCH_PROFILE="deep" ;;
    *) SEARCH_PROFILE="quick" ;;
  esac
fi
SEARCH_MODE="${SEARCH_MODE:-frontier}"
SAGE_OBJECTIVES="${SAGE_OBJECTIVES:-all}"
SEARCH_RSS_LIMIT="${SEARCH_RSS_LIMIT:-32G}"
TLC_MAX_HEAP="${TLC_MAX_HEAP:-28g}"
TLC_JAR="${TLC_JAR:-/usr/share/java/tla2tools.jar}"
export DOT_SAGE="${DOT_SAGE:-/tmp/sage}"

mkdir -p "$OUT_DIR"
mkdir -p "$DOT_SAGE"

run() {
  echo "+ $*"
  "$@"
}

memory_limit_bytes() {
  local value="$1"
  local number="${value%[KkMmGg]}"
  local suffix="${value:${#number}}"
  case "$suffix" in
    [Kk]) echo $((number * 1024)) ;;
    [Mm]) echo $((number * 1024 * 1024)) ;;
    [Gg]) echo $((number * 1024 * 1024 * 1024)) ;;
    "") echo "$number" ;;
    *)
      echo "error: unsupported SEARCH_RSS_LIMIT suffix in $value" >&2
      exit 1
      ;;
  esac
}

run_bounded() {
  if [[ "${ALLOW_UNBOUNDED_SEARCH:-0}" == "1" ]]; then
    run "$@"
    return
  fi

  if command -v systemd-run >/dev/null 2>&1 && systemd-run --user --scope true >/dev/null 2>&1; then
    echo "+ systemd-run --user --scope -p MemoryMax=$SEARCH_RSS_LIMIT -p MemorySwapMax=0 $*"
    local systemd_args=(
      --user
      --scope
      -p "MemoryMax=$SEARCH_RSS_LIMIT"
      -p "MemorySwapMax=0"
    )
    if [[ -n "${SYSTEMD_CPU_QUOTA:-}" ]]; then
      systemd_args+=(-p "CPUQuota=$SYSTEMD_CPU_QUOTA")
    fi
    systemd-run \
      "${systemd_args[@]}" \
      "$@"
    return
  fi

  if command -v prlimit >/dev/null 2>&1; then
    local limit_bytes
    limit_bytes="$(memory_limit_bytes "$SEARCH_RSS_LIMIT")"
    echo "+ prlimit --as=$limit_bytes -- $*"
    prlimit --as="$limit_bytes" -- "$@"
    return
  fi

  echo "error: neither systemd-run nor prlimit can enforce SEARCH_RSS_LIMIT=$SEARCH_RSS_LIMIT; set ALLOW_UNBOUNDED_SEARCH=1 to run without a memory envelope" >&2
  exit 1
}

nextest_fixture() {
  local fixture_file="$1"
  local test_name="$2"
  echo "+ COST_ACCOUNTING_FRONTIER_FIXTURES_JSON=$fixture_file cargo nextest run -p rholang $test_name"
  COST_ACCOUNTING_FRONTIER_FIXTURES_JSON="$fixture_file" \
    cargo nextest run -p rholang "$test_name"
}

run_smoke_nextest() {
  run cargo nextest run -p rholang cost_accounting_frontier_generated_fixtures_are_classified
  run cargo nextest run -p rholang generated_frontier_replay_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_metamorphic_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_differential_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_stateful_campaign_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_adversarial_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_property_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_negative_auth_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_source_shape_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_production_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_rholang_eval_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_casper_boundary_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_semantic_eval_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_play_replay_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_phlo_boundary_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_state_root_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_auth_composition_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_generative_semantic_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_semantic_metamorphic_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_external_service_replay_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_coverage_adequacy_holds
  run cargo nextest run -p rholang generated_frontier_corpus_semantic_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_grammar_mutation_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_differential_oracle_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_external_service_matrix_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_casper_security_matrix_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_runtime_trace_interleaving_properties_hold
  run cargo nextest run -p rholang generated_frontier_v9_coverage_adequacy_holds
  run cargo nextest run -p rholang generated_frontier_v10_fuzz_seed_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_v10_lifecycle_trace_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_v10_replay_payload_matrix_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_v10_casper_block_auth_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_v10_parallel_schedule_stress_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_v10_semantic_corpus_mutation_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_v10_coverage_adequacy_holds
  run cargo nextest run -p rholang generated_frontier_v11_source_anchored_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_v11_runtime_budget_source_risks_hold
  run cargo nextest run -p rholang generated_frontier_v11_casper_settlement_slashing_source_risks_hold
  run cargo nextest run -p rholang generated_frontier_v11_coverage_adequacy_holds
  run cargo nextest run -p rholang generated_frontier_v12_production_oracle_fixtures_hold
  run cargo nextest run -p rholang generated_frontier_v12_runtime_metering_parallel_oracles_hold
  run cargo nextest run -p rholang generated_frontier_v12_casper_settlement_slashing_oracles_hold
  run cargo nextest run -p rholang generated_frontier_v12_coverage_adequacy_holds
  run cargo nextest run -p rholang generated_frontier_v13_source_semantic_oracles_hold
  run cargo nextest run -p rholang generated_frontier_v13_runtime_metering_parallel_oracles_hold
  run cargo nextest run -p rholang generated_frontier_v13_casper_settlement_slashing_oracles_hold
  run cargo nextest run -p rholang generated_frontier_v13_coverage_adequacy_holds
  run cargo nextest run -p rholang generated_frontier_v14_source_graph_oracles_hold
  run cargo nextest run -p rholang generated_frontier_v14_slashing_security_oracles_hold
  run cargo nextest run -p rholang generated_frontier_v14_node_security_oracles_hold
  run cargo nextest run -p rholang generated_frontier_v14_coverage_adequacy_holds
  run cargo nextest run -p casper cost_accounting_v12_casper_replay_payload_oracles_hold
  run cargo nextest run -p casper cost_accounting_v12_slashing_replay_oracles_hold
  run cargo nextest run -p casper cost_accounting_v13_source_semantic_replay_payload_oracles_hold
  run cargo nextest run -p casper cost_accounting_v13_settlement_slashing_legacy_oracles_hold
  run cargo nextest run -p casper cost_accounting_v14_replay_slashing_oracles_hold
  run cargo nextest run -p rholang runtime_budget_event_sequence_properties_hold
  run cargo nextest run -p rholang projection_risk
}

run_sage_horizon() {
  command -v sage >/dev/null 2>&1 || {
    echo "error: sage is required for SEARCH_TIER=$SEARCH_TIER" >&2
    exit 1
  }

  local hypothesis="$OUT_DIR/hypothesis-${SEARCH_PROFILE}-${SEARCH_MODE}"
  local horizon_v2="$OUT_DIR/horizon-v2-${SEARCH_PROFILE}-${SEARCH_MODE}"
  local horizon_v3="$OUT_DIR/horizon-v3-${SEARCH_PROFILE}-${SEARCH_MODE}"
  local horizon_v4="$OUT_DIR/horizon-v4-${SEARCH_PROFILE}-${SEARCH_MODE}"
  local horizon_v5="$OUT_DIR/horizon-v5-${SEARCH_PROFILE}-${SEARCH_MODE}"
  local horizon_v6="$OUT_DIR/horizon-v6-${SEARCH_PROFILE}-${SEARCH_MODE}"
  local horizon_v7="$OUT_DIR/horizon-v7-${SEARCH_PROFILE}-${SEARCH_MODE}"
  local horizon_v8="$OUT_DIR/horizon-v8-${SEARCH_PROFILE}-${SEARCH_MODE}"
  local horizon_v9="$OUT_DIR/horizon-v9-${SEARCH_PROFILE}-${SEARCH_MODE}"
  local horizon_v10="$OUT_DIR/horizon-v10-${SEARCH_PROFILE}-${SEARCH_MODE}"
  local horizon_v11="$OUT_DIR/horizon-v11-${SEARCH_PROFILE}-${SEARCH_MODE}"
  local horizon_v12="$OUT_DIR/horizon-v12-${SEARCH_PROFILE}-${SEARCH_MODE}"
  local horizon_v13="$OUT_DIR/horizon-v13-${SEARCH_PROFILE}-${SEARCH_MODE}"
  local horizon_v14="$OUT_DIR/horizon-v14-${SEARCH_PROFILE}-${SEARCH_MODE}"
  local source_surface="$OUT_DIR/source-surface.json"
  local source_root="${SOURCE_ROOT:-$ROOT/rholang/examples}"

  run_bounded sage "$SAGE_DIR/hypothesis_search/hypothesis_scenario_search.sage" -- \
    --profile "$SEARCH_PROFILE" \
    --search-mode "$SEARCH_MODE" \
    --objectives "$SAGE_OBJECTIVES" \
    --json-out "$hypothesis.json" \
    --fixture-out "$hypothesis-fixtures.json" \
    --coverage-out "$hypothesis-coverage.json" \
    --rust-fixtures-out "$hypothesis-rust-fixtures.json"

  run_bounded sage "$SAGE_DIR/hypothesis_search/horizon_v2_search.sage" -- \
    --profile "$SEARCH_PROFILE" \
    --search-mode "$SEARCH_MODE" \
    --objectives "$SAGE_OBJECTIVES" \
    --source-root "$source_root" \
    --json-out "$horizon_v2.json" \
    --fixture-out "$horizon_v2-fixtures.json" \
    --coverage-out "$horizon_v2-coverage.json" \
    --rust-fixtures-out "$horizon_v2-rust-fixtures.json"

  run_bounded sage "$SAGE_DIR/hypothesis_search/horizon_v3_stateful_search.sage" -- \
    --profile "$SEARCH_PROFILE" \
    --search-mode "$SEARCH_MODE" \
    --objectives "$SAGE_OBJECTIVES" \
    --source-root "$source_root" \
    --json-out "$horizon_v3.json" \
    --fixture-out "$horizon_v3-fixtures.json" \
    --coverage-out "$horizon_v3-coverage.json" \
    --rust-fixtures-out "$horizon_v3-rust-fixtures.json"

  run_bounded sage "$SAGE_DIR/hypothesis_search/horizon_v4_adversarial_search.sage" -- \
    --profile "$SEARCH_PROFILE" \
    --search-mode "$SEARCH_MODE" \
    --objectives "$SAGE_OBJECTIVES" \
    --source-root "$source_root" \
    --json-out "$horizon_v4.json" \
    --fixture-out "$horizon_v4-fixtures.json" \
    --coverage-out "$horizon_v4-coverage.json" \
    --rust-fixtures-out "$horizon_v4-rust-fixtures.json"

  run_bounded sage "$SAGE_DIR/hypothesis_search/horizon_v5_property_search.sage" -- \
    --profile "$SEARCH_PROFILE" \
    --search-mode "$SEARCH_MODE" \
    --objectives "$SAGE_OBJECTIVES" \
    --source-root "$source_root" \
    --json-out "$horizon_v5.json" \
    --fixture-out "$horizon_v5-fixtures.json" \
    --coverage-out "$horizon_v5-coverage.json" \
    --rust-fixtures-out "$horizon_v5-rust-fixtures.json"

  run_bounded sage "$SAGE_DIR/hypothesis_search/horizon_v6_production_frontier.sage" -- \
    --profile "$SEARCH_PROFILE" \
    --search-mode "$SEARCH_MODE" \
    --objectives "$SAGE_OBJECTIVES" \
    --source-root "$source_root" \
    --json-out "$horizon_v6.json" \
    --fixture-out "$horizon_v6-fixtures.json" \
    --coverage-out "$horizon_v6-coverage.json" \
    --rust-fixtures-out "$horizon_v6-rust-fixtures.json"

  run_bounded sage "$SAGE_DIR/hypothesis_search/horizon_v7_production_semantic_search.sage" -- \
    --profile "$SEARCH_PROFILE" \
    --search-mode "$SEARCH_MODE" \
    --objectives "$SAGE_OBJECTIVES" \
    --source-root "$source_root" \
    --json-out "$horizon_v7.json" \
    --fixture-out "$horizon_v7-fixtures.json" \
    --coverage-out "$horizon_v7-coverage.json" \
    --rust-fixtures-out "$horizon_v7-rust-fixtures.json"

  run_bounded sage "$SAGE_DIR/hypothesis_search/horizon_v8_generative_semantic_search.sage" -- \
    --profile "$SEARCH_PROFILE" \
    --search-mode "$SEARCH_MODE" \
    --objectives "$SAGE_OBJECTIVES" \
    --source-root "$source_root" \
    --json-out "$horizon_v8.json" \
    --fixture-out "$horizon_v8-fixtures.json" \
    --coverage-out "$horizon_v8-coverage.json" \
    --rust-fixtures-out "$horizon_v8-rust-fixtures.json"

  run_bounded sage "$SAGE_DIR/hypothesis_search/horizon_v9_differential_corpus_security_search.sage" -- \
    --profile "$SEARCH_PROFILE" \
    --search-mode "$SEARCH_MODE" \
    --objectives "$SAGE_OBJECTIVES" \
    --source-root "$source_root" \
    --json-out "$horizon_v9.json" \
    --fixture-out "$horizon_v9-fixtures.json" \
    --coverage-out "$horizon_v9-coverage.json" \
    --rust-fixtures-out "$horizon_v9-rust-fixtures.json"

  run_bounded sage "$SAGE_DIR/hypothesis_search/horizon_v10_hybrid_fuzz_security_search.sage" -- \
    --profile "$SEARCH_PROFILE" \
    --search-mode "$SEARCH_MODE" \
    --objectives "$SAGE_OBJECTIVES" \
    --source-root "$source_root" \
    --json-out "$horizon_v10.json" \
    --fixture-out "$horizon_v10-fixtures.json" \
    --coverage-out "$horizon_v10-coverage.json" \
    --rust-fixtures-out "$horizon_v10-rust-fixtures.json"

  run bash "$ROOT/scripts/cost-accounting-source-surface.sh" --json-out "$source_surface"
  run_bounded sage "$SAGE_DIR/hypothesis_search/horizon_v11_source_anchored_security_search.sage" -- \
    --profile "$SEARCH_PROFILE" \
    --search-mode "$SEARCH_MODE" \
    --objectives "$SAGE_OBJECTIVES" \
    --source-surface-json "$source_surface" \
    --json-out "$horizon_v11.json" \
    --fixture-out "$horizon_v11-fixtures.json" \
    --coverage-out "$horizon_v11-coverage.json" \
    --rust-fixtures-out "$horizon_v11-rust-fixtures.json"

  run_bounded sage "$SAGE_DIR/hypothesis_search/horizon_v12_production_oracle_security_search.sage" -- \
    --profile "$SEARCH_PROFILE" \
    --search-mode "$SEARCH_MODE" \
    --objectives "$SAGE_OBJECTIVES" \
    --source-surface-json "$source_surface" \
    --json-out "$horizon_v12.json" \
    --fixture-out "$horizon_v12-fixtures.json" \
    --coverage-out "$horizon_v12-coverage.json" \
    --rust-fixtures-out "$horizon_v12-rust-fixtures.json"

  run_bounded sage "$SAGE_DIR/hypothesis_search/horizon_v13_source_semantic_security_search.sage" -- \
    --profile "$SEARCH_PROFILE" \
    --search-mode "$SEARCH_MODE" \
    --objectives "$SAGE_OBJECTIVES" \
    --source-surface-json "$source_surface" \
    --json-out "$horizon_v13.json" \
    --fixture-out "$horizon_v13-fixtures.json" \
    --coverage-out "$horizon_v13-coverage.json" \
    --rust-fixtures-out "$horizon_v13-rust-fixtures.json"

  run_bounded sage "$SAGE_DIR/hypothesis_search/horizon_v14_source_graph_security_search.sage" -- \
    --profile "$SEARCH_PROFILE" \
    --search-mode "$SEARCH_MODE" \
    --objectives "$SAGE_OBJECTIVES" \
    --source-surface-json "$source_surface" \
    --json-out "$horizon_v14.json" \
    --fixture-out "$horizon_v14-fixtures.json" \
    --coverage-out "$horizon_v14-coverage.json" \
    --rust-fixtures-out "$horizon_v14-rust-fixtures.json"

  nextest_fixture "$hypothesis-rust-fixtures.json" generated_frontier_replay_fixtures_hold
  nextest_fixture "$hypothesis-rust-fixtures.json" generated_frontier_metamorphic_fixtures_hold
  nextest_fixture "$horizon_v2-rust-fixtures.json" generated_frontier_differential_fixtures_hold
  nextest_fixture "$horizon_v3-rust-fixtures.json" generated_frontier_stateful_campaign_fixtures_hold
  nextest_fixture "$horizon_v4-rust-fixtures.json" generated_frontier_adversarial_fixtures_hold
  nextest_fixture "$horizon_v5-rust-fixtures.json" generated_frontier_property_fixtures_hold
  nextest_fixture "$horizon_v5-rust-fixtures.json" generated_frontier_negative_auth_fixtures_hold
  nextest_fixture "$horizon_v5-rust-fixtures.json" generated_frontier_source_shape_fixtures_hold
  nextest_fixture "$horizon_v6-rust-fixtures.json" generated_frontier_production_fixtures_hold
  nextest_fixture "$horizon_v6-rust-fixtures.json" generated_frontier_rholang_eval_fixtures_hold
  nextest_fixture "$horizon_v6-rust-fixtures.json" generated_frontier_casper_boundary_fixtures_hold
  nextest_fixture "$horizon_v7-rust-fixtures.json" generated_frontier_semantic_eval_fixtures_hold
  nextest_fixture "$horizon_v7-rust-fixtures.json" generated_frontier_play_replay_fixtures_hold
  nextest_fixture "$horizon_v7-rust-fixtures.json" generated_frontier_phlo_boundary_fixtures_hold
  nextest_fixture "$horizon_v7-rust-fixtures.json" generated_frontier_state_root_fixtures_hold
  nextest_fixture "$horizon_v7-rust-fixtures.json" generated_frontier_auth_composition_fixtures_hold
  nextest_fixture "$horizon_v8-rust-fixtures.json" generated_frontier_generative_semantic_fixtures_hold
  nextest_fixture "$horizon_v8-rust-fixtures.json" generated_frontier_semantic_metamorphic_fixtures_hold
  nextest_fixture "$horizon_v8-rust-fixtures.json" generated_frontier_external_service_replay_fixtures_hold
  nextest_fixture "$horizon_v8-rust-fixtures.json" generated_frontier_coverage_adequacy_holds
  nextest_fixture "$horizon_v9-rust-fixtures.json" generated_frontier_corpus_semantic_fixtures_hold
  nextest_fixture "$horizon_v9-rust-fixtures.json" generated_frontier_grammar_mutation_fixtures_hold
  nextest_fixture "$horizon_v9-rust-fixtures.json" generated_frontier_differential_oracle_fixtures_hold
  nextest_fixture "$horizon_v9-rust-fixtures.json" generated_frontier_external_service_matrix_fixtures_hold
  nextest_fixture "$horizon_v9-rust-fixtures.json" generated_frontier_casper_security_matrix_fixtures_hold
  nextest_fixture "$horizon_v9-rust-fixtures.json" generated_frontier_runtime_trace_interleaving_properties_hold
  nextest_fixture "$horizon_v9-rust-fixtures.json" generated_frontier_v9_coverage_adequacy_holds
  nextest_fixture "$horizon_v10-rust-fixtures.json" generated_frontier_v10_fuzz_seed_fixtures_hold
  nextest_fixture "$horizon_v10-rust-fixtures.json" generated_frontier_v10_lifecycle_trace_fixtures_hold
  nextest_fixture "$horizon_v10-rust-fixtures.json" generated_frontier_v10_replay_payload_matrix_fixtures_hold
  nextest_fixture "$horizon_v10-rust-fixtures.json" generated_frontier_v10_casper_block_auth_fixtures_hold
  nextest_fixture "$horizon_v10-rust-fixtures.json" generated_frontier_v10_parallel_schedule_stress_fixtures_hold
  nextest_fixture "$horizon_v10-rust-fixtures.json" generated_frontier_v10_semantic_corpus_mutation_fixtures_hold
  nextest_fixture "$horizon_v10-rust-fixtures.json" generated_frontier_v10_coverage_adequacy_holds
  nextest_fixture "$horizon_v11-rust-fixtures.json" generated_frontier_v11_source_anchored_fixtures_hold
  nextest_fixture "$horizon_v11-rust-fixtures.json" generated_frontier_v11_runtime_budget_source_risks_hold
  nextest_fixture "$horizon_v11-rust-fixtures.json" generated_frontier_v11_casper_settlement_slashing_source_risks_hold
  nextest_fixture "$horizon_v11-rust-fixtures.json" generated_frontier_v11_coverage_adequacy_holds
  nextest_fixture "$horizon_v12-rust-fixtures.json" generated_frontier_v12_production_oracle_fixtures_hold
  nextest_fixture "$horizon_v12-rust-fixtures.json" generated_frontier_v12_runtime_metering_parallel_oracles_hold
  nextest_fixture "$horizon_v12-rust-fixtures.json" generated_frontier_v12_casper_settlement_slashing_oracles_hold
  nextest_fixture "$horizon_v12-rust-fixtures.json" generated_frontier_v12_coverage_adequacy_holds
  nextest_fixture "$horizon_v13-rust-fixtures.json" generated_frontier_v13_source_semantic_oracles_hold
  nextest_fixture "$horizon_v13-rust-fixtures.json" generated_frontier_v13_runtime_metering_parallel_oracles_hold
  nextest_fixture "$horizon_v13-rust-fixtures.json" generated_frontier_v13_casper_settlement_slashing_oracles_hold
  nextest_fixture "$horizon_v13-rust-fixtures.json" generated_frontier_v13_coverage_adequacy_holds
  nextest_fixture "$horizon_v14-rust-fixtures.json" generated_frontier_v14_source_graph_oracles_hold
  nextest_fixture "$horizon_v14-rust-fixtures.json" generated_frontier_v14_slashing_security_oracles_hold
  nextest_fixture "$horizon_v14-rust-fixtures.json" generated_frontier_v14_node_security_oracles_hold
  nextest_fixture "$horizon_v14-rust-fixtures.json" generated_frontier_v14_coverage_adequacy_holds
  run cargo nextest run -p casper cost_accounting_v14_replay_slashing_oracles_hold
  nextest_fixture "$hypothesis-rust-fixtures.json" projection_risk_witnesses_have_guarded_safe_disposition
  nextest_fixture "$horizon_v2-rust-fixtures.json" projection_risk_witnesses_have_guarded_safe_disposition
  nextest_fixture "$horizon_v4-rust-fixtures.json" projection_risk_witnesses_have_guarded_safe_disposition
  nextest_fixture "$horizon_v5-rust-fixtures.json" projection_risk_witnesses_have_guarded_safe_disposition
  nextest_fixture "$horizon_v6-rust-fixtures.json" projection_risk_witnesses_have_guarded_safe_disposition
  nextest_fixture "$horizon_v7-rust-fixtures.json" projection_risk_witnesses_have_guarded_safe_disposition
  nextest_fixture "$horizon_v8-rust-fixtures.json" projection_risk_witnesses_have_guarded_safe_disposition
  nextest_fixture "$horizon_v9-rust-fixtures.json" projection_risk_witnesses_have_guarded_safe_disposition
  nextest_fixture "$horizon_v10-rust-fixtures.json" projection_risk_witnesses_have_guarded_safe_disposition
  nextest_fixture "$horizon_v11-rust-fixtures.json" projection_risk_witnesses_have_guarded_safe_disposition
  nextest_fixture "$horizon_v12-rust-fixtures.json" projection_risk_witnesses_have_guarded_safe_disposition
  nextest_fixture "$horizon_v13-rust-fixtures.json" projection_risk_witnesses_have_guarded_safe_disposition
  nextest_fixture "$horizon_v14-rust-fixtures.json" projection_risk_witnesses_have_guarded_safe_disposition
}

run_tlc_model() {
  local config="$1"
  local module="$2"
  local metadir="$OUT_DIR/tlc-$module"
  mkdir -p "$metadir"
  (
    cd "$TLA_DIR"
    run_bounded java "-Xmx$TLC_MAX_HEAP" -cp "$TLC_JAR" tlc2.TLC \
      -metadir "$metadir" \
      -config "$config" \
      "$module"
  )
}

run_tla() {
  [[ -f "$TLC_JAR" ]] || {
    echo "error: TLC_JAR does not exist: $TLC_JAR" >&2
    exit 1
  }
  run_tlc_model CostAccountedRho MC
  run_tlc_model CompoundProtocol MCCompound
  run_tlc_model EvalScheduling MCEval
  run_tlc_model FullProtocol MCFull
  run_tlc_model RuntimeBudgetReplay MCRuntimeBudgetReplay
  run_tlc_model CostAccountingThreats MCCostAccountingThreats
  run_tlc_model CostAccountingSearchFrontier MCCostAccountingSearchFrontier
  run_tlc_model MergeableChannelAccounting MCMergeableChannelAccounting
}

run_optional_deep_controls() {
  if [[ "${RUN_COVERAGE:-0}" == "1" ]]; then
    if command -v cargo-llvm-cov >/dev/null 2>&1; then
      run_bounded cargo llvm-cov nextest -p rholang --json --output-path "$OUT_DIR/llvm-cov-rholang.json"
    else
      echo "warning: RUN_COVERAGE=1 requested but cargo-llvm-cov is not installed" >&2
    fi
  fi

  if [[ "${RUN_MUTANTS:-0}" == "1" ]]; then
    if command -v cargo-mutants >/dev/null 2>&1; then
      run_bounded cargo mutants -p rholang --timeout "${MUTANTS_TIMEOUT:-300}" --output "$OUT_DIR/cargo-mutants-rholang"
    else
      echo "warning: RUN_MUTANTS=1 requested but cargo-mutants is not installed" >&2
    fi
  fi

  if [[ "${RUN_MIRI:-0}" == "1" ]]; then
    if cargo miri --version >/dev/null 2>&1; then
      run_bounded cargo miri test -p rholang generated_frontier_semantic_metamorphic_fixtures_hold
    else
      echo "warning: RUN_MIRI=1 requested but cargo miri is not installed" >&2
    fi
  fi

  if [[ "${RUN_FUZZ:-0}" == "1" ]]; then
    if cargo fuzz --help >/dev/null 2>&1 && [[ -d "$ROOT/fuzz/fuzz_targets" ]]; then
      local fuzz_seconds="${FUZZ_SECONDS:-60}"
      local fuzz_rss_mb="${FUZZ_RSS_MB:-28672}"
      local fuzz_targets="${FUZZ_TARGETS:-runtime_budget_admission replay_payload_cost_fields cost_accounting_lifecycle_trace}"
      for target in $fuzz_targets; do
        run_bounded cargo fuzz run "$target" -- -max_total_time="$fuzz_seconds" -rss_limit_mb="$fuzz_rss_mb"
      done
    else
      echo "warning: RUN_FUZZ=1 requested but cargo-fuzz or fuzz targets are unavailable" >&2
    fi
  fi

  if [[ "${RUN_KANI:-0}" == "1" ]]; then
    if cargo kani --version >/dev/null 2>&1; then
      local kani_harnesses="${KANI_HARNESSES:-kani_runtime_budget_conservation kani_invalid_admission_no_mutation kani_oop_single_boundary}"
      for harness in $kani_harnesses; do
        run_bounded cargo kani -p rholang --harness "$harness"
      done
    else
      echo "warning: RUN_KANI=1 requested but cargo-kani is not installed" >&2
    fi
  fi

  if [[ "${RUN_DENY:-0}" == "1" ]]; then
    if command -v cargo-deny >/dev/null 2>&1; then
      run_bounded cargo deny check
    else
      echo "warning: RUN_DENY=1 requested but cargo-deny is not installed" >&2
    fi
  fi

  if [[ "${RUN_APALACHE:-0}" == "1" ]]; then
    if command -v apalache-mc >/dev/null 2>&1; then
      (
        cd "$TLA_DIR"
        run_bounded apalache-mc check --config=CostAccountingSearchFrontier.cfg CostAccountingSearchFrontier.tla
      )
    else
      echo "warning: RUN_APALACHE=1 requested but apalache-mc is not installed" >&2
    fi
  fi
}

case "$SEARCH_TIER" in
  smoke)
    run_smoke_nextest
    ;;
  frontier)
    run_smoke_nextest
    run_sage_horizon
    ;;
  nightly)
    run_smoke_nextest
    run_sage_horizon
    ;;
  exhaustive)
    run_smoke_nextest
    run_sage_horizon
    if [[ "${RUN_MODEL_CHECKERS:-0}" == "1" ]]; then
      RUN_ROCQ=1
      RUN_TLA=1
    fi
    [[ "${RUN_ROCQ:-0}" == "1" ]] && run "$ROOT/scripts/check-cost-accounted-rho-proofs.sh"
    [[ "${RUN_TLA:-0}" == "1" ]] && run_tla
    run_optional_deep_controls
    ;;
  *)
    echo "error: unknown SEARCH_TIER=$SEARCH_TIER" >&2
    exit 1
    ;;
esac

echo "cost-accounting search horizon checks completed: $OUT_DIR"

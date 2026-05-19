# Cost-Accounting Sage Models

These models provide exact finite search and objective-frontier ranking
for the cost-accounting search horizon. They are not proof authority by
themselves; generated witnesses must be classified and traced into the
production Rust path before they motivate implementation changes.

Run the representative models:

```sh
sage formal/sage/cost_accounting/budget_admission_model.sage -- --json-out /tmp/budget.json
sage formal/sage/cost_accounting/producer_routing_model.sage -- --json-out /tmp/producer.json
sage formal/sage/cost_accounting/concurrency_schedule_model.sage -- --json-out /tmp/concurrency.json
sage formal/sage/cost_accounting/settlement_model.sage -- --json-out /tmp/settlement.json
sage formal/sage/cost_accounting/replay_auth_model.sage -- --json-out /tmp/replay.json
sage formal/sage/cost_accounting/slashing_composition_model.sage -- --json-out /tmp/slashing.json
sage formal/sage/cost_accounting/resource_exhaustion_model.sage -- --json-out /tmp/resources.json
sage formal/sage/cost_accounting/objective_frontier_model.sage -- --json-out /tmp/frontier.json
sage formal/sage/cost_accounting/hypothesis_search/hypothesis_scenario_search.sage -- --profile quick --search-mode frontier --objectives all --json-out /tmp/hypothesis.json --fixture-out /tmp/hypothesis-fixtures.json --coverage-out /tmp/hypothesis-coverage.json --rust-fixtures-out /tmp/hypothesis-rust-fixtures.json
sage formal/sage/cost_accounting/hypothesis_search/horizon_v2_search.sage -- --profile quick --search-mode frontier --objectives all --source-root rholang/examples --json-out /tmp/horizon-v2.json --fixture-out /tmp/horizon-v2-fixtures.json --coverage-out /tmp/horizon-v2-coverage.json --rust-fixtures-out /tmp/horizon-v2-rust-fixtures.json
sage formal/sage/cost_accounting/hypothesis_search/horizon_v3_stateful_search.sage -- --profile quick --search-mode frontier --objectives all --source-root rholang/examples --json-out /tmp/horizon-v3.json --fixture-out /tmp/horizon-v3-fixtures.json --coverage-out /tmp/horizon-v3-coverage.json --rust-fixtures-out /tmp/horizon-v3-rust-fixtures.json
sage formal/sage/cost_accounting/hypothesis_search/horizon_v4_adversarial_search.sage -- --profile quick --search-mode frontier --objectives all --source-root rholang/examples --json-out /tmp/horizon-v4.json --fixture-out /tmp/horizon-v4-fixtures.json --coverage-out /tmp/horizon-v4-coverage.json --rust-fixtures-out /tmp/horizon-v4-rust-fixtures.json
sage formal/sage/cost_accounting/hypothesis_search/horizon_v5_property_search.sage -- --profile quick --search-mode frontier --objectives all --source-root rholang/examples --json-out /tmp/horizon-v5.json --fixture-out /tmp/horizon-v5-fixtures.json --coverage-out /tmp/horizon-v5-coverage.json --rust-fixtures-out /tmp/horizon-v5-rust-fixtures.json
sage formal/sage/cost_accounting/hypothesis_search/horizon_v6_production_frontier.sage -- --profile quick --search-mode frontier --objectives all --source-root rholang/examples --json-out /tmp/horizon-v6.json --fixture-out /tmp/horizon-v6-fixtures.json --coverage-out /tmp/horizon-v6-coverage.json --rust-fixtures-out /tmp/horizon-v6-rust-fixtures.json
sage formal/sage/cost_accounting/hypothesis_search/horizon_v7_production_semantic_search.sage -- --profile quick --search-mode frontier --objectives all --source-root rholang/examples --json-out /tmp/horizon-v7.json --fixture-out /tmp/horizon-v7-fixtures.json --coverage-out /tmp/horizon-v7-coverage.json --rust-fixtures-out /tmp/horizon-v7-rust-fixtures.json
sage formal/sage/cost_accounting/hypothesis_search/horizon_v8_generative_semantic_search.sage -- --profile quick --search-mode frontier --objectives all --source-root rholang/examples --json-out /tmp/horizon-v8.json --fixture-out /tmp/horizon-v8-fixtures.json --coverage-out /tmp/horizon-v8-coverage.json --rust-fixtures-out /tmp/horizon-v8-rust-fixtures.json
sage formal/sage/cost_accounting/hypothesis_search/horizon_v9_differential_corpus_security_search.sage -- --profile quick --search-mode frontier --objectives all --source-root rholang/examples --json-out /tmp/horizon-v9.json --fixture-out /tmp/horizon-v9-fixtures.json --coverage-out /tmp/horizon-v9-coverage.json --rust-fixtures-out /tmp/horizon-v9-rust-fixtures.json
sage formal/sage/cost_accounting/hypothesis_search/horizon_v10_hybrid_fuzz_security_search.sage -- --profile quick --search-mode frontier --objectives all --source-root rholang/examples --json-out /tmp/horizon-v10.json --fixture-out /tmp/horizon-v10-fixtures.json --coverage-out /tmp/horizon-v10-coverage.json --rust-fixtures-out /tmp/horizon-v10-rust-fixtures.json
bash scripts/cost-accounting-source-surface.sh --json-out /tmp/source-surface.json
sage formal/sage/cost_accounting/hypothesis_search/horizon_v11_source_anchored_security_search.sage -- --profile quick --search-mode frontier --objectives all --source-surface-json /tmp/source-surface.json --json-out /tmp/horizon-v11.json --fixture-out /tmp/horizon-v11-fixtures.json --coverage-out /tmp/horizon-v11-coverage.json --rust-fixtures-out /tmp/horizon-v11-rust-fixtures.json
sage formal/sage/cost_accounting/hypothesis_search/horizon_v12_production_oracle_security_search.sage -- --profile quick --search-mode frontier --objectives all --source-surface-json /tmp/source-surface.json --json-out /tmp/horizon-v12.json --fixture-out /tmp/horizon-v12-fixtures.json --coverage-out /tmp/horizon-v12-coverage.json --rust-fixtures-out /tmp/horizon-v12-rust-fixtures.json
sage formal/sage/cost_accounting/hypothesis_search/horizon_v13_source_semantic_security_search.sage -- --profile quick --search-mode frontier --objectives all --source-surface-json /tmp/source-surface.json --json-out /tmp/horizon-v13.json --fixture-out /tmp/horizon-v13-fixtures.json --coverage-out /tmp/horizon-v13-coverage.json --rust-fixtures-out /tmp/horizon-v13-rust-fixtures.json
sage formal/sage/cost_accounting/hypothesis_search/horizon_v14_source_graph_security_search.sage -- --profile quick --search-mode frontier --objectives all --source-surface-json /tmp/source-surface.json --json-out /tmp/horizon-v14.json --fixture-out /tmp/horizon-v14-fixtures.json --coverage-out /tmp/horizon-v14-coverage.json --rust-fixtures-out /tmp/horizon-v14-rust-fixtures.json
sage formal/sage/cost_accounting/scenario_search/corpus_generator.sage -- --json-out /tmp/corpus.json --fixture-out /tmp/corpus-fixtures.json
```

The JSON records use the same classification vocabulary as
`docs/theory/cost-accounting-search-horizon.md`. Each record also carries
its threat family, expected invariants, Rust reproducer metadata, and
promotion target so generated findings can be triaged without guessing.

The Hypothesis search layer emits four artifacts: raw records, minimized
fixtures, coverage summaries, and Rust replay fixtures. Its witnesses are
still subject to the same witness-to-source rule: a generated counterexample
is not a implementation bug until it reproduces against production Rust or violates a
production-path invariant. Projection-risk records are not reclassified by
passing integration tests; generated fixtures instead carry
`production_disposition = guarded_safe` and concrete production guard names.

The v2 horizon search extends the first Hypothesis layer with cross-product
scenarios, source-derived Rholang seeds, explicit exploit campaigns, and
differential Rust replay fixtures. It is intended to find concrete Rust bugs,
formal proof gaps, and security vulnerabilities without treating model-only
witnesses as implementation defects.

The v3 stateful horizon adds minimized operation campaigns with explicit
production-path and oracle metadata. It composes runtime budgeting, replay
authentication, settlement, slashing, source corpus descriptors, and resource
bounds in one search layer, then replays generated fixtures through Rust
before any witness is promoted.

The v4 adversarial horizon searches budget-boundary, replay-authentication,
settlement, slashing, lifecycle, and source-corpus attacks as terminally
classified witnesses. The v5 property/security horizon then mines named
candidate invariants, negative replay-authentication mutations, source-shape
seeds, cross-deploy separation, scheduler interleavings, settlement/slashing
cache composition, and cache/resource bounds. V5 Rust fixtures are consumed by
`generated_frontier_property_fixtures_hold`,
`generated_frontier_negative_auth_fixtures_hold`, and
`generated_frontier_source_shape_fixtures_hold`.

The v6 production frontier compares generated witnesses with real Rust
production surfaces before promotion. It covers RuntimeBudget projection,
Rholang source evaluation, replay downgrade authentication, Casper settlement,
slashing evidence, scheduler finalization, and invalid-admission boundaries.
V6 Rust fixtures are consumed by `generated_frontier_production_fixtures_hold`,
`generated_frontier_rholang_eval_fixtures_hold`, and
`generated_frontier_casper_boundary_fixtures_hold`.

The v7 production semantic frontier deepens v6 by executing non-Nil Rholang
sources through production evaluation, production play/replay, finite-phlo and
error boundaries, state-root replay evidence, and replay-authentication
composition. V7 Rust fixtures are consumed by
`generated_frontier_semantic_eval_fixtures_hold`,
`generated_frontier_play_replay_fixtures_hold`,
`generated_frontier_phlo_boundary_fixtures_hold`,
`generated_frontier_state_root_fixtures_hold`, and
`generated_frontier_auth_composition_fixtures_hold`.

The v8 generative semantic frontier broadens v7 with bounded grammar-family
generation, semantic metamorphic variants, mocked external-service replay,
auth/settlement/slashing cross-products, and RuntimeBudget event-sequence
property checks. V8 Rust fixtures are consumed by
`generated_frontier_generative_semantic_fixtures_hold`,
`generated_frontier_semantic_metamorphic_fixtures_hold`,
`generated_frontier_external_service_replay_fixtures_hold`,
`generated_frontier_coverage_adequacy_holds`, and
`runtime_budget_event_sequence_properties_hold`.

The v9 differential corpus/security frontier broadens v8 with source-corpus
semantic replay, grammar mutation checks, production differential oracles,
GPT/DALL-E/TTS/gRPC external-service matrix cases, Casper authenticated
payload and settlement/slashing security axes, runtime trace interleaving
properties, and a dedicated adequacy gate. V9 Rust fixtures are consumed by
`generated_frontier_corpus_semantic_fixtures_hold`,
`generated_frontier_grammar_mutation_fixtures_hold`,
`generated_frontier_differential_oracle_fixtures_hold`,
`generated_frontier_external_service_matrix_fixtures_hold`,
`generated_frontier_casper_security_matrix_fixtures_hold`,
`generated_frontier_runtime_trace_interleaving_properties_hold`, and
`generated_frontier_v9_coverage_adequacy_holds`.

The v10 hybrid fuzz/security frontier turns the v9 production replay
surfaces into a promotion discipline for fuzz seeds, Kani-bound witnesses,
lifecycle traces, replay payload mutation matrices, Casper block-auth
composition, mocked external-service error replay, semantic Rholang corpus
mutation, parallel schedule stress, settlement/refund isolation, slashing
isolation, legacy downgrade quarantine, and a dedicated adequacy gate. V10
Rust fixtures are consumed by the `generated_frontier_v10_*` nextest targets.

The v11 source-anchored security frontier first extracts source-surface
metadata from the current `f1r3node-rust` tree, then projects those anchors
into classified fixtures. Runtime budget, metering, parallel evaluation,
Casper replay, settlement, slashing, and legacy-quarantine witnesses must
carry file, symbol, line, cost surface, source risk, reachability, source
presence, replay target, and promotion-gate metadata before promotion. V11
Rust fixtures are consumed by the `generated_frontier_v11_*` nextest targets.

The v12 production-oracle frontier extends v11 by requiring the anchored
witnesses to replay through native production oracles before they become
implementation evidence. RuntimeBudget admission, metering drain order,
parallel digest stability, settlement/refund arithmetic, legacy quarantine,
and Casper/slashing replay-payload hashing are all checked by dedicated
`generated_frontier_v12_*` and `cost_accounting_v12_*` nextest targets.

The v13 source-semantic frontier composes the v11 source anchors and v12
native oracles into named cross-surface semantic obligations. It requires
source facets, source-anchor digests, cross-surface roles, and Rust replay
targets for runtime-to-replay trace commitments, runtime-to-settlement fuel
isolation, metering-to-parallel digest stability, replay-to-slashing
authentication, and legacy-to-runtime quarantine. V13 fixtures are consumed
by `generated_frontier_v13_*` and `cost_accounting_v13_*` nextest targets.

The v14 source-graph security frontier expands v13 from cost-accounting
semantics into current whole-node source surfaces. It binds API ingress,
runtime/replay cost evidence, replay-cache payloads, slashing authorization,
TLS peer certificate handling, private-key debug exposure, and accepted
RustSec policy exceptions to extracted `f1r3node-rust` source anchors. V14
fixtures are consumed by `generated_frontier_v14_*` and
`cost_accounting_v14_*` nextest targets.

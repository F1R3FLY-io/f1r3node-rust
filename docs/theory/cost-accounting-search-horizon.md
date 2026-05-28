# Cost-Accounting Search Horizon

This document defines the defensive search program for cost-accounted
rho calculus. It complements the Rocq and TLA+ proofs, the threat model,
and the Rust implementation. It does not replace proof authority.

The governing rule is:

```text
witness -> source traceability -> classification -> proof/test/doc/implementation action
```

A generated witness is not a Rust vulnerability unless it reproduces on
the production Rust path or contradicts a production-path invariant.

## Search Layers

| Layer | Tooling | Purpose | Output |
|---|---|---|---|
| Exact finite modeling | Sage | Enumerate small budget, trace, replay, settlement, and multi-deploy spaces. | Classified witness candidates. |
| Objective frontier | Sage | Rank witnesses by severity, novelty, replay impact, settlement impact, and concurrency depth. | Pareto frontier for promotion. |
| Explicit model checking | TLC | Exhaust bounded witness-classification and runtime-budget state machines. | Counterexample trace or exhaustion statistics. |
| Property testing | `proptest` / `nextest` | Exercise production-shaped Rust helpers over generated domains. | Minimal failing input or passed sample set. |
| Stateful scenario search | Sage + Hypothesis | Generate and shrink lifecycle, replay, settlement, scheduler, resource, and slashing-composition witnesses. | Raw records, minimized fixtures, coverage summaries, and Rust replay fixtures. |
| Persistent corpus replay | Sage + `nextest` | Re-run promoted witness corpora against production-shaped Rust replay fixtures. | Stable regression evidence for previously classified witnesses. |
| Coverage-guided fuzzing | `cargo-fuzz` | Mutate structured cost traces, replay payloads, settlement terms, and lifecycle traces. | Minimized crash/assertion corpus entries. |
| Symbolic Rust checking | Kani | Prove bounded arithmetic and admission predicates over real Rust source. | Proof success or concrete counterexample. |
| Concurrency interleaving | Loom | Exhaust shadow models for reservation ownership, trace-slot accounting, and finalization. | Interleaving counterexample or proof by exhaustion. |
| Mutation and memory tools | `cargo-mutants`, Miri, coverage, `cargo-deny`, Apalache | Optional deeper runs for adequacy, UB-adjacent checks, dependency risk, and alternate model checking. | Tool-specific reports under the search output directory. |

## Implemented Frontier Hooks

The cost-accounting frontier should be run from this repository:

```sh
SEARCH_TIER=smoke bash scripts/check-cost-accounting-search-horizon.sh
```

The tiers are:

| Tier | Additional behavior |
|---|---|
| `smoke` | Deterministic nextest slices only. |
| `frontier` | Smoke plus Sage objective-frontier JSON generation and quick v2/v3/v4/v5/v6/v7/v8/v9/v10/v11/v12/v13/v14 horizon search. |
| `nightly` | Frontier plus corpus-depth v2/v3/v4/v5/v6/v7/v8/v9/v10/v11/v12/v13/v14 horizon search. |
| `exhaustive` | Nightly plus deep v2/v3/v4/v5/v6/v7/v8/v9/v10/v11/v12/v13/v14 horizon search, Rocq, and TLA+ checks when `RUN_ROCQ=1`, `RUN_TLA=1`, or `RUN_MODEL_CHECKERS=1`. |

The smoke tier is intentionally deterministic and uses `nextest` only; it
does not require Sage, TLC, Kani, fuzzing, or network access. Heavier tiers
generate Sage/Hypothesis corpora and immediately replay the emitted Rust
fixture JSON through the same production-shaped nextest entry points.

`SEARCH_PROFILE` controls the Hypothesis/Sage search depth. When it is
unset, the runner uses `quick` for `frontier`, `corpus` for `nightly`, and
`deep` for `exhaustive`. `SAGE_OBJECTIVES` may be `all` or a comma-separated
subset of `security,concurrency,settlement,replay,resource,metamorphic,slashing,cross_product,source,differential,exploit,stateful,production_path,source_corpus,exploit_cross_product,adversarial,budget,lifecycle,invariant_mining,negative_auth,source_shape,cross_deploy,scheduler,settlement_slashing,cache,production,rholang_eval,semantic_eval,play_replay,error_boundary,state_root,auth_composition,generative_semantic,semantic_metamorphic,external_service_replay,coverage_adequacy,semantic_cross_product,corpus_semantic,grammar_mutation,differential_oracle,external_service_matrix,casper_security_matrix,runtime_trace_interleaving,fuzz_runtime_budget,fuzz_replay_payload,fuzz_lifecycle_trace,fuzz_casper_block_auth,fuzz_external_service_error,fuzz_rholang_corpus_mutation,kani_budget,parallel_schedule_stress,settlement_refund,slashing_isolation,legacy_downgrade,source_anchored,runtime_budget,metering,parallel_eval,casper_replay,legacy_quarantine,production_oracle,source_semantic_oracle,source_graph_security,replay_cache,transport_tls,crypto_key_material,api_ingress,dependency_advisory`.
The generated Rust replay fixtures are immediately replayed by
`generated_frontier_replay_fixtures_hold` and
`generated_frontier_differential_fixtures_hold`. The v3 stateful fixtures
are replayed by `generated_frontier_stateful_campaign_fixtures_hold`; the
v4 adversarial fixtures are replayed by
`generated_frontier_adversarial_fixtures_hold`; the v5 property/security
fixtures are replayed by `generated_frontier_property_fixtures_hold`,
`generated_frontier_negative_auth_fixtures_hold`, and
`generated_frontier_source_shape_fixtures_hold`; the v6 production-frontier
fixtures are replayed by `generated_frontier_production_fixtures_hold`,
`generated_frontier_rholang_eval_fixtures_hold`, and
`generated_frontier_casper_boundary_fixtures_hold`; the v7 production
semantic fixtures are replayed by
`generated_frontier_semantic_eval_fixtures_hold`,
`generated_frontier_play_replay_fixtures_hold`,
`generated_frontier_phlo_boundary_fixtures_hold`,
`generated_frontier_state_root_fixtures_hold`, and
`generated_frontier_auth_composition_fixtures_hold`; the v8 generative
semantic fixtures are replayed by
`generated_frontier_generative_semantic_fixtures_hold`,
`generated_frontier_semantic_metamorphic_fixtures_hold`,
`generated_frontier_external_service_replay_fixtures_hold`,
`generated_frontier_coverage_adequacy_holds`, and
`runtime_budget_event_sequence_properties_hold`; the v9 differential
corpus/security fixtures are replayed by
`generated_frontier_corpus_semantic_fixtures_hold`,
`generated_frontier_grammar_mutation_fixtures_hold`,
`generated_frontier_differential_oracle_fixtures_hold`,
`generated_frontier_external_service_matrix_fixtures_hold`,
`generated_frontier_casper_security_matrix_fixtures_hold`,
`generated_frontier_runtime_trace_interleaving_properties_hold`, and
`generated_frontier_v9_coverage_adequacy_holds`; the v10 hybrid
fuzz/security fixtures are replayed by
`generated_frontier_v10_fuzz_seed_fixtures_hold`,
`generated_frontier_v10_lifecycle_trace_fixtures_hold`,
`generated_frontier_v10_replay_payload_matrix_fixtures_hold`,
`generated_frontier_v10_casper_block_auth_fixtures_hold`,
`generated_frontier_v10_parallel_schedule_stress_fixtures_hold`,
`generated_frontier_v10_semantic_corpus_mutation_fixtures_hold`, and
`generated_frontier_v10_coverage_adequacy_holds`; the v11 source-anchored
fixtures are replayed by `generated_frontier_v11_source_anchored_fixtures_hold`,
`generated_frontier_v11_runtime_budget_source_risks_hold`,
`generated_frontier_v11_casper_settlement_slashing_source_risks_hold`, and
`generated_frontier_v11_coverage_adequacy_holds`; the v12 production-oracle
fixtures are replayed by `generated_frontier_v12_production_oracle_fixtures_hold`,
`generated_frontier_v12_runtime_metering_parallel_oracles_hold`,
`generated_frontier_v12_casper_settlement_slashing_oracles_hold`,
`generated_frontier_v12_coverage_adequacy_holds`, and Casper-native
`cost_accounting_v12_*` replay-payload tests; the v13 source-semantic
fixtures are replayed by `generated_frontier_v13_source_semantic_oracles_hold`,
`generated_frontier_v13_runtime_metering_parallel_oracles_hold`,
`generated_frontier_v13_casper_settlement_slashing_oracles_hold`,
`generated_frontier_v13_coverage_adequacy_holds`, and Casper-native
`cost_accounting_v13_*` replay-payload and settlement/legacy tests; the v14
source-graph security fixtures are replayed by
`generated_frontier_v14_source_graph_oracles_hold`,
`generated_frontier_v14_slashing_security_oracles_hold`,
`generated_frontier_v14_mergeable_channel_oracles_hold`,
`generated_frontier_v14_node_security_oracles_hold`,
`generated_frontier_v14_coverage_adequacy_holds`, and Casper-native
`cost_accounting_v14_replay_slashing_oracles_hold`.

Every heavyweight search command is wrapped by a process memory envelope.
`SEARCH_RSS_LIMIT` defaults to `32G`; TLC uses `TLC_MAX_HEAP=28g` by default
so JVM native overhead stays below the 32GB process envelope. The runner
prefers `systemd-run --user` for RSS enforcement and falls back to `prlimit`
when the user scope bus is unavailable. If neither limiter exists, it refuses
to run bounded search commands unless `ALLOW_UNBOUNDED_SEARCH=1` is explicitly
set.

`SEARCH_FAMILIES` may be set to `all` or a comma-separated subset to
focus the Sage model run:

| Family | Sage model |
|---|---|
| `objective` | `objective_frontier_model.sage` |
| `budget` | `budget_admission_model.sage` |
| `producer` | `producer_routing_model.sage` |
| `concurrency` | `concurrency_schedule_model.sage` |
| `settlement` | `settlement_model.sage` |
| `replay` | `replay_auth_model.sage` |
| `slashing` | `slashing_composition_model.sage` |
| `resource` | `resource_exhaustion_model.sage` |

The stateful search emits:

| Artifact | Meaning |
|---|---|
| `hypothesis-<profile>-<mode>.json` | Raw classified records and Pareto frontier. |
| `hypothesis-<profile>-<mode>-fixtures.json` | Minimized schema fixtures for review and promotion. |
| `hypothesis-<profile>-<mode>-coverage.json` | Feature/class coverage summary. |
| `hypothesis-<profile>-<mode>-rust-fixtures.json` | Production-shaped Rust replay fixtures consumed by nextest. |
| `horizon-v2-<profile>-<mode>.json` | Cross-product, source-seed, differential, and exploit-campaign classified records. |
| `horizon-v2-<profile>-<mode>-rust-fixtures.json` | Production-shaped differential replay fixtures consumed by nextest. |
| `horizon-v3-<profile>-<mode>.json` | Stateful campaign, production-path differential, source-corpus, resource, and exploit cross-product records. |
| `horizon-v3-<profile>-<mode>-rust-fixtures.json` | Production-shaped stateful campaign fixtures consumed by nextest. |
| `horizon-v4-<profile>-<mode>.json` | Adversarial budget, replay, settlement, slashing, lifecycle, and source-corpus records. |
| `horizon-v4-<profile>-<mode>-rust-fixtures.json` | Production-shaped adversarial fixtures consumed by nextest. |
| `horizon-v5-<profile>-<mode>.json` | Property-invariant, negative-authentication, source-shape, cross-deploy, scheduler, settlement/slashing, cache, and resource records. |
| `horizon-v5-<profile>-<mode>-rust-fixtures.json` | Production-shaped property/security fixtures consumed by nextest. |
| `horizon-v6-<profile>-<mode>.json` | Production differential, Rholang evaluation, replay downgrade, Casper settlement, slashing evidence, scheduler, and resource-boundary records. |
| `horizon-v6-<profile>-<mode>-rust-fixtures.json` | Production-frontier fixtures consumed by nextest. |
| `horizon-v7-<profile>-<mode>.json` | Production semantic evaluation, play/replay, source-corpus, error-boundary, state-root, and auth-composition records. |
| `horizon-v7-<profile>-<mode>-rust-fixtures.json` | Production semantic fixtures consumed by nextest. |
| `horizon-v8-<profile>-<mode>.json` | Generative semantic, metamorphic, mocked external-service replay, auth/settlement/slashing cross-product, and RuntimeBudget boundary records. |
| `horizon-v8-<profile>-<mode>-rust-fixtures.json` | Generative semantic fixtures and adequacy gates consumed by nextest. |
| `horizon-v9-<profile>-<mode>.json` | Corpus semantic, grammar mutation, differential oracle, external-service matrix, Casper security, and runtime trace interleaving records. |
| `horizon-v9-<profile>-<mode>-rust-fixtures.json` | Differential corpus/security fixtures and adequacy gates consumed by nextest. |
| `horizon-v10-<profile>-<mode>.json` | Hybrid fuzz, Kani-bound, parallel stress, settlement, slashing, legacy-quarantine, and coverage-adequacy records. |
| `horizon-v10-<profile>-<mode>-rust-fixtures.json` | Hybrid fuzz/security fixtures and adequacy gates consumed by nextest. |
| `source-surface.json` | Extracted Rust source anchors for runtime budget, metering, parallel evaluation, Casper replay, settlement, slashing authorization, recovered rejected slash current-evidence filtering, typed mergeable channels, and legacy quarantine. |
| `horizon-v11-<profile>-<mode>.json` | Source-anchored cost-surface records classified against the current `f1r3node-rust` source tree. |
| `horizon-v11-<profile>-<mode>-rust-fixtures.json` | Source-anchored fixtures and adequacy gates consumed by nextest. |
| `horizon-v12-<profile>-<mode>.json` | Production-oracle records that bind source anchors to native RuntimeBudget, metering, parallel evaluation, Casper replay, settlement, slashing, and legacy-quarantine oracles. |
| `horizon-v12-<profile>-<mode>-rust-fixtures.json` | Production-oracle fixtures and adequacy gates consumed by rholang and Casper nextest targets. |
| `horizon-v13-<profile>-<mode>.json` | Source-semantic cross-surface records that bind RuntimeBudget, metering, parallel evaluation, Casper replay, settlement, slashing, and legacy-quarantine source facets to named semantic oracles. |
| `horizon-v13-<profile>-<mode>-rust-fixtures.json` | Source-semantic fixtures and adequacy gates consumed by rholang and Casper nextest targets. |
| `horizon-v14-<profile>-<mode>.json` | Source-graph security records binding cost-accounting, slashing authorization, typed mergeable-channel accounting, replay cache, TLS/crypto, API ingress, and dependency advisory surfaces to current source anchors. |
| `horizon-v14-<profile>-<mode>-rust-fixtures.json` | Source-graph security fixtures and adequacy gates consumed by rholang and Casper nextest targets. |

The runner writes operational evidence under
`target/cost-accounting-search-horizon/`. Durable conclusions are
promoted only after Rust traceability classifies the witness. Promoted
Sage/Hypothesis findings are recorded in
`formal/sage/cost_accounting/FINDINGS.md`.

## Classification

| Class | Meaning | Required action |
|---|---|---|
| `confirmed_safe` | The model found an expected protected behavior. | Keep as corroborating evidence if useful. |
| `bisimilar` | Model and production projection agree. | Record or promote as a regression fixture. |
| `projection_risk` | A bounded model-to-code projection could diverge. | Add a guard test and document the boundary. |
| `assumption_counterexample` | A theorem precondition is necessary. | Strengthen the theorem statement or proof document. |
| `proof_or_model_strengthening` | The property is true but underrepresented in mechanized artifacts. | Add Rocq theorem, TLA+ invariant, or Sage/TLA model. |
| `needs_source_audit` | The witness touches production behavior but reproduction is inconclusive. | Audit before changing source. |
| `confirmed_current_bug` | The witness reproduces on production Rust or violates a production invariant. | Fix Rust and add regression. |

No generated witness may remain `unexpected` after triage.

## Search Priorities

The current highest-value expansion points are:

1. Producer routing: zero-capable work must use incremental reservation,
   while standalone billable work must produce positive bounded events.
2. Runtime-budget concurrency: canonical batch permit grants, trace-slot
   reservation, and release must remain linearizable under success, OOP,
   invalid admission, repeated OOP races, and low-phlo fanout attempts.
3. Replay authentication: every replay-relevant cost field mutation must
   be visible to validation or authenticated payload hashing.
4. Settlement isolation: precharge/refund arithmetic must be bounded,
   total on valid inputs, and unable to mutate runtime fuel.
5. Multi-deploy blocks: budgets and traces must remain deploy-local while
   settlement adds independently.
6. Slashing composition: current cost-invalid evidence may authorize
   slashing through the parent pre-state bond boundary without changing
   user cost, runtime fuel, or settlement inputs.
7. Resource exhaustion: event weights, primitive descriptors, source paths,
   trace windows, and generated lifecycle traces must match Rust admission
   bounds and reject before budget or trace mutation.
8. Stateful lifecycle search: finalization, rollback, replay, and settlement
   phases must be ordered so evidence is complete before it becomes
   authenticated.
9. Metamorphic trace search: event permutations that should be canonicalized
   must preserve digest and cost, while duplicates and mutations of each Rust
   cost-trace digest input (`deploy_id`, `source_path`, `redex_id`,
   `local_index`, billable kind, primitive descriptor, and weight) must change
   authenticated evidence.
10. Permit-frontier search: descriptors may be discovered cheaply, but every
   expensive primitive, substitution, RSpace search, hash, serialization, or
   branch spawn must be preceded by a charged permit or deterministic cap.
11. Corpus retention: every promoted generated witness must remain replayable
   as a stable Rust fixture and must keep its terminal classification.
11. Cross-product horizon search: multi-deploy events, replay mutation,
    settlement, slashing evidence, resource bounds, and lifecycle ordering
    must compose without weakening any individual invariant.
12. Source-aware seed search: real Rholang examples and fixtures should seed
    production-shaped replay fixtures so search covers realistic source paths,
    primitive descriptors, and event distributions.
13. Exploit campaigns: replay-cache substitution, refund replenishment,
    slashing/refund confusion, descriptor inflation, and rollback/finalization
    attacks must land in documented classification buckets.
14. Stateful campaign search: minimized operation sequences must carry explicit
    campaign steps, oracle kind, production path, reproducer command, and
    terminal classification before promotion.
15. Production-path differentials: generated witnesses should compare replay
    payloads, block authentication, and settlement arithmetic against Rust
    production helpers, not only shadow models.
16. Source-corpus projection: real Rholang source paths and primitive
    descriptor distributions should continue feeding minimized fixtures as the
    corpus grows.
17. Exploit cross-products: slashing/refund/replay/resource interactions must
    be searched together so composition bugs are not hidden by isolated checks.
    The v3 slashing/refund/replay witness is promoted to `confirmed_safe` only
    because composed Rust tests now authenticate the user cost trace, replay
    event logs, slash evidence, block hash payload, and refund projection in the
    same production-shaped scenario.
18. Adversarial horizon search: budget-boundary, replay-authentication,
    settlement, slashing, lifecycle, and source-corpus attacks must be generated
    as terminally classified witnesses and replayed through
    `generated_frontier_adversarial_fixtures_hold`.
19. Property/security horizon search: candidate invariants, negative replay
    authentication, real source-shape seeds, deploy-domain separation,
    scheduler interleavings, slashing/settlement/cache composition, and resource
    boundaries must be generated as classified fixtures and replayed through the
    v5 property, negative-auth, and source-shape nextest targets.
20. Production-frontier search: generated witnesses must compare against real
    Rust production surfaces: RuntimeBudget projection, Rholang evaluation,
    replay payload authentication, Casper settlement, slashing evidence,
    scheduler finalization, and invalid-admission boundaries. V6 witnesses are
    replayed through the production, Rholang-eval, and Casper-boundary nextest
    targets before promotion.
21. Production semantic search: generated witnesses must execute non-trivial
    Rholang source through production evaluation, compare play/replay cost
    evidence, classify finite-phlo and user-error boundaries, and keep replay
    authentication axes tied to the resulting cost trace. V7 witnesses are
    replayed through semantic-eval, play/replay, phlo-boundary, state-root, and
    auth-composition nextest targets.
22. Generative semantic search: bounded grammar families, semantic
    metamorphic variants, mocked external-service replay, auth/settlement/
    slashing cross-products, and RuntimeBudget event sequences must be searched
    together. V8 witnesses are replayed through the generative semantic,
    metamorphic, external-service replay, adequacy, and property nextest
    targets before promotion.
23. Differential corpus/security search: source-corpus metadata must be tied
    to executable semantic fixtures, grammar rewrites must preserve the stated
    cost-accounting relation, production play/replay must agree on cost trace
    evidence, all mocked external-service classes must be replayed, Casper
    authentication and settlement/slashing axes must remain explicit, and
    multi-deploy trace interleavings must preserve canonical digest stability
    while detecting deploy-domain mutation.
24. Hybrid fuzz/security search: fuzz seed metadata, Kani harness bounds,
    lifecycle traces, replay payload mutations, external-service errors,
    Rholang corpus mutations, parallel schedule stress, settlement/refund,
    slashing isolation, and legacy downgrade quarantine must all retain a
    concrete production replay target and promotion gate before implementation action.
25. Trace identity coverage: promoted Rust replay fixtures carry the concrete
    cost-trace digest field list, and the frontier tests mutate those fields
    through production `RuntimeBudget::reserve_canonical` events before
    treating a search witness as covered.
26. Source-anchored horizon search: source-surface extraction must bind each
    generated cost-accounting witness to the current `f1r3node-rust` file,
    symbol, line, cost surface, source risk, reachability classification, and
    expected-present/expected-absent status before promotion.
27. Production-oracle horizon search: source-anchored witnesses must be
    replayed through native Rust oracles for RuntimeBudget admission/OOP,
    metering drain order, parallel digest stability, Casper replay-payload
    hashing, settlement/refund arithmetic, slashing isolation, and legacy
    quarantine before they are treated as implementation evidence.
28. Source-semantic horizon search: cross-surface source facets, stable source
    anchor digests, and semantic oracle names must be present before promotion
    for runtime-to-replay trace commitments, runtime-to-settlement fuel
    isolation, metering-to-parallel digest stability, replay-to-slashing
    authentication, and legacy-to-runtime quarantine.
29. Source-graph security horizon search: cost-accounting witnesses must be
    aligned with the current Rust graph for API ingress, replay cache payload
    binding, slashing authorization, recovered rejected slash current-evidence
    filtering, typed mergeable-channel accounting, TLS peer identity,
    private-key debug surfaces, and accepted dependency advisories before any
    bug or security claim is promoted.

## Promotion Rules

| Finding status | Required promotion |
|---|---|
| `confirmed_current_bug` | Source fix, deterministic regression, threat/use-case update, and formal artifact update when normative. |
| `projection_risk` | Guard test plus specification text; keep the search classification but require `production_disposition = guarded_safe` unless production reproduces the risk. |
| `assumption_counterexample` | Strengthen theorem preconditions and keep a counterexample fixture. |
| `proof_or_model_strengthening` | Promote to Rocq/TLA+/Sage theorem or invariant and deterministic Rust fixture. |
| `needs_source_audit` | Leave source unchanged until audit reaches a terminal classification. |
| `confirmed_safe` / `bisimilar` | Keep only if it improves regression or coverage evidence. |

Every promoted witness must include a deterministic reproduction command,
the minimized input or trace, the classification, the production source
path checked, and the proof/test/doc target it updates. Projection-risk
witnesses must additionally name concrete production guards; the checked-in
fixture harness fails if a projection-risk witness lacks a guarded-safe
production disposition.

## Optional Deepening Controls

| Variable | Effect |
|---|---|
| `RUN_COVERAGE=1` | Emit an `llvm-cov` JSON summary for cost-accounting tests when installed. |
| `RUN_MUTANTS=1` | Run `cargo-mutants` against the Rust cost-accounting surface when installed. |
| `RUN_MIRI=1` | Run the generated metamorphic replay test under Miri when installed. |
| `RUN_DENY=1` | Run dependency policy checks through `cargo-deny` when installed. |
| `RUN_APALACHE=1` | Cross-check `CostAccountingThreats` and `CostAccountingSearchFrontier` through Apalache when installed. |
| `RUN_FUZZ=1` | Run configured `cargo-fuzz` targets when cargo-fuzz and local fuzz targets are available. |
| `RUN_KANI=1` | Run configured Kani harnesses when cargo-kani is installed. |
| `FUZZ_TARGETS` | Space-separated fuzz targets; defaults to runtime-budget admission, replay payload, and lifecycle traces. |
| `KANI_HARNESSES` | Space-separated Kani harnesses; defaults to budget conservation, invalid admission, and OOP boundary harness names. |
| `FUZZ_SECONDS=60` | Per-target fuzzing time budget for optional fuzz runs. |
| `FUZZ_RSS_MB=28672` | Per-target libFuzzer RSS cap inside the outer search memory envelope. |
| `SEARCH_RSS_LIMIT=32G` | Default hard memory envelope for heavyweight search subprocesses. |
| `TLC_MAX_HEAP=28g` | Default TLC heap inside the 32GB process envelope. |
| `ALLOW_UNBOUNDED_SEARCH=1` | Explicit escape hatch when cgroup limiting is unavailable. |
| `SYSTEMD_CPU_QUOTA` | Optional CPU quota when `systemd-run --user` is available. |

## Option E — Reconciliation Search Anchors

The post-hoc canonical reconciliation introduced for the
`cost_trace_digest` schedule-invariance fix (see TM-CA-144 in the
threat model and Appendix A of the verification doc) adds the
following Sage scenario record to the search frontier:

| Sage record | Scenario | Promotion target |
|-------------|----------|------------------|
| `sage_concurrency_reconciliation_is_schedule_independent` | 2-event OOP race against budget=4 | `rocq:rb_reconcile_consumed_invariant_under_permutation` |

The accompanying coverage anchors:

- Rust: `cost_accounting_spec::concurrent_runtime_budget_reservations_are_linearizable`, `loom_runtime_budget_reconciliation::*`.
- Rocq: `rb_reconcile_consumed_eq_min_initial_or_sum`, `rb_reconcile_consumed_invariant_under_permutation`, `rb_reconcile_oop_iff_sum_overflows`, `rb_reconcile_oop_occurrence_invariant_under_permutation`.
- TLA+: `RuntimeBudgetReplay.ConsumedAndVerdictScheduleIndependent`, `TotalCostMatchesClampedSum`, `ConsumedFollowsReconciliationContract`, `NoCrossWorkerStateMixing`.

The search horizon still has a checked adequacy gate on
`concurrency_schedule` coverage; the Option-E record is admitted on
the same gate as the prior `repeated_oop` and
`finalization_completion` records.

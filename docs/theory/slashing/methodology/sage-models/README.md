# Sage models — index

This directory documents the **14 Sage model families** that make
up the slashing search corpus at
[`formal/sage/slashing/`](../../../../../formal/sage/slashing/). Each
chapter explains *why* the family exists, *what* it searches, *how*
the search is structured, and *what* the typical witness output
looks like.

The framework chapter is
[`../formal-methods/04-finite-modeling-sage.md`](../formal-methods/04-finite-modeling-sage.md);
the chapters below are the per-family deep dives.

## Index

| #  | Family                                                                           | Models                                                                                                                 |
|----|----------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------|
| 01 | [Closure & graph models](./01-closure-and-graph.md)                              | `closure_model`, `weighted_closure_model`, `closure_certificate_model`, `dag_behavior_model`, `graph_edge_cases_model` |
| 02 | [Adversarial & damage models](./02-adversarial-and-damage.md)                    | `adversarial_campaign_model`, `adversarial_timing_game`, `damage_optimizer`, `deep_threat_model`                       |
| 03 | [Arithmetic & projection models](./03-arithmetic-and-projection.md)              | `arithmetic_envelope_model`, `bounded_arithmetic_model`, `implementation_projection_risk_model`                        |
| 04 | [Differential & bisimilarity models](./04-differential-and-bisimilarity.md)      | `differential_bisimilarity_model`, `differential_trace_generator`                                                      |
| 05 | [Epoch & lifecycle models](./05-epoch-and-lifecycle.md)                          | `epoch_lifecycle_model`, `epoch_churn_attack_model`                                                                    |
| 06 | [Evidence visibility & timing models](./06-evidence-visibility-and-timing.md)    | `evidence_propagation_model`, `evidence_timing_attack_search`, `evidence_visibility_model`                             |
| 07 | [Horizon & objective-frontier search](./07-horizon-and-objective-frontier.md)    | `horizon_search_model`, `horizon_v2_search_model`, `objective_frontier_model`                                          |
| 08 | [Hypothesis stateful search](./08-hypothesis-stateful-search.md)                 | `hypothesis_search/hypothesis_scenario_search`                                                                         |
| 09 | [Pipeline & accounting models](./09-pipeline-and-accounting.md)                  | `pipeline_effect_model`, `record_normalization_model`, `slash_order_model`, `validator_boundary_model`                 |
| 10 | [Quorum intersection models](./10-quorum-intersection.md)                        | `quorum_intersection_model`                                                                                            |
| 11 | [Tracker race models](./11-tracker-race.md)                                      | `tracker_race_model`                                                                                                   |
| 12 | [Theorem-assumption counterexamples](./12-theorem-assumption-counterexamples.md) | `theorem_assumption_counterexamples`                                                                                   |
| 13 | [Scenario corpus generation](./13-scenario-corpus-generation.md)                 | `scenario_schema`, `scenario_search/corpus_generator`                                                                  |
| 14 | [Weighted stake optimization](./14-weighted-stake-optimization.md)               | `weighted_stake_optimization`                                                                                          |

## How to read these chapters

Each chapter follows the same template:

1. **Family motivation** — what bug class or property does the family
   defend against?
2. **Models in this family** — one paragraph per model.
3. **Representative witness** — JSON shape and reading guide.
4. **Promotion targets** — which Rocq theorems / TLA⁺ invariants /
   Rust regressions the family's witnesses feed.
5. **Related findings** — pointer to entries in
   [`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md).

# 02 · Adversarial & damage models

## 1 · Family motivation

The adversarial-and-damage family is the methodology's
**objective-guided** search engine for the *worst-case* slashing
behavior. Where the closure family
([01](./01-closure-and-graph.md)) enumerates the *protocol* state
space, this family enumerates the *adversary's* strategy space and
scores each strategy by its damage potential.

The pedagogical framework lives in
[`../attack-modeling/02-adversarial-search.md`](../attack-modeling/02-adversarial-search.md);
this chapter documents the four Sage models that implement it.

## 2 · Models in this family

| Model                                                                                                    | Searches                                                                          |
|----------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------|
| [`adversarial_campaign_model.sage`](../../../../../formal/sage/slashing/adversarial_campaign_model.sage) | Multi-round adversarial campaigns mixing equivocate / neglect / withhold / report |
| [`adversarial_timing_game.sage`](../../../../../formal/sage/slashing/adversarial_timing_game.sage)       | Timing-dimension search: when to release evidence, propose, withhold              |
| [`damage_optimizer.sage`](../../../../../formal/sage/slashing/damage_optimizer.sage)                     | Single-shot damage-objective optimization over graphs and stakes                  |
| [`deep_threat_model.sage`](../../../../../formal/sage/slashing/deep_threat_model.sage)                   | Multi-objective Pareto-frontier sweep across all five adversary objectives        |

The four models share a common scoring interface (the strategy
encoding from
[`scenario_schema.sage`](../../../../../formal/sage/slashing/scenario_schema.sage));
the differences are in which dimensions they search and which
objective they optimize.

## 3 · Representative witness

```json
{
  "kind": "deep_threat_pareto_point",
  "n": 4,
  "stakes": [5, 5, 5, 5],
  "objective_scores": {
    "honest_slashed_stake": 10,
    "quorum_drop": 1,
    "accountability_gap": 2,
    "delay_rounds": 3,
    "damage_ratio": 2.0
  },
  "strategy": {
    "equivocators": [3],
    "neglect_edges": [[0, 3], [1, 3]],
    "visibility": {"0": [3], "1": [3], "2": [], "3": [3]},
    "reports": [],
    "gossip_schedule": "lazy",
    "validator_churn": []
  },
  "is_pareto_optimal": true,
  "dominated_by": [],
  "dominates": [3, 7, 12]
}
```

Reading: a 4-validator scenario where validator 3 equivocates;
validators 0 and 1 (with neglect edges to 3) are themselves slashed,
while validator 2 (which did not see 3's equivocation) is spared.
The strategy is Pareto-optimal — no other searched strategy improves
on it across all five objectives.

## 4 · Promotion targets

| Witness shape                           | Rocq theorem                              | Rust regression                                                                    |
|-----------------------------------------|-------------------------------------------|------------------------------------------------------------------------------------|
| Damage amplification chain              | `two_level_closure_depth_bound` (T-11)    | `prop_t_11_neglect_closure.rs`                                                     |
| Quorum drop under campaign              | `weighted_quorum_intersection`            | `quorum_intersection_after_slash.rs`                                               |
| Accountability gap (visibility/reports) | (per `evidence_visibility_model`)         | `evidence_visibility_gap.rs`, `report_time_closure_shrinkage.rs`                   |
| Delay (rounds to closure)               | `two_level_closure_converges_in_n_rounds` | `frontier_monotonicity_merge_basis.rs`                                             |
| Damage-ratio bound                      | (informal; see threat model §5.A)         | `deep_sage_economic_safety_envelopes.rs`, `deep_sage_stake_damage_optimization.rs` |

## 5 · Related findings

In [`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md):

- **#4** Damage optimizer finds the canonical chain attack.
- **#33–#40** Deep-threat findings (Pareto-frontier points).
- **#41–#48** Adversarial-campaign findings (multi-round attacks).
- **#49–#52** Timing-game findings.

## 6 · Cost note

The deep-threat sweep is the methodology's most expensive Sage
search. CI smoke runs at `budget = 10³` (seconds); nightly runs at
`budget = 10⁶` (hours). Both are documented in
[`scripts/ci/slashing-search-horizon.sh`](../../../../../scripts/ci/slashing-search-horizon.sh).

# 07 · Horizon & objective-frontier search

## 1 · Family motivation

The slashing strategy space is too large for exhaustive enumeration
beyond `n ≈ 5, depth ≈ 4`. **Objective-guided** search (see
[`../attack-modeling/02-adversarial-search.md`](../attack-modeling/02-adversarial-search.md))
extends the reach by sampling proportional to a scoring function;
**horizon search** extends it further by structuring the search as
an iterative *frontier* — at each step, the most promising
strategies expand into their neighbors, the less promising are
pruned.

## 2 · Models in this family

| Model                                                                                                | Searches                                                                                  |
|------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------|
| [`horizon_search_model.sage`](../../../../../formal/sage/slashing/horizon_search_model.sage)         | First-generation horizon search; closure / quorum / accountability objectives             |
| [`horizon_v2_search_model.sage`](../../../../../formal/sage/slashing/horizon_v2_search_model.sage)   | Second-generation horizon search with extended strategy encoding and shrinking            |
| [`objective_frontier_model.sage`](../../../../../formal/sage/slashing/objective_frontier_model.sage) | Multi-objective Pareto-frontier with explicit `bound`, `tier`, and `objective` parameters |

The horizon-search models are the operational engine for the
"Frontier", "Nightly", and "Exhaustive" CI tiers documented in
[`../../slashing-search-horizon.md §4`](../../slashing-search-horizon.md).

## 3 · Representative witness

```json
{
  "kind": "horizon_v2_witness",
  "tier": "nightly",
  "round": 47,
  "frontier_size": 128,
  "objective_scores": {
    "honest_slashed_stake": 14,
    "quorum_drop": 2,
    "accountability_gap": 3,
    "delay_rounds": 5
  },
  "strategy_summary": {
    "n": 5,
    "equivocators": [3, 4],
    "neglect_edges_count": 6,
    "report_rounds": 3
  },
  "is_new_pareto_point": true,
  "displaced_pareto_point": null
}
```

Reading: at round 47 of the nightly horizon-search tier, a new
Pareto-optimal strategy emerged with five validators, two direct
equivocators, six neglect edges, and three report rounds. The new
point did not displace any existing Pareto point (it strictly
extends the frontier).

## 4 · Promotion targets

The horizon search does not have its own dedicated promotion
targets — its output flows into the same theorems and regressions
as the adversarial-and-damage family ([§04](./02-adversarial-and-damage.md)).
The horizon-search-specific artifacts are:

| Witness shape                        | Rust regression                            |
|--------------------------------------|--------------------------------------------|
| Per-round frontier snapshot          | `horizon_search_fixtures.rs`               |
| Per-tier fixture corpus              | `horizon_v2_search_fixtures.rs`            |
| Minimal counterexample from frontier | `minimal_counterexample_catalog_replay.rs` |

## 5 · Related findings

In [`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md):

- **#33–#48** Hypothesis / deep-threat findings are typically
  surfaced first by horizon-search and then minimized via
  Hypothesis.

## 6 · Methodology note

The horizon-search models are the methodology's answer to *how to
scale adversarial search beyond exhaustive enumeration*. The
trade-off is:

| Property                                  | Exhaustive enumeration | Horizon search                |
|-------------------------------------------|------------------------|-------------------------------|
| Guaranteed to find every witness in bound | Yes                    | No                            |
| Tractable for `n > 6`                     | No                     | Yes                           |
| Misses local minima                       | No                     | Possibly                      |
| Cost                                      | Exponential            | Sub-exponential (with tuning) |

The methodology runs **both** — exhaustive on `n ≤ 5`, horizon-
guided on `n ≥ 6` — and treats agreement as corroboration. The
tier parameterization in
[`scripts/ci/slashing-search-horizon.sh`](../../../../../scripts/ci/slashing-search-horizon.sh)
implements the trade-off operationally.

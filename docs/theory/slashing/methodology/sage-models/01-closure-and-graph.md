# 01 · Closure & graph models

## 1 · Family motivation

The slashing subsystem's two-level closure is its core combinatorial
construct: given a set of direct equivocators `E` and a directed graph
of *neglect edges* `N`, the closure is the least set `C` such that
`E ⊆ C` and every validator with a neglect edge to a member of `C` is
itself in `C`. Bugs in the closure manifest as honest validators
being slashed (𝖡ₛ) or as direct offenders escaping slashing through
graph manipulation (𝖡𝖼). This family searches the closure's
behavior exhaustively on small `n`.

## 2 · Models in this family

| Model                                                                                                  | Searches                                                                       |
|--------------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------|
| [`closure_model.sage`](../../../../../formal/sage/slashing/closure_model.sage)                         | Unweighted two-level closure with iterative + transitive-closure cross-check   |
| [`weighted_closure_model.sage`](../../../../../formal/sage/slashing/weighted_closure_model.sage)       | Stake-weighted closure with active-stake quorum checks                         |
| [`closure_certificate_model.sage`](../../../../../formal/sage/slashing/closure_certificate_model.sage) | Fixed-point depth + first-slash-round certificates + shortest neglect paths    |
| [`dag_behavior_model.sage`](../../../../../formal/sage/slashing/dag_behavior_model.sage)               | Block-DAG behavioral cases (forks, merges, orphans)                            |
| [`graph_edge_cases_model.sage`](../../../../../formal/sage/slashing/graph_edge_cases_model.sage)       | Duplicate edges, self-edges, disconnected cycles, cycles into direct offenders |

## 3 · Representative witness

```json
{
  "kind": "two_level_closure_witness",
  "n": 4,
  "equivocators": [3],
  "edges": [[0, 1], [1, 2], [2, 3]],
  "closure": [0, 1, 2, 3],
  "rounds": [[3], [2, 3], [1, 2, 3], [0, 1, 2, 3]],
  "depth": 3,
  "bft_bound": 1,
  "quorum_required": 3,
  "active_after": 0,
  "quorum_violated": true,
  "shortest_neglect_paths": {"0": [0, 1, 2, 3], "1": [1, 2, 3], "2": [2, 3]}
}
```

Reading: `n = 4` validators, validator 3 is the direct offender,
neglect edges form a chain `0 → 1 → 2 → 3`. The closure includes
every validator (chain amplification). The active set after slashing
is empty, violating the quorum requirement. The first-slash-round
certificate confirms the closure converges in 3 rounds (= chain
length).

## 4 · Promotion targets

| Witness shape                         | Rocq theorem                               | TLA⁺ invariant           | Rust regression                                               |
|---------------------------------------|--------------------------------------------|--------------------------|---------------------------------------------------------------|
| Closure depth ≤ `n − 1`               | `two_level_closure_depth_bound` (T-11)     | `Inv_ClosureTermination` | `prop_t_11_neglect_closure.rs`                                |
| BFT bound under `f < n/3`             | `two_level_closure_bft_bound` (T-12)       | `Inv_BFTBound`           | `prop_t_12_quorum_preservation.rs`                            |
| Stake-weighted quorum preservation    | `weighted_quorum_intersection`             | `Inv_QuorumIntersect`    | `quorum_intersection_after_slash.rs`                          |
| Graph edge cases (duplicates, cycles) | (covered by closure characterization)      | (TLC enumerated)         | `duplicate_neglect_edges.rs`, `disconnected_neglect_cycle.rs` |
| Closure certificate path              | `slash_iter_reachability_characterization` | (model-checked)          | `frontier_monotonicity_merge_basis.rs`                        |

## 5 · Related findings

In [`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md):

- **#1** Unweighted closure = reverse reachability.
- **#2** Minimal unweighted quorum drop at `n=4, F=1`.
- **#4** Damage optimization finds chain amplification.
- **#7** Duplicate / self-edge / cycle cases behave as predicted.
- **#11** Closure certificates show first-slash round = shortest path
  distance.

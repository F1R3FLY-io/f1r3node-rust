# 14 · Weighted stake optimization

## 1 · Family motivation

The closure family ([01](./01-closure-and-graph.md)) optimizes
*validator count*; this family optimizes *slashed stake* under a
bounded stake budget. The two are different because validators
with larger bonds are more valuable to slash — both economically
(for the attacker, who burns bond) and consensus-theoretically (for
the system, which loses voting weight).

## 2 · The model

| Model                                                                                                      | Searches                                                                           |
|------------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------|
| [`weighted_stake_optimization.sage`](../../../../../formal/sage/slashing/weighted_stake_optimization.sage) | Stake-weighted attack optimization under a bounded direct-equivocator stake budget |

## 3 · Representative witness

```json
{
  "kind": "weighted_stake_optimization_witness",
  "n": 5,
  "stake_budget": 3,
  "stakes": [1, 1, 5, 5, 1],
  "best_equivocators": [4],
  "best_neglect_edges": [[2, 4], [3, 4]],
  "direct_offender_stake": 1,
  "slashed_total_stake": 11,
  "amplification_factor": 11.0,
  "honest_stake_slashed": 10
}
```

Reading: with a budget of stake 3 for direct equivocators, the
optimal adversary picks the low-stake validator 4 (stake 1) and
arranges neglect edges from the two high-stake validators 2 and 3
(stake 5 each). The chain amplifies the damage by a factor of 11.
**Total slashed honest stake is 10** for an adversarial cost of
**stake 1** — an asymmetric outcome the defender must prevent.

## 4 · Promotion targets

| Witness shape                                            | Defense / theorem                              | Rust regression                          |
|----------------------------------------------------------|------------------------------------------------|------------------------------------------|
| Asymmetric stake-weighted amplification                  | T-12 (BFT bound on stake)                      | `prop_t_12_quorum_preservation.rs`       |
| Low-stake direct offender exploits high-stake neglecters | Defended by `f_stake < n_stake/3` precondition | `deep_sage_stake_damage_optimization.rs` |
| Damage-ratio bound                                       | (informal; see threat model §5.A)              | `deep_sage_economic_safety_envelopes.rs` |

## 5 · Related findings

In [`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md):

- **#4** (cross-ref) Damage optimization finds chain amplification
  in the unweighted case; this family extends to the weighted case.
- **#10** Weighted quorum intersection.

## 6 · Methodology note

This family is the bridge between the **correctness** verification
(unweighted closure) and the **economic** analysis (rational
adversary; see
[`../attack-modeling/03-economic-game-theoretic.md`](../attack-modeling/03-economic-game-theoretic.md)).
The Rocq theorem T-12 is a stake-weighted statement; the Sage
model corroborates it on small bounds and surfaces stake
distributions that maximize adversary leverage.

# 10 · Quorum-intersection models

## 1 · Family motivation

The BFT safety guarantee depends on **quorum intersection**: any two
quorums of size `> 2n/3` must share at least one honest validator
[CL99, LSP82]. The slashing closure preserves this guarantee only
if the closure does not slash all honest validators in *both*
quorums. This family searches small stake distributions for
intersection violations.

## 2 · The model

| Model                                                                                                  | Searches                                                |
|--------------------------------------------------------------------------------------------------------|---------------------------------------------------------|
| [`quorum_intersection_model.sage`](../../../../../formal/sage/slashing/quorum_intersection_model.sage) | Weighted quorum intersection over bounded stake vectors |

## 3 · Representative witness

```json
{
  "kind": "quorum_intersection_witness",
  "n": 5,
  "stakes": [3, 3, 3, 3, 3],
  "total_stake": 15,
  "quorum_threshold": 10,
  "candidate_quorum_a": [0, 1, 2, 3],
  "candidate_quorum_b": [1, 2, 3, 4],
  "quorum_a_weight": 12,
  "quorum_b_weight": 12,
  "intersection": [1, 2, 3],
  "intersection_weight": 9,
  "is_valid": true
}
```

Reading: two quorums of weight 12 each (above threshold 10) share
three validators of weight 9. The intersection is non-empty —
quorum intersection holds.

## 4 · Promotion targets

| Witness shape                      | Rocq theorem                   | TLA⁺ model                                   | Rust regression                      |
|------------------------------------|--------------------------------|----------------------------------------------|--------------------------------------|
| Weighted quorum intersection       | `weighted_quorum_intersection` | `TwoLevelSlashing.tla` `Inv_QuorumIntersect` | `quorum_intersection_after_slash.rs` |
| Intersection preserved under slash | T-12 family                    | `TwoLevelSlashing.tla` `Inv_BFTBound`        | `prop_t_12_quorum_preservation.rs`   |

## 5 · Related findings

In [`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md):

- **#10** Weighted quorum intersection held for all bounded stake
  vectors tested through `n=5, stake 1..3`.

## 6 · Methodology note

Quorum intersection is the *foundational* BFT property — every other
safety property in CBC Casper [Zam17] ultimately depends on it. The
slashing methodology's role here is **defensive**: the slashing
process must not slash *both quorums into emptiness* by amplifying a
single direct offender's damage past the BFT bound. The Sage model
corroborates the Rocq theorem `weighted_quorum_intersection` on
small bounds; the unbounded statement lives in
[`formal/rocq/slashing/theories/TwoLevelSlashing.v`](../../../../../formal/rocq/slashing/theories/TwoLevelSlashing.v).

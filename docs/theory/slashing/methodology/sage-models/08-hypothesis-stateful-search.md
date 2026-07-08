# 08 · Hypothesis stateful search

## 1 · Family motivation

Sage's enumeration is exhaustive on small bounds, but a 50-action
adversarial campaign is beyond Sage's reach. The Hypothesis search
engine
[`hypothesis_search/hypothesis_scenario_search.sage`](../../../../../formal/sage/slashing/hypothesis_search/hypothesis_scenario_search.sage)
extends the methodology with a Python Hypothesis state machine
running *inside* Sage, with shrinking-by-action-removal and a
persistent failing-example database.

The pedagogical framework for stateful Hypothesis is
[`../randomized-search/02-stateful-hypothesis.md`](../randomized-search/02-stateful-hypothesis.md);
this chapter documents the in-Sage scenario engine.

## 2 · The model

| Model                                                                                                                                        | Searches                                                                                              |
|----------------------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------|
| [`hypothesis_search/hypothesis_scenario_search.sage`](../../../../../formal/sage/slashing/hypothesis_search/hypothesis_scenario_search.sage) | Stateful action-sequence search across the full lifecycle (bond, equivocate, slash, withdraw, rebond) |

The model uses Hypothesis's `stateful` API: it builds a finite-state
abstraction of the slashing harness, declares actions with
preconditions and post-conditions, and lets Hypothesis search for
sequences that violate the post-conditions.

## 3 · Representative witness

```json
{
  "kind": "hypothesis_scenario_witness",
  "seed": "0xfeedface",
  "actions": [
    {"op": "bond", "v": "v0", "stake": 100},
    {"op": "bond", "v": "v1", "stake": 100},
    {"op": "sign", "v": "v0", "seq": 0},
    {"op": "sign", "v": "v1", "seq": 0},
    {"op": "equivocate", "v": "v0", "seq": 0},
    {"op": "slash_proposal", "proposer": "v1", "offender": "v0"},
    {"op": "withdraw_request", "v": "v0"},
    {"op": "withdraw_transfer_fail", "v": "v0"},
    {"op": "withdraw_retry", "v": "v0"}
  ],
  "violated_invariant": null,
  "shrunken_from_actions_count": 73,
  "covered_invariants": ["I_total_funds_conserved", "I_withdrawer_eventually_paid"]
}
```

Reading: a 9-action lifecycle covering the canonical Bug #10 scenario
(post-quarantine withdrawal transfer-failure flow). The witness was
**shrunk from 73 actions to 9** by Hypothesis's automatic minimization;
the shrunken witness is the deterministic regression seed.

## 4 · Promotion targets

| Witness shape                       | Rocq theorem                               | TLA⁺ model                                   | Rust regression                                                                 |
|-------------------------------------|--------------------------------------------|----------------------------------------------|---------------------------------------------------------------------------------|
| Lifecycle invariants under campaign | T-9.10 (withdrawal safety)                 | `WithdrawFlow.tla` `Inv_TotalFundsConserved` | `prop_t_9_10_withdraw_safety.rs`, `hypothesis_bundle_evidence_state_machine.rs` |
| Multi-epoch invariants              | T-9.11                                     | `AuthorizedSlashFlow.tla`                    | `hypothesis_multi_epoch_state_machine.rs`                                       |
| Partition / gossip campaigns        | (informal; threat model §5.A.5)            | (partial; documented in TLA⁺ README)         | `hypothesis_partition_gossip_state_machine.rs`                                  |
| Liveness as safety                  | T-2 (detection completeness)               | (LTL fairness)                               | `hypothesis_liveness_as_safety.rs`                                              |
| Persistent corpus replay            | (no theorem; corpus is a regression layer) | —                                            | `hypothesis_persistent_corpus.rs`                                               |

## 5 · Related findings

In [`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md):

- **#33–#52** Most Hypothesis-driven findings are recorded in this
  range; each is shrunk to a deterministic action sequence before
  being added to the corpus.

## 6 · Methodology note

This model is the bridge between Sage's *exact-enumeration* paradigm
and the Rust-side Hypothesis tests
([`casper/tests/slashing/hypothesis_*.rs`](../../../../../casper/tests/slashing/)).
The Sage side generates scenarios; the Rust side replays them
deterministically. The two halves cooperate to deliver the
methodology's most cost-effective adversarial coverage.

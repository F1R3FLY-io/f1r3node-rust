# Casper Authority and Finality Soak Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-002
status: pending
adapter: embedded
decisions: [D-02, D-03, D-04]
pre_merge_tasks: [TASK-017-5]
post_merge_tasks: [TASK-018-3]
artifacts:
  - casper/src/rust/estimator.rs
  - casper/src/rust/util/dag_operations.rs
  - casper/src/rust/finality/floor.rs
  - casper/src/rust/validate.rs
refutation: pending
construction: pending
construction_assumptions: null
binding: pending
soak: pending
```

## Contract

Inputs are authenticated blocks, exact justification sets, stake provenance, finalized floor, execution-effect identities, and explicit metadata availability.

Outputs are the selected head, finality decision, hold reason, and traversal work counters.

For each admissible DAG, floor-bounded and reference LCA traversal must select the same valid GHOST head.

Both paths must preserve electorate, main-parent-relative depth, latest-message filtering, truncation, and progress rules.

Finality requires the strict agreeing-majority precondition and inclusive clique threshold `2qd >= S(d+n)`.

Missing required metadata produces a hold, not a negative vote. Verified failed-body settlement counts as applied accounting effects.

Committee authority requires exact justification equality, duplicate rejection, upstream stake provenance, and validator block signatures. Required certificate sidecars and second state certificates are excluded.

## Model and oracle

Audit existing `formal/tlaplus/fork_choice/` and `formal/tlaplus/finalized_floor/` models against the ratifications before reuse.

The proposed finite instance uses three validators, six blocks, two forks, and explicit missing-metadata states. Record the actual constants for every run.

The reference oracle must independently implement upstream head selection. Calling the optimized traversal from both paths is not a differential test.

The production bridge covers estimator, DAG traversal, floor evaluation, and validation. TASK-017-5 must record exact function names and source digests before verification.

## Positive and negative controls

| Control | Required observation |
| --- | --- |
| Clean admissible DAGs | Identical valid heads and retained finalized effects. |
| Traverse below finalized floor | Violate the declared floor work bound. |
| Replace inclusive threshold with strict threshold | Reject a boundary case accepted by the reference oracle. |
| Omit agreeing-majority precondition | Accept an invalid finality case and fail the oracle comparison. |
| Treat missing metadata as disagreement | Violate the hold rule. |
| Substitute committee provenance | Violate authenticated committee agreement. |

Each defect needs a named model property and a production regression. A model failure alone does not identify a production defect.

## Tiers and limits

TLC establishes only the stated finite result. Arbitrary-DAG and validator-set claims need Rocq construction evidence and production bindings.

Candidate construction projects are `formal/rocq/fork_choice/` and `formal/rocq/finalized_floor/`. Existing theorem names and assumptions need audit before reuse.

Construction requires a named theorem, source digest, kernel check, and closed assumption set. No `Axiom`, `Admitted`, or unaccounted `Parameter` can discharge this claim.

Soaks measure completed finalization, retained effects, traversal work, and resource use. Low latency alone does not prove a traversal bound.

## Phase obligations

Pre-merge work supplies audited models, reference oracles, and current-dev bindings. Missing candidate-specific bindings remain pending.

Post-merge work reruns the bindings against the actual #216 merge. Certificate-removal regressions must pass before coupled certificate code is removed.

The [harness contract](./casper-soak-harness.md) defines evidence fields, failure handling, and closure. No part of this scaffold is discharge evidence.

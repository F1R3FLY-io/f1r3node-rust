# Casper Slashing Profile Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-006
status: pending
adapter: embedded
scope: harness-profile
profile_implementation: not-implemented
decisions: [D-09]
pre_merge_tasks: [TASK-017-9]
post_merge_tasks: [TASK-018-3, TASK-018-5]
artifacts:
  - scripts/run-merge-recovery-soak.sh
  - scripts/bench/test-run-merge-recovery-soak.sh
  - scripts/bench/write-soak-summary.sh
  - formal/tlaplus/casper_soak/verification-plan.jsonc
refutation: pending
construction: not-applicable
construction_assumptions: null
binding: pending
soak: pending
```

## Scope

This claim verifies the profile generator, fault scheduling, collector, and verdict classifier. The Rust node is the system under test, not a proof artifact.

It does not prove the node's consensus, storage, cryptography, or accounting implementation. Product failures remain observations for separate node work.

## Profile contract

The [common interface contract](../casper/design/soak-interface-contract.md) defines record types, correlation, verdicts, and source bindings.

Additional request fields are `evidence_digest`, `invalid_hash_seed`, `offender`, `parent_prestate_digest`, `stake`, `epoch`, `rebond_epoch`, and expected authorization.

Required observations contain delivery receipts, evidence identity, parent pre-state reference, observed epoch, actual authorization, and recovery outcome.

The positive transcript matches a reviewed truth-table row. Expected rejection of stale or forged evidence is a passing scenario, not a verifier counterexample.

Source expectations require positive stake, matching epochs, rebond protection, deterministic invalid-hash seeds, current evidence, and offender deduplication.

Inputs are the pinned scenario, expected fixture outcomes, candidate identities, deterministic seed, requested faults, and observed event transcript.

Outputs are coverage acknowledgments, correlated measurements, scenario verdicts, and immutable evidence references.

Pin the evidence scenario, offender identity, epoch, expected outcome, and candidate revision.

Record the actual delivery order, rebond event, restart, and observed authorization result.

Report differences from reviewed fixture expectations without granting slash authority or changing node policy.

## Scenario coverage

- Evidence arrival permutations and merge-lost slash scenarios.
- Same-key rebond, stale epochs, duplicate evidence, and forged-deploy scenarios.
- Missing dependencies and restart during evidence delivery.

Unavailable test interfaces produce a blocked scenario, not a passing result. Adding or repairing node interfaces is outside these epics.

## Formal controls and executable fixtures

| Proposed property | Defect knob | Fixture ID | Required fixture result |
| --- | --- | --- | --- |
| EvidenceOrderRecorded | ReuseRequestedOrder | slashing_delivery_order | Request A then B, observe B then A. Retain observed order and report incomplete coverage of the requested order. |
| EpochCorrelationRequired | DropEpochIdentity | slashing_epoch_mismatch | Supply an observation from an unrelated epoch. Quarantine it and report `incomplete`. |
| AuthorizationMismatchReported | SuppressSlashMismatch | slashing_authorization_mismatch | Supply correlated evidence with authorization opposite to the pinned expectation. Expect `product_failure`. |

A clean fixture uses a complete known transcript. Each negative control mutates profile handling, not the node, and must violate its named property.

TLC explores bounded scenario, event, and outcome states. Real harness fixtures must exercise the profile implementation with matching and mismatching transcripts.

Construction is not applicable under PR #433's harness approach. No Rocq theorem or node-code discharge is required by this claim.

The bound is two scenarios and three observations per scenario for the initial model. This is proposed coverage, not completed verification.

## Interface qualification and phase obligations

SI-DEPLOY and SI-QUERY do not qualify evidence delivery order or complete slash-authorization context. Generic process control cannot establish ordered protocol-message delivery.

TASK-017-9 must qualify delivery receipts, parent pre-state, rebond identity, and epoch-bound authorization observations. Their absence blocks affected live scenarios.

The family includes merge-lost slash, missing evidence, forged deploy, rebond, duplicate offender evidence, stale epochs, and restart during delivery.

Rejection records alone do not authorize slashing. Reconstruction is supplementary, and node bisimilarity proofs remain external to this profile.

TASK-018-3 must adapt evidence encoding, delivery receipts, pre-state/epoch fields, authorization results, and merge-rejected recovery observations.

Pre-merge work defines and verifies the profile against current supported interfaces and controlled transcripts.

After PR #216 merges, adapt the profile interfaces and rerun its model controls, fixtures, and approved soak scenarios with new identities.

A correct harness can report a failed product scenario. Passing harness verification does not convert that product failure into a passing soak.

The [harness contract](./casper-soak-harness.md) defines provenance and outcome rules. Deferred policies still require separate approval before activation.

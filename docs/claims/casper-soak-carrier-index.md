# Casper Carrier Index Profile Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-008
status: pending
adapter: embedded
scope: harness-profile
profile_implementation: not-implemented
decisions: [D-10]
pre_merge_tasks: [TASK-017-10]
post_merge_tasks: [TASK-018-4, TASK-018-5]
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

Inputs are the pinned scenario, expected fixture outcomes, candidate identities, deterministic seed, requested faults, and observed event transcript.

Outputs are coverage acknowledgments, correlated measurements, scenario verdicts, and immutable evidence references.

Pair forced-index and reference-scan observations from the same candidate, DAG, window, and availability fixture.

Record watermark, pruning, crash, read-failure, and identity-domain settings before comparing results.

Report verdict differences and probe/body-read counts only when the selected path and required counters are observed.

## Scenario coverage

- Valid, invalid, and approved carriers with forks and missing history.
- Watermark and pruning boundaries, read failures, and restart.
- Cross-domain identity fixtures when the approved candidate exposes the required test interface.

Unavailable test interfaces produce a blocked scenario, not a passing result. Adding or repairing node interfaces is outside these epics.

## Formal controls and executable fixtures

| Proposed property | Defect knob | Required fixture result |
| --- | --- | --- |
| PathEngagementObserved | AssumeIndexEngaged | A requested index path without engagement evidence must not establish the work bound. |
| CarrierInputsMatched | CompareDifferentWindows | Different scan windows cannot produce a passing differential result. |
| MissingCountersUnknown | ZeroMissingCounters | Missing counters must not become zero ancestor reads. |

A clean fixture uses a complete known transcript. Each negative control mutates profile handling, not the node, and must violate its named property.

TLC explores bounded scenario, event, and outcome states. Real harness fixtures must exercise the profile implementation with matching and mismatching transcripts.

Construction is not applicable under PR #433's harness approach. No Rocq theorem or node-code discharge is required by this claim.

The bound is two scenarios and three observations per scenario for the initial model. This is proposed coverage, not completed verification.

## Phase obligations

Pre-merge work defines and verifies the profile against current supported interfaces and controlled transcripts.

After PR #216 merges, adapt the profile interfaces and rerun its model controls, fixtures, and approved soak scenarios with new identities.

A correct harness can report a failed product scenario. Passing harness verification does not convert that product failure into a passing soak.

The [harness contract](./casper-soak-harness.md) defines provenance and outcome rules. Deferred policies still require separate approval before activation.

[CLAIM-FINALITY-002](./repeat-deploy-carrier-index-equivalence.md) remains an external node-correctness claim. This profile neither owns nor discharges it.

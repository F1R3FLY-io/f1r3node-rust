# Casper Publication and Restart Profile Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-003
status: pending
adapter: embedded
scope: harness-profile
profile_implementation: not-implemented
decisions: [D-05]
pre_merge_tasks: [TASK-017-6]
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

Additional request fields are `cut_point`, `publication_id`, `generation`, `predecessor_incarnation`, `expected_tuple`, and `unresolved_occurrences`.

A publication observation contains `block_hash`, `state_root`, `effect_digest`, generation, durable terminal verdicts, and retained-work identities from one correlated snapshot.

The positive transcript contains an acknowledged cut point and a linked restart. Its recovered tuple matches one permitted complete tuple, never mixed components.

Expected tuples are pinned fixture values. The collector must not recompute node execution or substitute post-crash observations from another node.

Inputs are the pinned scenario, expected fixture outcomes, candidate identities, deterministic seed, requested faults, and observed event transcript.

Outputs are coverage acknowledgments, correlated measurements, scenario verdicts, and immutable evidence references.

Record each requested crash point and the observed process termination before labeling a fault as injected.

Correlate pre-crash and recovered block/root/effect observations by candidate, node, and restart identity.

Report torn tuples, stale publication observations, and unresolved-work loss without overwriting earlier failures.

## Scenario coverage

- Crash before and after observable publication boundaries.
- Restart with unresolved deploy work and durable terminal verdicts.
- Single-flight baseline and separately approved parallel experiments.

Unavailable test interfaces produce a blocked scenario, not a passing result. Adding or repairing node interfaces is outside these epics.

## Formal controls and executable fixtures

| Proposed property | Defect knob | Fixture ID | Required fixture result |
| --- | --- | --- | --- |
| FaultAcknowledged | AssumeCrashApplied | publication_fault_unacknowledged | Remove the cut-point or exit receipt. Expect `incomplete` and zero acknowledged coverage. |
| RestartIdentityMatched | MixRestartObservations | publication_restart_mismatch | Substitute an unrelated node/incarnation. Quarantine that observation and report `incomplete`. |
| TornTupleReported | HideTupleMismatch | publication_torn_tuple | Mix old root with new block in an otherwise correlated observed tuple. Expect `product_failure`. |

A clean fixture uses a complete known transcript. Each negative control mutates profile handling, not the node, and must violate its named property.

TLC explores bounded scenario, event, and outcome states. Real harness fixtures must exercise the profile implementation with matching and mismatching transcripts.

Construction is not applicable under PR #433's harness approach. No Rocq theorem or node-code discharge is required by this claim.

The bound is two scenarios and three observations per scenario for the initial model. This is proposed coverage, not completed verification.

## Interface qualification and phase obligations

SI-FAULT provides generic restarts, not publication-boundary crashes. SI-QUERY does not qualify an atomic durable publication snapshot.

TASK-017-6 must qualify cut-point receipts, durable tuples, stale-generation observations, and unresolved-work inventories. Missing capabilities block their live scenarios.

Generic restart coverage cannot replace before/after-publication coverage. Adopted subprocess handles cannot supply SI-ADOPTED restarts.

Additional fixtures retain stale-publication failures and lost unresolved work. Eviction is acceptable only with the required durable terminal-verdict observation.

TASK-018-3 must adapt tuple fields, generation identity, terminal-verdict capture, and restart receipts to the merged node.

Pre-merge work defines and verifies the profile against current supported interfaces and controlled transcripts.

After PR #216 merges, adapt the profile interfaces and rerun its model controls, fixtures, and approved soak scenarios with new identities.

A correct harness can report a failed product scenario. Passing harness verification does not convert that product failure into a passing soak.

The [harness contract](./casper-soak-harness.md) defines provenance and outcome rules. Deferred policies still require separate approval before activation.

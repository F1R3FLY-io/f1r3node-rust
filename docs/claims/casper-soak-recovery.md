# Casper Recovery and Custody Profile Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-004
status: discharged
adapter: embedded
scope: harness-profile
profile_implementation: controlled-transcript-implemented
decisions: [D-06, D-07]
pre_merge_tasks: [TASK-017-7]
post_merge_tasks: [TASK-018-3, TASK-018-5]
artifacts:
  - scripts/casper-soak/src/profiles/recovery.rs
  - scripts/casper-soak/src/bin/casper-recovery.rs
  - scripts/casper-soak/tests/recovery.rs
  - scripts/casper-soak/check-recovery.sh
  - .github/workflows/casper-recovery.yml
  - formal/tlaplus/casper_soak/profiles/recovery/Recovery.tla
  - formal/tlaplus/casper_soak/profiles/recovery/MC_Recovery.cfg
  - formal/tlaplus/casper_soak/profiles/recovery/MC_Recovery_lane_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/recovery/MC_Recovery_occurrence_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/recovery/MC_Recovery_pause_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/recovery/verification-plan.jsonc
  - formal/tlaplus/casper_soak/profiles/recovery/README.md
refutation: bounded-safety-pass
construction: not-applicable
construction_assumptions: null
binding: passed
soak: pending
```

## Scope

This claim verifies the profile generator, fault scheduling, collector, and verdict classifier. The Rust node is the system under test, not a proof artifact.

It does not prove the node's consensus, storage, cryptography, or accounting implementation. Product failures remain observations for separate node work.

## Profile contract

The [common interface contract](../casper/design/soak-interface-contract.md) defines record types, correlation, verdicts, and source bindings.

Additional request fields are `recovery_lane`, `policy_variant`, `coverage_rule`, `frontier_digest`, `objective_height`, `lifespan`, and requested pause/delivery schedule.

Observations retain source occurrence identity and schema, deploy signature, carrier block, sender, custodian, joined reasons, causal references, tombstone state, lease, and terminal outcome.

The occurrence key is candidate-scoped `occurrence_id` under the pinned `occurrence_schema`. Repeated observations of one key differ from independent occurrences.

The adapter must preserve the source identity definition. It cannot fabricate an occurrence or execution index from observation order.

The positive transcript preserves lane labels and occurrence multiplicity. It includes an observed pause acknowledgment and expected retry or expiry outcome.

Durations need same-clock start/end observations. Cross-node clocks require declared synchronization bounds, otherwise recovery latency remains unknown.

Inputs are the pinned scenario, expected fixture outcomes, candidate identities, deterministic seed, requested faults, and observed event transcript.

Outputs are coverage acknowledgments, correlated measurements, scenario verdicts, and immutable evidence references.

Pin the recovery lane and policy variant in the profile manifest before workload launch.

Record requested and observed pauses, delivery delays, frontiers, retries, and objective heights.

Compute duplicates, custody disagreements, retry completion, and expiry from exact occurrence identities.

## Scenario coverage

- All-eligible stale recovery and leader-only convergence as distinct baseline lanes.
- Paused validators, delayed messages, split frontiers, and empty or deploy-bearing workloads.
- Isolated frontier, leadership, clock, and coverage variants when supported by an approved test build.

Unavailable test interfaces produce a blocked scenario, not a passing result. Adding or repairing node interfaces is outside these epics.

## Formal controls and executable fixtures

| Property | Defect knob | Fixture ID | Required fixture result |
| --- | --- | --- | --- |
| LaneLabelsPreserved | ConflateRecoveryLanes | recovery_lane_mismatch | Give a convergence request only stale-recovery observations. Expect `incomplete` for required convergence coverage. |
| OccurrenceCountsPreserved | CollapseOccurrenceIdentity | recovery_occurrence_counts | Supply three observations: two copies of occurrence A and one of B. Expect two occurrences and one duplicate. |
| PauseCoverageAcknowledged | AssumePauseApplied | recovery_pause_unacknowledged | Request a pause without an observed paused-state receipt. Expect `incomplete`, not exercised coverage. |

A clean fixture uses a complete known transcript. Each negative control mutates profile handling, not the node, and must violate its named property.

TLC explores bounded scenario, event, and outcome states. Real harness fixtures must exercise the profile implementation with matching and mismatching transcripts.

Construction is not applicable under PR #433's harness approach. No Rocq theorem or node-code discharge is required by this claim.

The model explores two scenarios and three occurrence sample slots. Its clean control passes, and three unsafe controls violate their named properties.

The [implementation contract](../../formal/tlaplus/casper_soak/profiles/recovery/README.md) defines the controlled schema, measurement rules, and executable commands.

The [work log](../work-logs/task-017-7-recovery.md) records evidence and remaining gates. The user accepted the bounded pre-merge binding and ratified the mandatory workflow tag.

The [acceptance record](../work-logs/task-017-5-7-acceptance.md) binds approval to the verified sources. This discharge does not resolve D-07 or establish node support for the synthetic schema.

## Interface qualification and phase obligations

SI-FAULT provides pause commands, but their return values do not acknowledge paused state. SI-LOAD uses explicit stress overrides, not baseline recovery settings.

TASK-017-7 implements controlled lane/custody observations, objective-height samples, delivery receipts, and paused-state receipts. Live adapter qualification remains incomplete.

D-07 now identifies uncertainty about exact-occurrence and reason-join support on `dev`. These controlled fixtures do not resolve that ratification question.

The synthetic occurrence schema does not claim current node support. All live, experimental-policy, and post-merge requests remain blocked.

Baseline expectations retain all-eligible stale recovery, leader-only convergence, frontier follow, readiness/backstop lanes, and one-parent B1 coverage.

A lease cannot authorize recovery or bypass missing-body, lifespan, or objective-height gates. Reason joins preserve pinned commutative, associative, and idempotent expectations.

Rotating leaders, alternate clocks, collective coverage, and leader-free custody require separate experimental identities and supported test builds.

TASK-018-3 must adapt lane labels, occurrence identities, fault receipts, and retry/expiry observations. TASK-018-5 compares only compatible policy variants.

Pre-merge work defines and verifies the profile against current supported interfaces and controlled transcripts.

After PR #216 merges, adapt the profile interfaces and rerun its model controls, fixtures, and approved soak scenarios with new identities.

A correct harness can report a failed product scenario. Passing harness verification does not convert that product failure into a passing soak.

The [harness contract](./casper-soak-harness.md) defines provenance and outcome rules. Deferred policies still require separate approval before activation.

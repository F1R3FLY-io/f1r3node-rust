# Casper Protocol and Phlo Profile Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-007
status: pending
adapter: embedded
scope: harness-profile
profile_implementation: not-implemented
decisions: [D-01, D-12]
pre_merge_tasks: [TASK-017-11]
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

Pin Casper protocol and accounting authority as separate manifest fields.

Preserve both phloLimit and phloPrice in generated requests and captured envelope observations.

Report observed acceptance, rejection, prepayment, refund, and exhaustion against reviewed fixture expectations.

## Scenario coverage

- Approved and unsupported protocol versions.
- Signed-field mutation, shard minimum-price boundaries, and exhausted deploys.
- Separate protocol-7 candidate shards only when the required test build and configuration exist.

Unavailable test interfaces produce a blocked scenario, not a passing result. Adding or repairing node interfaces is outside these epics.

## Formal controls and executable fixtures

| Proposed property | Defect knob | Required fixture result |
| --- | --- | --- |
| VersionLabelsSeparate | ConflateAuthorityVersions | Accounting version 8 cannot label a Casper protocol-7 run. |
| BothPhloFieldsCaptured | OmitPhloPrice | Omitting either Phlo field must fail profile completeness checks. |
| SettlementOutcomeClassified | IgnoreRefundMismatch | A planted refund mismatch must remain visible in the report. |

A clean fixture uses a complete known transcript. Each negative control mutates profile handling, not the node, and must violate its named property.

TLC explores bounded scenario, event, and outcome states. Real harness fixtures must exercise the profile implementation with matching and mismatching transcripts.

Construction is not applicable under PR #433's harness approach. No Rocq theorem or node-code discharge is required by this claim.

The bound is two scenarios and three observations per scenario for the initial model. This is proposed coverage, not completed verification.

## Phase obligations

Pre-merge work defines and verifies the profile against current supported interfaces and controlled transcripts.

After PR #216 merges, adapt the profile interfaces and rerun its model controls, fixtures, and approved soak scenarios with new identities.

A correct harness can report a failed product scenario. Passing harness verification does not convert that product failure into a passing soak.

The [harness contract](./casper-soak-harness.md) defines provenance and outcome rules. Deferred policies still require separate approval before activation.

# Casper Merge and Accounting Profile Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-005
status: pending
adapter: embedded
scope: harness-profile
profile_implementation: not-implemented
decisions: [D-08]
pre_merge_tasks: [TASK-017-8]
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

The [common interface contract](../casper/design/soak-interface-contract.md) defines record types, correlation, verdicts, and source bindings.

Additional request fields are `execution_fixture`, `causal_edges`, `token_domain`, `protocol_epoch`, `accounting_mode`, and pinned expected admission, effect, and settlement values.

Each observation retains deploy signature, block, execution position, full context digest, admission result, body result, effect identity, and settlement values with presence states.

The execution identity is `(source_block_hash, execution_position)`. Deploy signature and complete context remain separate correlation fields, not replacements for that identity.

Repeated observations of one execution count once. Conflicting signatures or contexts for that execution invalidate the comparison instead of creating another execution.

Admission rejection has no execution position. Executed failure retains its position and requires settlement observations when the fixture expects applied accounting effects.

The positive transcript has two distinct executions with equal effects and a repeated observation of one execution. Counts are two executions and one duplicate.

Inputs are the pinned scenario, expected fixture outcomes, candidate identities, deterministic seed, requested faults, and observed event transcript.

Outputs are coverage acknowledgments, correlated measurements, scenario verdicts, and immutable evidence references.

Generate workloads that distinguish repeated observations from independent executions with identical effects.

Retain exact execution identities, admission outcomes, settlement observations, and token domains in the dataset.

Calculate expected values from pinned fixture data and report measured differences without changing node accounting.

## Scenario coverage

- Independent identical effects and repeated observations of the same execution.
- Causal chains, failed execution, admission rejection, and overflow boundary workloads.
- Separate legacy and conditional additive profiles with explicit compatibility labels.

Unavailable test interfaces produce a blocked scenario, not a passing result. Adding or repairing node interfaces is outside these epics.

## Formal controls and executable fixtures

| Proposed property | Defect knob | Fixture ID | Required fixture result |
| --- | --- | --- | --- |
| MultiplicityMeasured | DeduplicateByEffectValue | accounting_multiplicity | Observe executions A and B with equal effects, then A again. Retain both executions and one duplicate. |
| SettlementCoverageRequired | TreatMissingSettlementAsZero | accounting_settlement_missing | Remove one required settlement. Expect unknown amount and `incomplete`, never a balanced zero. |
| CompatibilityLabeled | MixProtocolProfiles | accounting_epoch_mismatch | Pair different epochs or accounting modes without a reviewed mapping. Expect `invalid_input`. |

A clean fixture uses a complete known transcript. Each negative control mutates profile handling, not the node, and must violate its named property.

TLC explores bounded scenario, event, and outcome states. Real harness fixtures must exercise the profile implementation with matching and mismatching transcripts.

Construction is not applicable under PR #433's harness approach. No Rocq theorem or node-code discharge is required by this claim.

The bound is two scenarios and three observations per scenario for the initial model. This is proposed coverage, not completed verification.

## Interface qualification and phase obligations

SI-DEPLOY and SI-QUERY supply submission and query primitives. They do not qualify execution-position, full-context, or complete settlement extraction.

TASK-017-8 must qualify those records and causal relationships. Unsupported overflow, token-domain, and effect-admission scenarios remain blocked.

Pinned fixture expectations cover admission/effect alignment, least causal rejection closure, checked arithmetic, and pooling once after aggregation.

Conditional additive profiles require protocol/FIPS/fresh-genesis and compatibility prerequisites. Mixed epochs cannot become a single comparable run.

TASK-018-4 must adapt execution identity, admission results, failed-body settlement, token domains, and compatibility mappings against the actual merged node.

Pre-merge work defines and verifies the profile against current supported interfaces and controlled transcripts.

After PR #216 merges, adapt the profile interfaces and rerun its model controls, fixtures, and approved soak scenarios with new identities.

A correct harness can report a failed product scenario. Passing harness verification does not convert that product failure into a passing soak.

The [harness contract](./casper-soak-harness.md) defines provenance and outcome rules. Deferred policies still require separate approval before activation.

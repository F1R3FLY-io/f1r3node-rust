# Casper Authority and Finality Profile Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-002
status: pending
adapter: embedded
scope: harness-profile
profile_implementation: controlled-transcript-implemented
decisions: [D-02, D-03, D-04]
pre_merge_tasks: [TASK-017-5]
post_merge_tasks: [TASK-018-3, TASK-018-5]
artifacts:
  - scripts/casper-soak/src/profiles/authority_finality.rs
  - scripts/casper-soak/src/bin/casper-authority-finality.rs
  - scripts/casper-soak/tests/authority_finality.rs
  - scripts/casper-soak/check-authority-finality.sh
  - .github/workflows/casper-authority-finality.yml
  - formal/tlaplus/casper_soak/profiles/authority_finality/AuthorityFinality.tla
  - formal/tlaplus/casper_soak/profiles/authority_finality/MC_AuthorityFinality.cfg
  - formal/tlaplus/casper_soak/profiles/authority_finality/MC_AuthorityFinality_pair_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/authority_finality/MC_AuthorityFinality_finality_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/authority_finality/MC_AuthorityFinality_head_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/authority_finality/verification-plan.jsonc
  - formal/tlaplus/casper_soak/profiles/authority_finality/README.md
refutation: bounded-safety-pass
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

Additional request fields are `dag_digest`, `electorate_digest`, `justification_digest`, `threshold_inputs`, `metadata_availability`, and `evaluation_mode`.

Required observations contain `head_hash`, `finality_decision`, `hold_reason`, original fault-tolerance value, projection, and traversal counters with presence states.

`threshold_inputs` names clique stake `q`, total stake `S`, agreeing stake, and threshold numerator `n` with positive denominator `d`.

Expectations require strict agreeing majority before the inclusive comparison `2qd >= S(d+n)`.

A threshold fixture uses `S=12`, `n=1`, and `d=3`: clique stakes 7, 8, and 9 respectively fail, equal, and exceed the threshold.

Another fixture uses agreeing stake 6 of 12 with threshold zero. It must fail the strict-majority precondition despite threshold equality.

These are threshold-component expectations, not automatic finalization. Missing metadata, containment, and other ratified gates remain applicable.

The positive transcript pairs identical inputs with equal heads and the expected finality decision. An explicit metadata hold can match an expectation.

A missing finality response is not a hold. Missing counters block a required work comparison but do not erase a separately observed head mismatch.

Inputs are the pinned scenario, expected fixture outcomes, candidate identities, deterministic seed, requested faults, and observed event transcript.

Outputs are coverage acknowledgments, correlated measurements, scenario verdicts, and immutable evidence references.

Generate paired runs from the same DAG fixture, seed, electorate, and candidate identities.

Compare reported heads and finality decisions only when both required observations are present.

Keep missing metadata, hold decisions, and missing telemetry distinct from disagreement and passing results.

## Scenario coverage

- Committee provenance and duplicate-justification scenarios.
- Inclusive finality boundaries, strict-majority preconditions, and missing dependencies.
- Available bounded/reference traversal controls with measured work counters.

Unavailable test interfaces produce a blocked scenario, not a passing result. Adding or repairing node interfaces is outside these epics.

## Formal controls and executable fixtures

| Property | Defect knob | Fixture ID | Required fixture result |
| --- | --- | --- | --- |
| MismatchedInputDetected | PairDifferentDags | authority_pair_mismatch | Change one paired DAG digest. Expect `invalid_input`, not a head-equivalence result. |
| MissingFinalityDetected | AcceptMissingFinality | authority_finality_missing | Remove one required finality observation. Expect `incomplete`, not hold or pass. |
| HeadMismatchReported | SuppressHeadMismatch | authority_head_mismatch | Give a matched pair different observed heads. Expect `product_failure` with both source references. |

A clean fixture uses a complete known transcript. Each negative control mutates profile handling, not the node, and must violate its named property.

TLC explores bounded scenario, event, and outcome states. Real harness fixtures must exercise the profile implementation with matching and mismatching transcripts.

Construction is not applicable under PR #433's harness approach. No Rocq theorem or node-code discharge is required by this claim.

The initial model uses two scenarios and three collection slots per scenario. Its clean search and three named negative controls pass.

Other required fields and applied-step receipts are assumed valid in this abstraction. Executable fixtures check the concrete record handling separately.

## Interface qualification and phase obligations

SI-QUERY supplies block and finalization primitives. It does not qualify same-DAG evaluation, complete electorate extraction, or original fault-tolerance/projection pairing.

Those capabilities require pinned adapters under TASK-017-5. Until qualification, the associated live scenarios are `blocked`.

The fixture family includes equality and adjacent threshold boundaries, failed strict-majority preconditions, duplicate justifications, signature rejection, replay, restart, and missing dependencies.

Pinned expectations preserve upstream committee provenance and exact justification sets. This profile does not require signed floors, sidecars, or a second state certificate.

TASK-018-3 must adapt electorate fields, finality decisions, traversal counters, and fault-tolerance/projection mappings to the actual merged node.

Pre-merge work defines and verifies the profile against current supported interfaces and controlled transcripts.

After PR #216 merges, adapt the profile interfaces and rerun its model controls, fixtures, and approved soak scenarios with new identities.

A correct harness can report a failed product scenario. Passing harness verification does not convert that product failure into a passing soak.

The [harness contract](./casper-soak-harness.md) defines provenance and outcome rules. Deferred policies still require separate approval before activation.

## Implementation and review status

The [profile guide](../../formal/tlaplus/casper_soak/profiles/authority_finality/README.md) defines the concrete records, commands, bounds, and model mapping.

The implementation uses a separate binary. Existing lifecycle code, its workflow, and accepted CLAIM-CASPER-SOAK-001 evidence remain unchanged.

The profile records compiled helper hashes. Shared helpers retain their separate obligations. Their prior acceptance does not discharge this profile claim.

The [work log](../work-logs/task-017-5-authority-finality.md) records executable fixtures, failure history, and retained verification evidence.

Claim discharge remains pending. The bounded binding review and proposed mandatory workflow tag require human acceptance. Live adapters remain unqualified.

# Casper Carrier Index Profile Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-008
status: pending
adapter: embedded
scope: harness-profile
profile_implementation: controlled-transcript
decisions: [D-10]
pre_merge_tasks: [TASK-017-10]
post_merge_tasks: [TASK-018-4, TASK-018-5]
artifacts:
  - scripts/casper-soak/src/profiles/carrier_index.rs
  - scripts/casper-soak/src/bin/casper-carrier-index.rs
  - scripts/casper-soak/tests/carrier_index.rs
  - scripts/casper-soak/check-carrier-index.sh
  - .github/workflows/casper-carrier-index.yml
  - formal/tlaplus/casper_soak/profiles/carrier_index/CarrierIndex.tla
  - formal/tlaplus/casper_soak/profiles/carrier_index/MC_CarrierIndex.cfg
  - formal/tlaplus/casper_soak/profiles/carrier_index/MC_CarrierIndex_path_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/carrier_index/MC_CarrierIndex_window_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/carrier_index/MC_CarrierIndex_counter_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/carrier_index/verification-plan.jsonc
  - formal/tlaplus/casper_soak/profiles/carrier_index/README.md
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

Additional request fields are `dag_digest`, `deploy_signature`, `scan_window`, `availability_digest`, `watermark`, `retention_boundary`, and requested traversal path.

Observations retain actual path engagement, result set/verdict, block identities, probe count, ancestor-body-read count, fallback reason, and counter presence states.

The positive transcript pairs identical candidate and fixture context across observed index/reference paths. It preserves observed zero counters distinctly from missing counters.

The current raw deploy key is user `DeployDataProto.sig`, extracted as `pd.deploy.sig.to_vec()`. It is not a block or validator signature.

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

| Property | Defect knob | Fixture ID | Required fixture result |
| --- | --- | --- | --- |
| PathEngagementObserved | AssumeIndexEngaged | carrier_path_unobserved | Request index traversal without an engagement receipt. Expect `incomplete`, not a measured work result. |
| CarrierInputsMatched | CompareDifferentWindows | carrier_window_mismatch | Change one paired scan window. Expect `invalid_input`, not a passing differential result. |
| MissingCountersUnknown | ZeroMissingCounters | carrier_counter_missing | Omit body-read count. Retain `null` plus a reason and report `incomplete` for required work coverage. |

A clean fixture uses a complete known transcript. Each negative control mutates profile handling, not the node, and must violate its named property.

TLC explores bounded scenario, event, and outcome states. Real harness fixtures must exercise the profile implementation with matching and mismatching transcripts.

Construction is not applicable under PR #433's harness approach. No Rocq theorem or node-code discharge is required by this claim.

The model bound is two scenarios and three observation steps per scenario. Executable fixture bounds remain separate from this finite abstraction.

## Interface qualification and phase obligations

SI-QUERY supplies block queries, not forced index/reference selection or path-engagement receipts. Generic metrics alone cannot establish which traversal path ran.

TASK-017-10 checks synthetic qualifications for matched inputs, engagement receipts, availability controls, work counters, and fault receipts.

All live requests remain blocked. Actual node interface qualification belongs to TASK-017-12.

The fixture family covers valid, invalid, and approved carriers, forks, missing history, read errors, restart, watermark boundaries, and retention boundaries.

Typed protocol-7 identity requires a FIP-defined domain-separated envelope. Authentication remains separate, and unavailable typed-identity scenarios cannot pass through raw-key fallback.

TASK-018-4 must adapt traversal selectors, engagement markers, watermark/retention fields, identity encoding, and counter mappings without expanding the optimization's scope.

Pre-merge work defines and verifies the profile against current supported interfaces and controlled transcripts.

After PR #216 merges, adapt the profile interfaces and rerun its model controls, fixtures, and approved soak scenarios with new identities.

A correct harness can report a failed product scenario. Passing harness verification does not convert that product failure into a passing soak.

The [profile guide](../../formal/tlaplus/casper_soak/profiles/carrier_index/README.md) defines executable bounds, source binding, limits, and verification commands.

The [work log](../work-logs/task-017-10-carrier-index.md) records verification progress and pending acceptance.

Binding acceptance, evidence publication, hosted workflow verification, and the workflow tag require separate review. This claim remains pending.

The [harness contract](./casper-soak-harness.md) defines provenance and outcome rules. Deferred policies still require separate approval before activation.

[CLAIM-FINALITY-002](./repeat-deploy-carrier-index-equivalence.md) remains an external node-correctness claim. This profile neither owns nor discharges it.

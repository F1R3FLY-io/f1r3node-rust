# Casper Protocol and Phlo Profile Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-007
status: pending
adapter: embedded
scope: harness-profile
profile_implementation: controlled-transcript
decisions: [D-01, D-12]
pre_merge_tasks: [TASK-017-11]
post_merge_tasks: [TASK-018-4, TASK-018-5]
artifacts:
  - scripts/casper-soak/src/profiles/version_phlo.rs
  - scripts/casper-soak/src/bin/casper-version-phlo.rs
  - scripts/casper-soak/tests/version_phlo.rs
  - scripts/casper-soak/check-version-phlo.sh
  - .github/workflows/casper-version-phlo.yml
  - formal/tlaplus/casper_soak/profiles/version_phlo/VersionPhlo.tla
  - formal/tlaplus/casper_soak/profiles/version_phlo/MC_VersionPhlo.cfg
  - formal/tlaplus/casper_soak/profiles/version_phlo/MC_VersionPhlo_versions_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/version_phlo/MC_VersionPhlo_fields_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/version_phlo/MC_VersionPhlo_refund_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/version_phlo/verification-plan.jsonc
  - formal/tlaplus/casper_soak/profiles/version_phlo/README.md
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

Additional request fields are `casper_protocol_version`, `accounting_authority_version`, `phloLimit`, `phloPrice`, `shard_minimum_price`, and signed-envelope digest.

Observations retain both signed Phlo fields, acceptance/rejection, execution outcome, prepayment, charge, refund, and exhaustion status with source references.

The positive transcript labels Casper protocol 7 and accounting authority 8 separately. It preserves both signed fields and matches pinned settlement expectations.

A fixture may test refusal of unsupported activation. Actual protocol-7 execution still needs FIP approval, fresh genesis, and an available supported build.

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

| Proposed property | Defect knob | Fixture ID | Required fixture result |
| --- | --- | --- | --- |
| VersionLabelsSeparate | ConflateAuthorityVersions | phlo_version_labels | Substitute authority version 8 for Casper version 7. Expect `invalid_input` before launch. |
| BothPhloFieldsCaptured | OmitPhloPrice | phlo_signed_field_missing | Remove either signed field in separate cases. Reject malformed requests, or report `incomplete` when capture is missing. |
| SettlementOutcomeClassified | IgnoreRefundMismatch | phlo_refund_mismatch | Supply a complete correlated refund unequal to its pinned expectation. Expect `product_failure`. |

A clean fixture uses a complete known transcript. Each negative control mutates profile handling, not the node, and must violate its named property.

TLC explores bounded scenario, event, and outcome states. Real harness fixtures must exercise the profile implementation with matching and mismatching transcripts.

Construction is not applicable under PR #433's harness approach. No Rocq theorem or node-code discharge is required by this claim.

The model has two scenarios and three abstract observation steps per scenario. Its Boolean abstractions do not prove node behavior or arbitrary observation histories.

## Interface qualification and phase obligations

SI-DEPLOY accepts `phlo_limit` and `phlo_price`; the adapter maps them explicitly to captured `phloLimit` and `phloPrice`.

TASK-017-11 must qualify signed-envelope capture, version rejection, minimum-price configuration, and settlement/exhaustion observations. Unsupported capabilities block their scenarios.

Additional fixtures cover minimum-price equality and adjacent values, exhausted execution, field mutation after signing, and missing prepayment/refund observations.

The profile retains both signed fields and their envelope commitment. Token accounting cannot replace either field, and undefined multi-wallet funding stays blocked.

TASK-018-4 must adapt request encoding, protobuf/API field mappings, version labels, and charge/refund/exhaustion extraction without removing either Phlo field.

Pre-merge work defines and verifies the profile against current supported interfaces and controlled transcripts.

After PR #216 merges, adapt the profile interfaces and rerun its model controls, fixtures, and approved soak scenarios with new identities.

A correct harness can report a failed product scenario. Passing harness verification does not convert that product failure into a passing soak.

The [harness contract](./casper-soak-harness.md) defines provenance and outcome rules. Deferred policies still require separate approval before activation.

## Controlled implementation

The standalone `casper-version-phlo` binary calls the profile generator, collector, and classifier. Shared compiled sources and accepted claims remain unchanged.

The [profile README](../../formal/tlaplus/casper_soak/profiles/version_phlo/README.md) records exact bounds, model assumptions, and verification commands.

The request retains separate protocol and accounting authority labels. A separate `proposed_protocol_version` identifies unsupported-version tests without changing either context label.

The synthetic envelope selects `controlled-json-v1`. Its retained bytes bind both signed fields and the deploy signature identity. This encoding does not qualify protobuf or cryptographic signature verification.

Envelope, admission, and settlement observations require matching context and fixture identities. A signed-field mutation also requires an applied receipt after envelope capture and before admission.

The receipt binds the fixture, deploy, envelope commitment, target, field, old value, and new value. A requested mutation alone cannot establish fault coverage.

The classifier compares pinned values without calculating node accounting. Missing captures remain unknown, and independent product failures survive malformed or incomplete evidence.

The implementation blocks live execution, post-merge execution, experimental policies, and undefined funding mappings. Every controlled result retains zero node launches and a non-passing soak verdict.

The new workflow tag remains unratified. Claim acceptance, canonical evidence review, and final discharge remain pending.

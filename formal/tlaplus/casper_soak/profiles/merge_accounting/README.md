# Merge and Accounting Profile

## Scope

This controlled-transcript profile checks CLAIM-CASPER-SOAK-005 for TASK-017-8. It does not execute a node or prove node accounting.

The executable compares observations with pinned fixtures. It does not derive accounting conservation, merge results, or least causal rejection closure.

Live requests, post-merge requests, and conditional additive requests remain blocked. Synthetic qualification does not establish actual node interface support.

## Inputs and generation

The `casper-merge-accounting` binary provides `identity`, `run`, and `models` commands.

A request binds exact manifest bytes, compiled source digests, executable digest, candidate identities, deterministic seed, and three input artifacts.

The input artifacts contain configuration, execution fixture, and expected outcomes. Qualified capabilities bind provider, revisions, profile executable, and all compatibility labels.

Compatibility labels include execution schema, protocol epoch, accounting mode, record version, and token domain. Epoch values in fixtures are synthetic labels.

Generation emits a fixture-loading workload, fault requests, and a capture workload. Blocked generation emits no workloads or fault requests.

Each fault has a unique identity and earlier dependencies. The profile supports observed pause and delayed-delivery receipts within one declared node incarnation.

## Collection and classification

Execution identity is `(source_block_hash, execution_position)`. Deploy signature, admission identity, and context digest remain independent fields.

Repeated observations count once. Contradictory observations of one execution invalidate comparison. Admission rejection uses a separate admission identity and no execution position.

A failed body retains execution identity and settlement observations. Settlement amounts and aggregate outcomes use canonical unsigned decimal strings.

The profile compares causal edges, rejected execution inventories, and settlement data with pinned expectations. It does not calculate a replacement node policy.

Each amount remains in its declared token domain. Checked-arithmetic and overflow-rejection outcomes are pinned observations, not a proof of arithmetic correctness.

Missing measurements remain null with a reason. Missing inventory cannot produce a zero execution count.

Invalid measurements do not remove independent known failures. Invalid evidence takes verdict precedence over product failure and incomplete coverage.

Transport copies compare immutable event content before deadline and context quarantine. Only the transport record identity can differ between identical copies.

Applied fault receipts must match the declared action and dependencies. Dependent receipts use one producer and clock with increasing sequence numbers.

A snapshot must follow acknowledged faults. Requested scheduling and provider success alone cannot establish fault coverage.

Reports retain raw artifacts, qualification inputs, source identities, coverage, rejected observations, and product failures. Synthetic reports always record zero node launches and a nonpassing soak.

## Finite model

The model explores two scenarios with three execution observations each. Each scenario observes A, B, then A, with equal effect values.

The clean model checks execution multiplicity, settlement coverage, and compatibility classification. Three unsafe configurations must violate their exact named properties with TLC exit 12.

| Property | Executable binding |
| --- | --- |
| `MultiplicityMeasured` | `accounting_multiplicity` checks two independent executions and one duplicate. |
| `SettlementCoverageRequired` | `accounting_settlement_missing` checks unknown settlement and an incomplete verdict. |
| `CompatibilityLabeled` | `accounting_epoch_mismatch` checks invalid comparison across epochs. |

The abstraction assumes valid auxiliary fields and no independent product failures. Executable fixtures separately check malformed evidence, fault receipts, admission rejection, and failure retention.

The finite model does not establish unbounded behavior or live execution. Construction is not applicable under the reviewed harness approach.

## Verification

Set `TLA_TOOLS_JAR` to the pinned TLC JAR. Run the following command with a new output directory:

```bash
bash scripts/casper-soak/check-merge-accounting.sh target/accounting-check
```

Use `SOAK_ACCOUNTING_JAVA` to select Java. The runner requires the documented TLC digest and retains tool versions, fixture outputs, and model evidence.

The macOS verification uses `CARGO_PROFILE_TEST_OPT_LEVEL=0` for the recorded toolchain limitation. Linux verification uses separate static binaries in an isolated container.

Binding acceptance and the proposed workflow tag require human review. Passing these checks does not discharge the claim or complete TASK-017-8 automatically.

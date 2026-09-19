# TASK-017-9 Slashing Verification

## Scope and ownership

The user authorized TASK-017-9 implementation on 2026-09-19. The owner is `pi-casper-slashing`.

The starting revision is `79d09441fb8a8824dfac53d152dca18636efd5ce`. The worktree was clean before the claim update.

This task implements the controlled-transcript profile for CLAIM-CASPER-SOAK-006. It does not implement node slash authorization, evidence reconstruction, or bisimilarity proofs.

## Coordination

The carrier-index agent retains TASK-017-10. The preparation agent retains TASK-017-12 and TASK-017-14 preparation. TASK-017-11 remains unclaimed.

The claim update changes only TASK-017-9 ownership fields and this work log. Claim publication precedes executable implementation under the coordination agreement.

No commit or push is authorized by the implementation request alone. Publication of this claim awaits separate approval or a confirmed external publication.

## Requirements

The [claim](../claims/casper-soak-slashing.md) defines delivery, epoch correlation, and authorization measurements.

[D-09](../casper/design/decision-ledger/09-slashing-authorization.md) preserves the current truth table, rejected-slash recovery, activation-epoch protection, and inactive economic neglect slashing.

The profile compares observations with pinned fixture expectations. It does not promote supplementary reconstruction or deferred evidence formats to protocol authority.

## Implementation plan

- [x] Read the task, claim, interface requirements, and ratified decision.
- [x] Prepare the local ownership claim and notify the carrier-index agent.
- [x] Confirm publication of this two-file claim.
- [x] Implement separate Rust generation, collection, correlation, and verdict classification.
- [x] Add controlled transcripts for merge-lost slash, delivery permutations, rebond, stale epochs, missing evidence, forged deploys, and restart.
- [x] Verify observed delivery order instead of substituting requested order.
- [x] Quarantine unrelated epochs without suppressing independent product failures.
- [x] Detect authorization differences from pinned expectations without granting slash authority.
- [x] Add the bounded model and three named negative controls.
- [x] Run native and isolated Linux fixtures, exact inventory checks, model controls, and shared regressions.
- [ ] Retain source-bound evidence and failures under the current retention rule.
- [ ] Request separate binding acceptance and workflow-tag ratification.

## Intended files

- `scripts/casper-soak/src/profiles/slashing.rs`
- `scripts/casper-soak/src/bin/casper-slashing.rs`
- `scripts/casper-soak/tests/slashing.rs`
- `scripts/casper-soak/check-slashing.sh`
- `.github/workflows/casper-slashing.yml`
- `formal/tlaplus/casper_soak/profiles/slashing/`
- `docs/claims/casper-soak-slashing.md`

Existing accepted profiles, shared compiled sources, Cargo configuration, and the common auditor remain unchanged.

## Verification boundary

The initial model bound is two scenarios and three observations. Executable malformed-evidence checks supplement that finite model without expanding its proof scope.

Requested faults, applied receipts, candidate identity, evidence identity, parent pre-state, and epoch identity remain separate.

Expected rejection of stale or forged evidence can pass a scenario. A verifier counterexample instead demonstrates an intentionally defective harness rule.

Live and post-merge requests remain blocked. Synthetic qualification does not establish actual node interface support.

CLAIM-CASPER-SOAK-006 remains pending. No workflow tag, waiver, node campaign, or task completion is implied.

## Current state

The claim was published at `51615255aa4ad24779639120ea2a8c5ec0500d78`. Local HEAD and the remote branch matched before implementation.

An external glossary commit subsequently changed HEAD to `590dd1f7e0d201dc13b90ccc2e4db6740f13584b`. This task did not modify the glossary.

The controlled-transcript implementation and bounded model checks passed. The task remains in progress pending evidence retention, binding review, and workflow-tag ratification.

## Verification results

Native macOS and isolated ARM64 Linux fixtures each passed seven Rust tests, 66 named cases, and 72 CLI invocations.

The Linux container used an unprivileged user, no network, no host mounts, no added capabilities, and explicit process, memory, CPU, and timeout limits.

The native runner checked exact invocation identities, exit codes, verdicts, source stability, and all four model controls.

| Model control | Exit | Generated states | Distinct states |
| --- | ---: | ---: | ---: |
| Clean | 0 | 3281 | 1681 |
| Requested-order substitution | 12 | 891 | 483 |
| Epoch identity omission | 12 | 834 | 450 |
| Authorization mismatch suppression | 12 | 948 | 516 |

Each negative control violated only its configured named property. The clean search completed within the declared bound.

The shared binding, claim, manifest, and model regression tests passed. Clippy passed for the new binary and integration test.

The renamed-case runner control exited 1. The interrupted runner control exited 143. Both controls retained failed summaries rather than claiming completion.

The language-server check found no diagnostics. It confirmed five paths clean and left one path inconclusive.

## Retained failures and corrections

The initial suite rejected the test-only label `live_node` as invalid input. The shared manifest contract requires `node_observation`.

The corrected live fixture uses that label and produces a blocked verdict. The original failed run and source archive remain intact.

A regression exposed missing deadline binding. The request extended the manifest deadline, and the original implementation incorrectly returned a passing scenario.

The RED run exited 101. Its assertion expected CLI exit 2 but observed exit 0. The source archive preserves the failing implementation.

The fix requires exact equality between the request deadline and manifest deadline. The final suite verifies the refusal without changing shared source.

Additional checks reject future rebond epochs and unauthorized positive fixture expectations. Independent failures survive malformed sibling fields, missing receipts, and multiple snapshots.

## Review boundary

The synthetic fixtures test pinned comparisons. They do not establish a reviewed node truth table, actual interface qualification, or an observed node slash.

No accepted profile or shared compiled source changed. No node campaign, policy activation, evidence upload, claim discharge, or workflow tag is authorized by these checks.

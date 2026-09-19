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
- [ ] Confirm publication of this two-file claim.
- [ ] Implement separate Rust generation, collection, correlation, and verdict classification.
- [ ] Add controlled transcripts for merge-lost slash, delivery permutations, rebond, stale epochs, missing evidence, forged deploys, and restart.
- [ ] Verify observed delivery order instead of substituting requested order.
- [ ] Quarantine unrelated epochs without suppressing independent product failures.
- [ ] Detect authorization differences from pinned expectations without granting slash authority.
- [ ] Add the bounded model and three named negative controls.
- [ ] Run native and isolated Linux fixtures, exact inventory checks, model controls, and shared regressions.
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

The local claim is prepared. No executable, model, or existing claim specification changed during this claim-only step.

# TASK-017-11: Protocol and Phlo Profile

## Ownership

- Task: TASK-017-11, within EPIC-017.
- Implementer: `pi-soak-carrier-index-linux`.
- Started: 2026-09-19T15:57:08Z.
- Branch: `formal/soak-casper-consensus`.
- Starting revision: `fcc2fb22270d403757785f6955915e40dec3625d`.
- Claim: CLAIM-CASPER-SOAK-007.

The remote agent relayed the explicit user assignment in message `01a0ba62-47e7-7b0e-92cc-c60f15cdea37`. The working tree was clean before this ownership update.

## Scope

This task verifies the controlled-transcript protocol and Phlo profile. It does not change node behavior or prove node accounting.

The [claim](../claims/casper-soak-version-phlo.md) requires separate Casper protocol and accounting authority labels. Generated requests and captured observations must preserve both `phloLimit` and `phloPrice`.

[D-01](../casper/design/decision-ledger/01-protocol-version-authority.md) keeps protocol 7 separate from accounting authority 8. Actual activation requires its independent approval, supported build, and fresh genesis.

[D-12](../casper/design/decision-ledger/12-deploy-cost-limits.md) preserves both signed fields, minimum-price validation, prepayment, refund, and exhaustion behavior. Undefined multi-wallet funding remains blocked.

The profile compares observations with pinned fixture expectations. It does not calculate a new economic policy or grant protocol authority.

## Coordination

This session retains TASK-017-10. The slashing agent owns TASK-017-9 acceptance records and its tracker block. The preparation agent retains TASK-017-12 and TASK-017-14.

This ownership step changes only the TASK-017-11 tracker fields and this work log. It does not change claim specifications, shared compiled sources, accepted claims, or `.gitattributes`.

Executable implementation waits for publication of this ownership claim. The assignment does not authorize a commit or push. Each publication operation requires separate user consent.

The six-workflow bindings-script change remains outside this assignment. Live qualification, node deployment, and soak dispatch remain outside this task.

## Plan

- [x] Read the task, claim, and D-01/D-12 decisions.
- [x] Prepare the ownership claim and notify the remote agent.
- [ ] Confirm separately authorized publication of the ownership claim.
- [ ] Implement the separate Rust generator, collector, classifier, and binary.
- [ ] Add fixtures for version separation, signed-field preservation, field mutation, minimum-price boundaries, and settlement outcomes.
- [ ] Preserve unknown measurements and independent product failures.
- [ ] Add the bounded model and three named negative controls.
- [ ] Verify native and isolated Linux fixtures, exact evidence inventories, shared regressions, and hosted CI.
- [ ] Retain source-bound evidence outside Git under the current retention rule.
- [ ] Add pending canonical ledger records and their required compatibility symlinks.
- [ ] Request separate binding acceptance and workflow-tag ratification.

## Intended files

- `scripts/casper-soak/src/profiles/version_phlo.rs`
- `scripts/casper-soak/src/bin/casper-version-phlo.rs`
- `scripts/casper-soak/tests/version_phlo.rs`
- `scripts/casper-soak/check-version-phlo.sh`
- `.github/workflows/casper-version-phlo.yml`
- `formal/tlaplus/casper_soak/profiles/version_phlo/`
- `docs/claims/casper-soak-version-phlo.md`

The future workflow remains a proposal until human ratification. Cargo configuration, shared library registration, accepted profiles, and the common claim auditor remain unchanged.

## Verification boundary

The initial model bound is two scenarios and three observations per scenario. The controls target `VersionLabelsSeparate`, `BothPhloFieldsCaptured`, and `SettlementOutcomeClassified`.

Passing controlled transcripts cannot establish actual node interface support. Live and post-merge requests remain blocked in this implementation scope.

Claim 007 remains pending. No verifier result, binding acceptance, workflow tag, waiver, or task completion is claimed.

## Current state

The ownership claim is prepared locally. No executable, model, workflow, or claim specification changed during this step.

The next step is a separately authorized claim commit and push. Implementation must not start before the published checkpoint.

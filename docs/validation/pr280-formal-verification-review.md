# PR 280 Formal Verification Review

## Scope

This review covers the slow-deploy replay defect and the parent-selection correction.

The review does not change the active Rust fixes.

## Findings

### FV-1: Cold replay caused the observed finality stall

All nine valid archives have the same replay sequence.

The proposer skips replay for its self-created carrier. Four remote nodes start cold replay and do not finish before teardown.

Those four nodes complete no later replay. The finality stall follows the replay stall.

A finality recovery protocol cannot repair this failure because validators have not accepted the carrier.

### FV-2: The fork-choice Rocq bridge does not model the current comparator

`GuardBridge.v` models deploy count before block age. The Rust comparator uses earliest deploy height before deploy count.

The Rust comparator also prefers smaller root height. The Rocq comments describe the opposite order.

The strict-total-order theorem remains structurally relevant. Its stated code correspondence is not exact.

### FV-3: The fork-choice Rocq bridge does not model the novelty gate

The Rust fix excludes the current main parent from sibling selection.

The Rust fix also rejects siblings whose deploy signatures are covered by the main ancestry.

`GuardBridge.v::prefer_deploy_support` scans all scored parents. The model has no deploy-signature set or coverage relation.

The proof therefore does not discharge the new liveness guard.

### FV-4: The replay index needs an observational-equivalence claim

The optimized index must not remove a replay candidate that the full scan can match.

The optimized index must preserve the selected recorded `COMM`, not only candidate membership.

The current implementation iterates a `HashSet` of present produce hashes. This iteration does not preserve full-scan candidate order.

`get_comm_or_candidate` selects the first matching candidate. Therefore, candidate order is part of observable replay behavior.

The implementation needs a stable trace ordinal or a proof that all simultaneously matching candidates are observationally equivalent.

### FV-5: CI did not run two existing TLA+ models

`check-tla-invariants.sh` requires a module wrapper with the configuration basename.

Fork-choice and deploy-lifecycle configurations lacked these wrappers. Neither model appeared in `POST_FIX_CONFIGS`.

### FV-6: CI rebuilds only the slashing Rocq theory

The workflow describes a repository Rocq gate. The job only builds `formal/rocq/slashing`.

Fork-choice proof drift can therefore merge without a Rocq rebuild.

## Required claims

### Replay index

1. Every indexed `COMM` exists in `replay_data` with the same multiplicity.
2. Every `COMM` in `replay_data` appears under each recorded produce hash.
3. `rig`, `clear`, and removal preserve both index directions.
4. Every full-scan matching candidate appears in the narrowed candidate sequence.
5. The narrowed scan selects the same `COMM` as the full scan.
6. Hash collisions can add candidates but cannot remove valid candidates.

### Parent selection

1. A covered sibling cannot replace the main parent.
2. A promoted sibling contains at least one novel deploy signature.
3. A promoted sibling strictly beats the main score when the main branch scores.
4. The output remains a permutation of the input parents.
5. Canonical stage-one ordering makes the complete pipeline permutation invariant.
6. Signature coverage removes promotion pressure after the main ancestry covers the deploy.

## Verification plan

1. Update `GuardBridge.v` after the Rust correction is stable.
2. Add a replay-index model with an explicit trace order.
3. Add a pre-fix configuration that permits unordered candidate selection.
4. Add property tests that compare indexed and full selection for generated replay states.
5. Add operation-sequence tests for `rig`, removal, and `clear`.
6. Build fork-choice Rocq proofs in the pull-request gate.
7. Run fork-choice and deploy-lifecycle TLA+ configurations in the pull-request gate.

## Completion gate

Do not claim formal discharge for PR 280 until FV-2, FV-3, and FV-4 have evidence.

The convergence test supplies liveness evidence. It does not prove replay observational equivalence.

## Follow-up formal artifacts

The follow-up adds these mandatory claims:

- `docs/claims/consensus-cross-view-determinism.md`
- `docs/claims/fork-choice-convergence.md`
- `docs/claims/replay-liveness-bound.md`

The follow-up also adds bounded TLA+ models for recovery leadership, promotion convergence, and replay work.

The replay selection-equivalence claim remains pending. The formal work does not declare complete discharge.

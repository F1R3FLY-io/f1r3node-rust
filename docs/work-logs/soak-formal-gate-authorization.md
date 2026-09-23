# Formal gate enforcement authorization

## Approval

The user approved the conditional protection change on 2026-09-19:

> yes - that is approved and you can authorize the enforecment tests, and final acceptance as well to discharge  CLAIM-SOAK-GATE-001

The approval covers the following actions:

- Require `Formal verification gate` from GitHub Actions, integration `15368`, on `dev`.
- Preserve all existing required checks and protections.
- Activate the rule only after the verified gate is available on `dev`.
- Run enforcement tests against the effective configuration.
- Record final acceptance and discharge `CLAIM-SOAK-GATE-001` after all required checks pass.

Final acceptance is conditional on evidence. This approval is not a waiver, a successful enforcement result, or an immediate discharge.

Git commits, pushes, and merges still require their own authorization. Node campaigns, other branches, and unrelated task closure remain outside this approval.

## Activation prerequisite

The read-only check at `2026-09-19T23:18:22Z` found `dev` at `6940a5beb4aa806d3d75f6df3be9f238512fcc2f`.

That workflow does not contain `Formal verification gate`. The effective required-check rules do not require that check.

PR #436 remains open at `7509c831c881ae6bbd284a5f2121da0ce7ae5605`. The approval does not authorize its merge.

Activation therefore remains blocked on baseline availability. No protection rule changed, and no live enforcement test ran.

The [hosted renewal record](soak-formal-gate-hosted-renewal.md) retains the earlier verified execution and bounded binding renewal. Those results do not establish required-check enforcement.

## Execution conditions

1. Confirm that `dev` contains the reviewed gate and all required dependencies.
2. Verify the baseline source identities and applicable hosted results.
3. Read the complete effective rules and their source configurations again.
4. Preserve all existing check contexts, integration identities, strictness settings, and unrelated protections.
5. Apply only the authorized `dev` gate requirement.
6. Read back the effective rules and compare them with the retained pre-change snapshot.
7. Run positive and negative enforcement checks without merging test changes into `dev`.
8. Retain every result and its source, run, attempt, and rule identities.
9. Apply final acceptance only after every required obligation passes.

The enforcement checks must cover missing, stale, skipped, canceled, and failing results. A current successful result must satisfy the formal-gate requirement.

Verify prerequisite refusal and repository enforcement separately. A failure in an unrelated required check does not prove that the formal gate blocks a merge.

Local fixtures, administrative bypasses, and an overall blocked PR cannot substitute for rule-specific enforcement evidence. An inconclusive result keeps the claim pending.

Git operations needed for test fixtures still require explicit authorization. Do not create a successful commit or merge as a test of refusal.

## Records

The [claim](../claims/soak-formal-gate.md) records approved authority separately from pending execution and enforcement.

Read-only snapshots remain under `target/formal-gate-authorization-20260919-01/`. The authorization record does not modify earlier accepted reports or runtime source bindings.

`CLAIM-SOAK-GATE-001` and TASK-017-13 remain pending and in progress, respectively.

# TASK-019-4: Node claim verification

---
handoff_status: in_progress
execution_revision: 6ea6bf029dc57caf1e5fb512a0eba88a846e959a
claimed_by: pi-session-01a0afde-d35c-70a2-b8c9-39aa11cbdfca
next_steps:
  - Complete the isolated rebuild and retain source-bound results.
  - Resolve the missing formal evidence before claim acceptance.
  - Obtain explicit acceptance from one eligible maintainer.
  - Keep B2 planning blocked until the complete gate passes.
---

## Scope and authorization

The user requested source-bound verification and explicit acceptance of the Batch A and B1 claims. This gate precedes B2 planning.

The eligible maintainers are `@spreston8`, `@dylon`, `@metaweta`, `@jeffrey-l-turner`, and `@jltatbeach`. No maintainer has accepted this evidence package.

Acceptance must identify the reviewed revision, both claims, and the evidence package. A maintainer name or an earlier PR approval does not establish acceptance.

This work changes task metadata and verification evidence only. It does not authorize B2, production changes, a waiver, a commit, or a push.

## Intake

PR #447 targets `dev` and names revision `6ea6bf029dc57caf1e5fb512a0eba88a846e959a`. The checkout was clean at intake.

The two claim inventories contain 18 source and test files. Seven files have mandatory CbC tags.

The explicit-inventory strict audit returned exit 4. All seven mandatory records remain pending.

The shared ledger gate checks recorded status, not source digests or proof coverage. Its result alone cannot establish source-bound verification.

No observer-specific proof inputs appear in the formal directories or CI registrations. Refutation, construction, and binding remain pending.

Only Java appears among the checked verifier commands on the current PATH. Tool availability does not establish claim coverage.

The PR contains one approval from `@jltatbeach` for revision `799e2136adc6e0100b289945d9a5a6851e81c91f`. It does not identify the current revision or B1 evidence.

The permission queries for all five maintainers returned HTTP 403. The user-supplied roster remains recorded, but these queries do not verify current GitHub permissions.

## Isolated rebuild

The rebuild uses a Git source archive of the exact reviewed revision. It must not reuse host-built project objects or test executables.

The base image digest is `rust:bookworm@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922`.

The first image pull timed out after 120 seconds. The second pull completed with the pinned digest. Both logs remain retained.

Preparation installs the pinned Rust toolchain and obtains locked dependency sources. Compilation and tests must run without network access in a separate container.

The host already runs blockchain nodes and another memory-intensive service. Verification must not stop or modify those services.

Preparation has a 2 GiB memory limit and a 30-minute timeout. The planned rebuild has a 4 GiB limit and two CPU cores.

## Evidence and blockers

Bulk evidence is under `target/node-claim-gate-6ea6bf029-gQCpO2/`. Both claim files and all existing evidence records remain unchanged.

A passing rebuild cannot replace missing formal evidence or maintainer acceptance. No claim is discharged, waived, or accepted.

The tracker now places TASK-019-4 before TASK-019-3. Neither the task nor the epic is complete.

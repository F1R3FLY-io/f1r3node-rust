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

The source audit matched all 18 inventory files against the reviewed revision. All seven existing records match their source and claim digests.

The shared ledger gate checks recorded status, not source digests or proof coverage. Its result alone cannot establish source-bound verification.

No observer-specific proof inputs appear in the formal directories or CI registrations. Refutation, construction, and binding remain pending.

Only Java appears among the checked verifier commands on the current PATH. Tool availability does not establish claim coverage.

The PR contains one approval from `@jltatbeach` for revision `799e2136adc6e0100b289945d9a5a6851e81c91f`. It does not identify the current revision or B1 evidence.

The proposed acceptance reviewer is `@jltatbeach`. This proposal is not confirmation or acceptance. No review request has been sent.

The permission queries for all five maintainers returned HTTP 403. The user-supplied roster remains recorded, but these queries do not verify current GitHub permissions.

## Isolated rebuild

The rebuild uses a Git source archive of the exact reviewed revision. It must not reuse host-built project objects or test executables.

The base image digest is `rust:bookworm@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922`.

The first image pull timed out after 120 seconds. The second pull completed with the pinned digest. Both logs remain retained.

Preparation installs the pinned Rust toolchain and obtains locked dependency sources. Compilation and tests must run without network access in a separate container.

The host already runs blockchain nodes and another memory-intensive service. Verification must not stop or modify those services.

Preparation completed without a project build. It used a 2 GiB memory limit and a 30-minute timeout.

Each rebuild attempt uses a 4 GiB memory limit, two CPU cores, one Cargo job, and a 45-minute timeout. It uses a non-root user, no network, and no Linux capabilities.

The first image creation command timed out. A later command created the image successfully. Both outcomes remain retained.

The first build attempt stopped on a Cargo panic before compilation. Full dependency resolution reproduced the panic for `/vendor-config.toml` but accepted `/configuration/vendor.toml`.

The initial metadata probe excluded dependencies and did not reproduce the panic. That failed probe remains retained.

The second build attempt stopped on an unreadable vendored file. Six public dependency files required read permission for the non-root user.

The permission correction changed no source bytes. It did not grant root access to the rebuild. The third attempt started with another empty target directory.

The third attempt stopped during protobuf generation because the builder lacked `libprotobuf-dev` headers. The correction installed the version that matches the pinned `protobuf-compiler` package.

Direct protobuf checks passed for the model, node, and communication schemas. Only `libprotobuf-dev` and `libprotobuf-lite32` were added.

The fourth attempt resumes the third attempt's isolated target after it checks 4,880 retained file hashes. It reuses only objects built inside that earlier container.

Both attempts use the same source archive and Rust compiler. No host-built project objects or test executables were imported.

## New source finding

`CaptureLimits::validate` rejects a zero lock wait but accepts `Duration::MAX`. The lock wrappers pass that duration to `parking_lot::RwLock::try_read_for`.

The pinned library converts the duration with `Instant::now().checked_add`. An overflow produces `None`, which the slow lock path treats as an untimed wait.

This path conflicts with B1 properties 1 and 2. The four previous implementation corrections remain intact. This is a new acceptance finding.

The dependency source copies match their pinned package checksums. A probe against the independently rebuilt library is prepared but has not run.

No production correction has been made. A correction must reject invalid deadlines before guard access and prevent each lock call from receiving an untimed fallback.

## Metadata advance

Another writer committed the intake documents as `314368bfb7ec48c34e5eb7e5534fd1959152d346`. That commit changed only the tracker and this work log.

The original source-audit guard rejected the changed HEAD. Its script and failure remain retained. The revised audit verifies the exact metadata-only change and the original source hashes.

The rebuild still uses the archive of `6ea6bf029dc57caf1e5fb512a0eba88a846e959a`. No test execution is relabeled as an execution of the later commit.

The tracker claim time now uses the recorded intake timestamp. The acceptance task does not depend on completion of the B1 task that needs this review.

## Evidence and blockers

Bulk evidence is under `target/node-claim-gate-6ea6bf029-gQCpO2/`. Both claim files and all existing evidence records remain unchanged.

A passing rebuild cannot replace missing formal evidence or maintainer acceptance. No claim is discharged, waived, or accepted.

The DAG storage file also retains its existing `CLAIM-FINALITY-002` obligations. This gate cannot erase or waive those obligations.

The tracker now places TASK-019-4 before TASK-019-3. Neither the task nor the epic is complete.

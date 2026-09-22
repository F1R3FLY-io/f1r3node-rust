# TASK-019-4: Node claim verification

---
handoff_status: paused
execution_revision: 6ea6bf029dc57caf1e5fb512a0eba88a846e959a
correction_checkout_base: 4561e064a70b495fe07cbcf779bff375d636aaad
correction_working_tree: true
correction_source_manifest: docs/cbc-evidence/runs/casper-node-deadline-correction-4561e064a-01/sources.sha256
claimed_by: pi-session-01a0afde-d35c-70a2-b8c9-39aa11cbdfca
next_steps:
  - Resolve the missing formal evidence before claim acceptance.
  - Obtain explicit acceptance from one eligible maintainer.
  - Keep B2 planning blocked until the complete gate passes.
---

## Scope and authorization

The user requested source-bound verification and explicit acceptance of the Batch A and B1 claims. This gate precedes B2 planning.

The eligible maintainers are `@spreston8`, `@dylon`, `@metaweta`, `@jeffrey-l-turner`, and `@jltatbeach`. No maintainer has accepted this evidence package.

Acceptance must identify the reviewed revision, both claims, and the evidence package. A maintainer name or an earlier PR approval does not establish acceptance.

The initial verification changed task metadata and evidence only. The user subsequently approved the five-file correction below.

This approval does not authorize B2, a waiver, a commit, or a push.

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

Compilation completed. The node library passed 255 tests. The storage suites passed 301 tests, including 24 capture tests and 19 bounded-reader tests.

The raw interface executable contained 551,120,080 bytes, above the 536,870,912-byte observer limit. The interface suite recorded six passes and twelve failures because the guard rejected this oversized artifact.

A separate debug-stripped copy contained 18,853,968 bytes. Every allocated section and every program header remained unchanged. The original executable remains retained.

The derived interface suite recorded 17 passes and one failure. A single-thread repeat produced the same result. The isolated test gate did not pass.

The retained test transcripts contain 596 passes and 14 failures across all attempts. These counts include repeated tests and are not a successful qualification total.

All verification containers have stopped. The actual build cgroup limits were 4 GiB memory, zero swap, 512 processes, and two CPU cores.

The initial cgroup probes used incorrect mount-relative paths. The successful check read the actual build process's cgroup from the host.

## New source finding

`CaptureLimits::validate` rejects a zero lock wait but accepts `Duration::MAX`. The lock wrappers pass that duration to `parking_lot::RwLock::try_read_for`.

The pinned library converts the duration with `Instant::now().checked_add`. An overflow produces `None`, which the slow lock path treats as an untimed wait.

This path conflicts with B1 properties 1 and 2. The four previous implementation corrections remain intact. This is a new acceptance finding.

The dependency source copies match their pinned package checksums. The probe compiled against the independently rebuilt `block_storage` and `shared` libraries.

The five-second control passed validation and had a deadline. The zero-duration control failed validation. `Duration::MAX` passed validation but had no representable deadline.

The probe exited 101 at the assertion that this invalid duration must be rejected. The probe did not start a blocking lock operation.

The wrapper exited zero because it verified the expected counterexample and unchanged library hashes. That wrapper result is not a passing claim or formal refutation tier.

The initial verification made no production correction. The required correction must reject invalid deadlines before guard access and prevent an untimed fallback.

## Interface test finding

The capabilities test hashes its executable after the handshake. Its session deadline is 500 milliseconds.

The debug hash measurement took 2,442, 2,441, and 2,439 milliseconds. Each measurement excluded the file read and used the independently rebuilt crypto library.

The test then received `UnexpectedEof`. The expired session is consistent with the enforced deadline, not evidence that the observer accepted a late request.

The proposed correction moves the expected hash calculation before the handshake. It must not increase the production limits or suppress the deadline check.

## Approved correction scope

The user approved the next step on 2026-09-22. The correction uses these five source and test files:

- `block-storage/src/rust/dag/soak_snapshot.rs`
- `block-storage/src/rust/dag/block_dag_key_value_storage.rs`
- `block-storage/src/rust/dag/block_metadata_store.rs`
- `block-storage/tests/soak_snapshot.rs`
- `node/tests/soak_observer.rs`

The B1 correction must validate a deadline before guard access and pass checked deadlines to the three lock acquisitions. The interface correction changes test setup only.

Formal evidence and explicit maintainer acceptance remain separate requirements after those corrections.

## Metadata advance

Another writer committed the intake documents as `314368bfb7ec48c34e5eb7e5534fd1959152d346`. That commit changed only the tracker and this work log.

A later external commit, `4561e064a70b495fe07cbcf779bff375d636aaad`, changed the same two documents. Neither commit changed the claim inventories, claims, dependencies, tags, or evidence records.

The original source-audit guard rejected the changed HEAD. Its script and failure remain retained. The revised audit verifies the exact metadata-only change and the original source hashes.

The rebuild still uses the archive of `6ea6bf029dc57caf1e5fb512a0eba88a846e959a`. No test execution is relabeled as an execution of the later commit.

The tracker claim time now uses the recorded intake timestamp. The acceptance task does not depend on completion of the B1 task that needs this review.

## Initial evidence and blockers

Bulk evidence is under `target/node-claim-gate-6ea6bf029-gQCpO2/`. The [verification report](../cbc-evidence/runs/casper-node-claim-gate-6ea6bf029-01/report.json) records the blocked result.

Both claim files and all existing evidence records remain unchanged. The final explicit-inventory strict audit returned exit 4 with seven gaps.

The first image-creation timeout has no observed process exit code. Its earlier inferred exit annotation remains retained and is explicitly excluded from exit evidence.

A passing rebuild cannot replace missing formal evidence or maintainer acceptance. No claim is discharged, waived, or accepted.

The DAG storage file also retains its existing `CLAIM-FINALITY-002` obligations. This gate cannot erase or waive those obligations.

The tracker now places TASK-019-4 before TASK-019-3. Neither the task nor the epic is complete.

## Correction verification

The correction rejects a lock wait without a representable deadline. All three lock acquisitions receive the same checked `Instant` through `try_read_until`.

The duration remains in timeout errors and canonical identity. The correction adds no public interface, dependency, storage format, socket operation, or production write path.

Two new integration tests failed against the original implementation. The retained test process exited 101 with both expected assertion failures.

A unit test holds each write guard in turn. It checks refusal at the supplied deadline, release of an earlier guard, and success after release.

The corrected storage build passed 304 test executions. These include 26 capture tests, 19 reader tests, and the new three-guard unit test.

The capabilities fixture now computes its expected executable hash before observer setup and the handshake. Production limits and session deadlines remain unchanged.

The rebuild uses the prior isolated tool image and a copy of its isolated build cache. It verifies all 13,034 cache files before compilation.

The source archive contains the original execution revision plus the five-file correction. No host-built project objects enter the rebuild.

The first regression attempt refused an incorrect source manifest and a missing descriptor placeholder. The second stopped because the container lacked its external Cargo configuration mount.

Both setup failures remain retained. The third regression attempt reproduced the two expected test failures.

The first new audit used commas instead of spaces between file paths. Its empty result is invalid evidence, not a passing gate.

The corrected audit explicitly checks seven mandatory files. It returned exit 4 with seven pending records.

The original blocked package and staged index remain unchanged. The new bulk evidence is under `target/node-deadline-correction-4561e064a-CDrmyQ/`.

The [correction report](../cbc-evidence/runs/casper-node-deadline-correction-4561e064a-01/report.json) records 577 passing test executions. These include 255 node-library tests and 18 interface tests, in addition to the storage tests.

One interface helper remains ignored when invoked directly. The corrected capabilities test passes without any extension of its session deadline.

The raw interface executable contains 551,123,280 bytes and remains above the observer limit. The new suite uses a separate 18,853,968-byte debug-stripped copy.

All allocated sections and program headers match between the two artifacts. Their whole-file hashes differ, and the report records both identities.

Strict Clippy, the workspace check, and formatting checks passed in the same isolated container. The container exited zero without an out-of-memory termination.

LSP checks remain incomplete because some requests timed out. An informational Rust 2024 suggestion concerns unchanged code.

Four pending evidence records now bind the corrected bytes. Their previous versions remain in the retained archive, and all seven records remain pending.

Both claims, all verification tiers, and maintainer acceptance remain pending. No formal proof ran, and no acceptance request was sent.

The source correction remains uncommitted. TASK-019-4 and B2 planning remain blocked, and TASK-019-6 still requires the minimum-code review before merge.

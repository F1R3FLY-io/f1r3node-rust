# TASK-019-4: Node claim verification

---
handoff_status: paused
execution_revision: 6ea6bf029dc57caf1e5fb512a0eba88a846e959a
correction_checkout_base: 4561e064a70b495fe07cbcf779bff375d636aaad
correction_working_tree: true
correction_source_manifest: docs/cbc-evidence/runs/casper-node-deadline-correction-4561e064a-01/sources.sha256
claimed_by: claude-session-7015f552
previous_claimed_by: pi-session-01a0afde-d35c-70a2-b8c9-39aa11cbdfca
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

The source correction was uncommitted at that verification checkpoint. External commit `de93425ee9cbc72a6509de21b0eb009a07eb48a7` subsequently included it.

TASK-019-4 and B2 planning remain blocked. TASK-019-6 still requires the minimum-code review before merge.

## Current verification intake

The user requested completion of TASK-019-4 after the applicability policy update. The new intake started at `27afe82365465b9b63d5d3f09f4ad75a4a0862d3`.

All 23 files in the correction input manifest still match their recorded hashes. This comparison does not execute the current revision.

The explicit-inventory strict audit again returned exit 4, with seven mandatory files and seven pending records. No claim status or evidence record changed.

The PR API showed PR #447 open against `dev`, with head `27afe82365465b9b63d5d3f09f4ad75a4a0862d3`. Its only returned review approved the historical revision `799e2136adc6e0100b289945d9a5a6851e81c91f`.

No current acceptance or observer-specific formal input was found. Java and Docker are available. Rocq, opam, and Kani commands are absent from the current PATH.

Another writer advanced HEAD to `3b1d2465a` and changed the tracker during this review. Those changes affect `docs/ToDos.md` only and remain untouched.

The language-server symbol query returned no results. Structural outlines and direct source reads supplied the source map below. This review does not establish clean diagnostic coverage.

Bulk intake evidence is under `target/node-claim-verification-27afe8236-ILAOnB/`. It contains the audit, input check, API responses, revision records, and external tracker patch.

### Challenge freshness finding

Batch A properties 5 and 6 require a fresh challenge and prohibit reuse across sessions. `Observer::session` uses `Uuid::new_v4()` without recording prior challenges.

The pinned `uuid` version is `1.24.0`. Its constructor masks random bits to set the UUID version and variant. It does not enforce uniqueness.

Two sessions in one observer can receive equal random values. The earlier request then has the same challenge and the same checked identity fields.

The request validator does not compare the request with the event sequence. The existing replay test samples two challenges and asserts that they differ.

This source review identifies a proof gap, not an observed random collision or an executed replay counterexample. No production correction has been made.

A model must permit repeated random outputs unless the specification supplies a justified assumption. It must not silently replace randomness with guaranteed freshness.

The proposed correction must enforce challenge separation within an observer lifetime. Cross-incarnation freshness also needs an explicit domain and reviewed assumptions.

### Property coverage plan

This plan maps all ten Batch A properties and all thirteen Batch B1 properties. Property numbers refer to the existing claim files.

Every classification below is provisional. No maintainer has reviewed this matrix, and no property receives `construction: not-applicable`.

`U` proposes construction for a property over arbitrary permitted states or histories. `F` identifies a possible finite predicate within a larger property.

An `F` entry still requires complete-domain coverage and an abstraction-preservation argument. It does not establish that the complete property is bounded by design.

All rows require Rust binding evidence. The existing tests supply candidate regression coverage, not a complete correspondence proof.

#### Common domains and assumptions

Batch A includes disabled startup, invalid configurations, accepted configurations, rejected peers, arbitrary request bytes, disconnects, expiry, and normal shutdown. Every request includes rejected requests.

The specified limits include 4,096 configuration bytes, 1 MiB frames, 4,096 sessions, and session timeouts from 50 through 30,000 milliseconds. Rejection cases include values outside these limits.

Path, process, executable, and public-field checks retain every bound in the claim. Unrelated node configuration fields remain variable when testing disabled behavior and public serialization.

Batch A retains its declared kernel, process, filesystem, and owner trust boundaries. Scheduling remains an explicit deadline assumption, not a hard execution-time guarantee.

Batch B1 includes every permitted DAG, request, limit configuration, backend, row encoding, and writer history. It includes failures, restored values, and writes outside the insertion-generation mechanism.

Capture limits vary by request. No small DAG fixture or finite machine integer establishes a fixed specification domain for these properties.

Batch B1 retains its LMDB identity, transaction, reader-slot, and native-I/O assumptions. Codec behavior and persistent collection behavior also require explicit dependency correspondence.

The proof must distinguish assumed library behavior from verified Rust behavior. A theorem over abstract transitions does not establish that Rust implements those transitions.

No TLC instance bounds are selected or verified yet. Each future configuration must state its bounds independently of the property domain.

#### Batch A source and evidence map

| Property | Domain, bounds, and proposed classification | Rust boundary | Required correspondence and remaining gap |
| --- | --- | --- | --- |
| A1 | U covers all disabled startup histories and unrelated node configurations. | `Observer::bind`, `NodeRuntime::start`, and configuration defaults. | The disabled test covers socket absence. Runtime task and journal absence still need a transition argument and a shutdown-aware harness. |
| A2 | F covers explicit activation predicates and fixed field limits. U covers configuration routes and activation histories. | `validate_config`, `Observer::bind`, CLI mapping, and configuration builder. | Existing limit and precedence tests need an independent configuration oracle and complete boundary coverage. |
| A3 | U covers accepted directory paths, ancestor states, symlinks, ownership, and permitted namespace changes. | `safe_directory` and socket setup. | Permission tests need a path-state model under the declared owner and root assumptions. |
| A4 | U covers peer arrivals and process replacement histories. | `process_start_ticks`, bind-time checks, and `Observer::session`. | Existing foreign-process and start-identity tests need matching process-lifetime transitions in the oracle. |
| A5 | U covers every request identity and observer lifetime. | `Request`, `Identity`, and `Observer::session`. | Model each identity comparison independently. Resolve random challenge and incarnation assumptions before claiming freshness. |
| A6 | U covers accepted and rejected session histories within each configured budget. | `Observer::session` and `Observer::run`. | Retain one-request and replay controls. Add repeated-random-output coverage instead of assuming unique UUIDs. |
| A7 | F covers the fixed frame predicate. U covers partial I/O, disconnects, expiry, and scheduling histories. | `Observer::write`, `Observer::session`, and `timeout_at`. | Exact-frame and deadline tests need a reference clock and I/O model. No host scheduling bound is claimed. |
| A8 | F covers the operation enum and capability fields. U covers all request histories and side effects. | `Operation::Capabilities` and the response construction. | Compare complete responses and effect traces. Unsupported operations must not enter evaluation, fault control, or store writes. |
| A9 | U covers normal shutdown, cancellation, filesystem failures, and replacement objects. | `SocketGuard::drop`, `RunningObserver::stop`, and runtime exit ordering. | Replacement and cancellation tests exist. Source ordering does not replace an end-to-end normal-shutdown test. |
| A10 | F covers the fixed public allowlist. U covers every node configuration and serialization path. | The public JSON value and `Identity` serialization in `Observer::bind`. | Secret-change tests need an independent allowlist oracle. Digest equality alone does not prove exclusion of every private field. |

#### Batch B1 source and evidence map

| Property | Domain, bounds, and proposed classification | Rust boundary | Required correspondence and remaining gap |
| --- | --- | --- | --- |
| B1 | U covers every limit configuration and request, including invalid arithmetic and deadlines. | `ReadLimits::validate`, `CaptureLimits::validate`, and `capture_observed`. | Retained invalid-limit regressions need an independent validation oracle and arithmetic harnesses. |
| B2 | U covers global, metadata, and DAG-state lock contention under the same checked deadline. | `soak_capture_access` and `capture_state`. | The three-guard regression needs a lock-state model with timeout and partial-acquisition release controls. |
| B3 | U covers participating stores and their environment partition. | `BoundedLmdbReader::open`. | Existing shared-transaction and separate-environment tests need a partition oracle, including open failures. |
| B4 | U covers environment paths and commits before or during transaction open. | `BoundedLmdbReader::open` and `identities`. | Verify equality at open with explicit transaction semantics. A fixture without a racing commit does not cover that interval. |
| B5 | U covers raw bytes, nested encodings, scan histories, and all supplied decode limits. | Reader charging, `preflight_metadata`, and `decode_block_bounded`. | Retain length, overflow, work, and short-decompression controls. Bind the actual pre-allocation arithmetic to Kani harnesses. |
| B6 | U covers captured state shapes, guard histories, and all capture call paths. | `capture_state` and `capture_observed`. | Check copy timing and forbidden effects. Persistent collection sharing requires an explicit dependency argument, not pointer equality. |
| B7 | U covers commits in every participating environment through its validation observation, including restoration of earlier bytes. | `BoundedLmdbReader::validate`. | Retain restored-value and separate-environment controls. Prove the transaction rule without equating it with atomic cross-environment publication. |
| B8 | U covers insertion-generation histories and writers that do not change generation. | The two generation reads in `capture_observed`. | Existing unchanged-generation writer tests need a combined generation and transaction oracle. |
| B9 | U covers held membership, requested bodies, metadata, and optional cache rows for every permitted DAG. | Metadata, body, floor, and frontier capture branches. | Compare every availability result with an independent row oracle. Missing required rows must remain errors. |
| B10 | U covers successful and rejected captures and subsequent caller actions. | Reader ownership, `validate(self)`, guard release, and `DetachedDagSnapshot::seal`. | Phase and store-byte tests need failure-path resource checks. Exclude caller-injected writes from claims about capture effects. |
| B11 | U covers captured collections, parent order, limits, transaction records, and availability states. | `CanonicalEncoder`, `SnapshotData::encode`, and `DetachedDagSnapshot::seal`. | Use an independent canonical encoder and field-mutation controls. Hash identity alone does not prove semantic completeness. |
| B12 | U covers captured rows, construction errors, multiple scratch views, and subsequent mutations. | `DetachedDagSnapshot::scratch_view` and `EXCLUDED_STORES`. | Extend independence tests with a store-construction oracle and failure controls. Verify every mutable store, not only one cache. |
| B13 | F covers backend classification. U covers arbitrary backend implementations and store lists. | `lmdb_store`, `BoundedLmdbReader::open`, and `session_for`. | Unsupported and foreign-store tests need proof that rejection precedes fallback operations. |

The existing DAG finality claim remains outside any new exemption. Its mandatory artifact record must retain all prior obligations.

### Proposed implementation scope

The repository requires file-scope confirmation before code changes. The following verification scope remains a proposal, not an approval.

The first formal files would be under `formal/tlaplus/node_observation/`:

- `ObserverSession.tla`
- `BoundedCapture.tla`
- `MC_ObserverSession.cfg`
- `MC_BoundedCapture.cfg`
- `README.md`

The negative-control configurations would use `MC_ObserverSession_<case>_pre_fix.cfg` and `MC_BoundedCapture_<case>_pre_fix.cfg`.

The proposed observer cases are `activation`, `directory`, `peer`, `identity`, `freshness`, `single_request`, `frame`, `deadline`, `budget`, `effects`, `cleanup`, and `public_config`.

The proposed capture cases are `limits`, `locks`, `transactions`, `open_identity`, `allocation`, `copy`, `environment`, `generation`, `incomplete`, `release`, `canonical`, `scratch`, and `backend`.

Each configuration must isolate its named defect class. Additional classes would require an explicit scope update rather than an undocumented omission.

The Rocq project would use these files under `formal/rocq/node_observation/`:

- `_CoqProject`
- `README.md`
- `theories/ObserverSession.v`
- `theories/BoundedCapture.v`
- `theories/MainTheorem.v`

The existing Rust files proposed for binding tests, arithmetic harnesses, and the smallest necessary freshness correction are:

- `node/src/rust/soak_observer.rs`
- `node/tests/soak_observer.rs`
- `shared/src/rust/store/soak_snapshot.rs`
- `shared/tests/soak_snapshot.rs`
- `block-storage/src/rust/dag/soak_snapshot.rs`
- `block-storage/src/rust/key_value_block_store.rs`
- `block-storage/tests/soak_snapshot.rs`

Arithmetic harnesses must call the same predicates as production decoding. A duplicate test-only predicate does not establish binding.

The proposed gate files are `scripts/ci/check-tla-invariants.sh`, `scripts/ci/test-check-tla-invariants.sh`, and `scripts/ci/check-formal-invariants.sh`. A new `scripts/ci/check-node-observation-bindings.sh` would run the dedicated Rust binding checks.

The proposed workflow change is `.github/workflows/slashing-tests.yml`. It would invoke those checks without changing branch protection or unrelated formal-gate delivery.

Claim inventories, `.gitattributes`, and pending evidence records must include the approved verification artifacts before implementation. The proposed new mandatory tags use high weight and still require ratification.

The two claim files, this work log, and the task tracker would record the reviewed coverage and results. Compact evidence would use a new run directory.

Verifier setup would use resource-limited containers, not host package installation. Rust builds would retain isolated-cache provenance and exact input hashes.

No B2 planning, B2/C implementation, cleanup deletion, campaign launch, merge, commit, or push is included. Maintainer acceptance remains a separate gate after successful verification.

### Scope confirmation and first cycle

The user confirmed the proposed scope and mandatory tags. This confirmation authorizes implementation, not claim acceptance or a Git operation.

The first cycle addresses repeated random challenges within one observer lifetime. The private session function now accepts an entropy function for deterministic testing.

Production still passes `Uuid::new_v4`. The regression supplies `Uuid::nil` twice and submits the first request in both sessions.

The random-only implementation failed the required-refusal assertion with exit 101. The counter-exhaustion control passed in the same run.

This RED execution used a behavior-preserving private test seam, not an unchanged historical executable. Its exact source and executable remain retained.

The correction appends the checked hello-event sequence to the random UUID. The request validator compares the complete challenge string.

The correction does not depend on random outputs being unique within an observer lifetime. Cross-incarnation uniqueness remains unresolved and is not covered by this theorem.

An independent integration oracle checks event sequences across acceptance, replay, and disconnect. The regression and oracle invoke production session or transport code.

The corrected isolated run passed 580 test executions. These comprise 304 storage tests, 257 node-library tests, and 19 interface tests.

One interface helper remains ignored when invoked directly. Strict Clippy, the workspace check, and formatting passed in the same container.

The build reused 21,779 hash-verified files from the isolated RED cache. It imported no host-built project objects and was not a fresh-target build.

The raw interface executable contains 551,199,488 bytes. The executed debug-stripped derivative contains 18,952,880 bytes.

Allocated sections and program headers match. Whole-file hashes differ, and both identities remain recorded.

The clean challenge model passed TLC with 199 generated states and 127 distinct states. Its bounds are three sessions and two random values.

The negative control exited 12 on `FreshChallenges`. Its trace repeats one random value across two sessions.

Rocq 8.16.1 compiled four allocation theorems. `coqchk` passed, and all four assumption checks reported `Closed under the global context`.

These results prove the stated allocation model, not the complete interface or capture claims. The area README keeps all 23 property classifications provisional.

The actual bounded TLC gate passed 14 positive configurations and 62 expected violations. The registry fixture suite also passed with 62 controls.

The dedicated binding driver checks that the new tests exist before execution. It preserves raw executables and verifies the debug-stripped interface derivative.

A separate isolated driver run passed 276 executions. These repeat the 257 node-library tests and 19 interface tests, rather than adding new behaviors.

That run checked 23,304 isolated-cache files before compilation. It used the retained vendor configuration as Cargo's global configuration.

The workflow adds a binding job without changing existing job names or branch protection. Two existing opam command substitutions now use quotes.

No hosted workflow ran, and no maintainer acceptance request was sent. The full existing Rocq suite was not rerun locally.

### Evidence and remaining work

The [cycle report](../cbc-evidence/runs/casper-node-challenge-freshness-3b1d2465a-01/report.json) records partial evidence, not discharge.

Bulk evidence is under `target/node-observation-verification-3b1d2465a-NIXDOR/`. Earlier reports and source archives remain unchanged.

Retained failures include the first Rocq tactic error and an incompatible host Java runtime. Container-installed Java resolved the runtime mismatch.

The first registry fixture copied ignored historical TLC state files and exhausted its temporary filesystem. Later attempts exposed a non-executable temporary mount.

The successful fixture used the source archive without ignored state files and an executable temporary mount. Those setup failures do not count as counterexamples.

The index changed during this work. The assistant issued no staging or commit command and did not restore the earlier index.

Active diagnostics did not confirm seven changed files as clean. Two checks timed out, and five servers could not confirm a clean result.

The remaining YAML length findings concern legacy lines. The added job passes Actionlint after the two opam quoting corrections.

Both claims remain pending. The added verification artifacts also remain pending, and existing finality and formal-gate obligations remain intact.

Remaining work includes cross-incarnation identity, the other interface properties, all capture properties, Kani arithmetic harnesses, and complete Rust correspondence.

Named maintainer review must cover every applicability decision and the final source-bound package. B2 planning and task completion remain blocked.

## Canonical model reconciliation

The user confirmed reconciliation after the downstream branch identified two conflicting model sets. The node branch now owns one canonical set.

The node base is `10e7b8452824e12a1fe2743dca7989b79fce2133`. Downstream `8a379f05a07974ae9af6b450e6cea5fa6da80e0f` identifies imported model inputs, not the evidence base.

The [reconciliation package](../cbc-evidence/runs/casper-node-model-reconciliation-10e7b8452-01/report.json) records the working-tree inputs above that node base. It does not cite the harness merge as its source base.

### Canonical set and registration union

`ObserverSession.tla` combines sequence-based challenge allocation with the broader session predicates. Repeated random outputs remain permitted.

`BoundedCapture.tla` retains the imported capture transition system. Entry modules share these two state machines rather than defining alternative implementations.

The configuration inventory contains two positive cases and 17 negative cases. The negative cases preserve all 16 downstream controls and the node freshness control.

The gate checks exact agreement between registrations, the JSON plan, and configuration files. It checks the complete canonical set in both gate tiers.

The gate also preserves stricter output classification. Incomplete searches, absent traces, duplicate violations, unrelated errors, and contradictory output fail verification.

The new fixtures fail against the old node gate with exit 1. That gate accepted a negative control without a trace.

The retained failure is a gate regression, not a model counterexample. Its wrapper exits zero only after checking the expected failed assertion.

### Claim-record separation

The node package owns the node-specific gate registration view. That view binds both node claim digests and the current gate source.

The default gate record retains its legacy governance claim and historical source identity. It no longer embeds node claim digests or node acceptance metadata.

The harness keeps its own primary record under `docs/casper/cbc-evidence/`. Downstream integration must not replace that record with the node view.

A gate source change can require a harness renewal. An unrelated node claim edit must not require that renewal merely through a shared digest field.

The workflow record uses the same separation. Existing finality and governance obligations remain pending and unchanged in meaning.

### Checker ownership and binding limits

This branch adds no standalone checker crate, workspace, or lockfile. The existing gate and node binding driver execute the checks.

The downloaded downstream crate was inspected only in ignored evidence storage. It was not added to the node tree or executed.

Downstream must remove that crate or place it under its supply-chain audit before acceptance. Its dependency issue is not resolved by this node reconciliation.

The downstream binding parser hardcodes 18 interface tests and two storage unit tests. This node revision instead has 19 interface tests and one capture-specific storage unit test.

The canonical map also names supplemental node unit tests and records binding gaps. Downstream tooling must preserve those distinctions instead of silently dropping them.

The map covers all 23 required property numbers and 64 test references. The references were checked against source declarations, not treated as semantic proofs.

The deterministic generation-rejection test exists only downstream. The node map records this B8 gap and retains the available partial tests.

No Rust implementation, Cargo manifest, Cargo lockfile, or supply-chain policy changed in this reconciliation. No B2 or campaign operation was added.

### New verification results

The session model passed with 745 generated states and 689 distinct states. The capture model passed with 20,376 generated states and 10,066 distinct states.

All 17 node negative controls failed on their named invariants. The full bounded gate passed 15 positive configurations and 78 expected violations.

The fixture suite passed with 78 registered controls. It also checked plan disagreement, duplicate entries, and unregistered controls in both node families.

The four existing Rocq theorems rebuilt successfully. Kernel checking passed, and every assumption query reported `Closed under the global context`.

The action projection to those theorems is documented, not machine-checked. The four theorems do not prove the additional session or capture properties.

The isolated binding driver passed 257 node-library tests and 19 interface tests. One interface helper remained ignored when invoked directly.

The driver checked 23,314 isolated-cache files before compilation. It imported no host-built project objects and did not use a fresh target directory.

The driver recorded both raw and debug-stripped executable identities. It checked allocated sections, program headers, source hashes, and executable hashes.

This cycle did not rerun the storage suites, workspace checks, or Clippy. Their earlier results remain historical evidence rather than new execution.

Five active language-server checks reported no diagnostics for the changed shell and JSON files. The model and theorem compilers supplied separate formal checks.

The STE check uses baselines for unchanged claim and tracker prose. Initial sentence, paragraph, semicolon, and baseline-coverage failures remain in the evidence.

The index changed externally during verification, but HEAD remained at the node base. The assistant issued no staging, commit, merge, or push command.

Both claims remain pending. All applicability decisions and complete Rust correspondence still require named maintainer review.

B2 planning remains blocked. Downstream integration must take the canonical model files, preserve the Rocq and binding registrations, and combine gate registration lists.

## Handoff to claude-session-7015f552 on 2026-09-23

The user handed TASK-019-4 to this session at `8789c1c3e`, the merge of `dev` into the branch. The previous session's evidence packages and records remain unchanged.

The merge changed `block-storage/src/rust/dag/block_dag_key_value_storage.rs`. Its record digest is stale until the next refresh.

Planned order: construction proofs for the capture and cross-incarnation properties, Rust binding evidence, a TLC rerun of all 19 configurations, the applicability review for maintainer sign-off, then record refresh and a gate package.

Verifier tools run in resource-limited containers. No host package is installed. No production limit changes without a recorded finding.

## Handoff cycle results on 2026-09-23

### Refutation tier

The pinned TLC jar was fetched at the CI release and verified against the CI digest. The host Java 8 runtime executed it without the runtime mismatch that the previous container run recorded.

Both positive configurations passed: `MC_ObserverSession` with 689 distinct states and `MC_BoundedCapture` with 10,066 distinct states. All 17 node controls exited 12 on their named invariants.

The full gate in pull-request tier passed 15 clean configurations and 78 expected violations with exit zero. The registry fixture suite passed separately.

### Construction tier

The pinned `coqorg/coq:8.16.1` image is amd64 only, and this host is arm64. A local verifier image built from Debian bookworm supplies Rocq 8.16.1 and a Java 17 runtime.

The `BoundedCapture` module adds a transaction clock, capture observations, the insertion generation, charge budgets, length prefixes, and the capture protocol state machine. The `ObserverSession` module adds incarnation-qualified tokens.

`MainTheorem` now exports 14 theorems. The build passed, `coqchk` reported that the modules were successfully checked, and all 14 assumption sets reported `Closed under the global context`.

Two tactic corrections were needed. A goal-count error in the interference proof and a folded `shape` definition in the guard-order lemma were resolved by explicit terms and an explicit unfold. Both failing runs remain in the retained container logs.

The formal gate registration now expects 14 closed sets. The container ran with 2 GiB of memory, two CPUs, 256 processes, and no network.

### Binding tier

The capture oracle test hand-translates the `BoundedCapture` admission, completeness, and validation predicates. It runs production capture under seven interference kinds with and without a requested body, and requires equal outcomes across all 14 scenarios.

Seven Kani harnesses remain under `#[cfg(kani)]`: six in the shared reader for the length prefix, limit comparison, and atomic charging, and one in the block store for decode-limit validation. A standalone checked-total harness timed out twice at 15 and 25 minutes and was removed, because both charging harnesses drive the same arithmetic with fully symbolic inputs and verified, and the Rocq overflow theorem covers the same property. A first full-crate run stalled for over an hour on the formatted error paths, so the prefix check was factored into a pure `split_length_prefixed` predicate that production and the harnesses share, the decoder harness was dropped, and the harnesses run one at a time with timeouts. Both crates carry the same `unexpected_cfgs` allowance the casper crate uses.

The Kani driver needs a glibc newer than Debian bookworm provides, so the harness run uses an Ubuntu 24.04 container with a fresh Rust toolchain. Six shared harnesses verified: the limit comparison, both atomic-charging harnesses, and all three prefix harnesses. The block-store harness could not execute because a hashing dependency in that crate tree requires aes and neon target features that the Kani compiler invocation does not pass. It remains authored, and unit tests cover its property.

Lint with warnings denied and formatting passed. The shared and block-storage suites passed with 27 capture tests and 19 reader tests.

### Applicability review

The area README now classifies all 23 properties with a refutation, construction, binding, and decision column. Five properties propose a bounded-by-design classification with a named finite domain: A1, A2, A8, A10, and B13.

Twelve properties have recorded construction theorems. A3, A4, A9, B9, B11, and B12 remain pending with Rust tests only, and A7 and B2 keep pending deadline parts.

No classification is accepted. Every decision awaits a named maintainer, and no claim status changed.

### Records and package

The dev merge changed the DAG storage file, and this cycle changed the Rocq project, the shared reader, the block store, the capture tests, the formal gate, and the bindings manifest. Every affected record is refreshed at this revision, and the new capture module receives its first record.

The strict audit returned exit 4 before the refresh with the new module unrecorded. It is expected to return exit 4 after the refresh with every mandatory record pending, which is the correct state before acceptance.

The package `casper-node-claim-gate-03d7f1b27-01` records the tiers reached. Bulk evidence remains under the session scratch directory outside Git.

The task stops at the acceptance gate. The proposed reviewer must review each applicability decision and accept both claims by naming the revision, both claim IDs, and the package.

## Merged-source cycle: e4d97bb83

This cycle verifies the merged working tree above `e4d97bb8356996a9371d6f54b6d9c05afe6378cb`. It creates no commit or merge.

The harness session ported the source, proof, driver, and claim changes of this cycle onto the node branch on 2026-09-23. The prefix parser keeps the node branch design from `03d7f1b27`. The node records and the package remain pending.

The earlier `casper-node-claim-gate-8789c1c3e-01` package was absent. The package for this cycle was withheld from the tree because it carried bulk logs and home-directory paths. The node session regenerates it under the evidence retention rule.

### Changes

`InterfaceSafety` adds conditional proofs for directory admission, peer identity, socket cleanup, awaited shutdown, and deadline admission.

`CaptureIntegrity` adds complete-row proofs, a logical framing proof, and scratch-store isolation proofs. The formal gate now requires 25 closed assumption sets.

The new Rust checks cover all 4,096 leaf permission values, peer mismatches, socket identity fields, expired writes, and awaited shutdown.

The canonical reader independently decodes four capture cases. Scratch checks compare allocation identities and test frontier mutation isolation.

The binding driver now runs the observer and capture suites. It also requires every test in `bindings.json` to report success.

B8 now names the retained `generation_change_after_validation_rejects_capture` regression. The merged branch already contained that test.

### Verification

The bounded TLA gate passed 16 positive configurations and 88 expected violations. The node subset contains two positive configurations and 17 controls.

The TLA fixture passed all 88 controls. The formal gate fixture passed 47 exact-exit refusal controls.

The full Rocq gate passed with Rocq 9.1.1. All 25 node theorem exports have closed assumption sets.

The final isolated Linux run passed 330 tests. One helper test is ignored by the test runner and invoked by its cross-process parent.

Strict Clippy, the workspace check, formatting, and source identity comparisons passed. The driver retains separate identities for raw and debug-stripped observer executables.

All nine Kani harnesses passed with Kani 0.67.0 and CBMC 6.8.0 on ARM64 Linux. The compiler uses the 2025-11-21 nightly.

Kani checks the production prefix parser and compressed-length preflight. Public paths retain native tests for allocation, diagnostic formatting, and complete decoding.

The prefix refactor preserves public errors and return values. The block decoder calls the extracted preflight before varint decoding or decompression.

Kani 0.68 produced a compiler error and stalled on allocation paths. Retained logs also record the disk exhaustion and the unsuccessful solver attempts.

### Limits

The filesystem proofs assume accurate metadata and a stable namespace under trusted owners. Cleanup can still fail if unlink fails.

Deadline proofs depend on the lock library, monotone clocks, and cooperative scheduling. They do not establish operating-system latency bounds.

The canonical theorem covers logical framing. Complete refinement of the Rust wire schema remains pending, including every field and collection representation.

The five proposed finite-domain classifications remain unaccepted. Both claims and all ledger tiers remain pending until the required evidence and named review exist.

The existing workflow and TLA gate governance records keep their prior claim identities. The regenerated package will supply separate node claim views for those files.

The six remaining STE findings occur in unchanged prose. The new and revised prose received a separate sentence-length review.

## Merged-source cycle 02: 00f91ca11

This cycle refreshes the verification evidence at the merged revision `00f91ca11fd1153183818cde605d7d19eea00a7f`. The harness session ran it on 2026-09-23 while the node session worked on B11.

The node branch refreshed its own records at `78d696ea6` in parallel. That package, `casper-node-claim-gate-78d696ea6-01`, is not merged into this branch. The next merge must reconcile the two record sets.

### Refutation tier

The bounded gate ran alone on a clean export of the execution base. It passed 16 clean configurations and 88 expected violations with exit zero.

Both node models passed: `MC_ObserverSession` with 689 distinct states and `MC_BoundedCapture` with 10,066 distinct states. All 17 node controls exited 12 on their named invariants.

The hosted full tier in run 35895550078 passed 31 clean configurations and 88 expected violations. The registry fixture passed all 88 controls in a separate run.

A first attempt ran the fixture and the gate at the same time. The fixture overwrote eleven shared TLC logs, so the gate misclassified eleven controls. The recorded run avoids that overlap.

### Construction tier

Local Rocq execution was not possible. Docker Desktop routes container traffic through a TLS-intercepting proxy, so apt and opam cannot install Rocq inside a container.

The hosted Rocq job in run 35895550078 ran `scripts/ci/check-formal-invariants.sh --rocq` with Rocq 9.2.0 from opam. It passed with 36 closed assumption sets across four projects, including all 25 node exports.

The formal gate fixture passed 47 exact-exit refusal controls locally.

### Binding tier

The hosted binding driver built the shared, block-storage, and node test targets on x86_64. It then stopped at its strip-equivalence check before any test executed.

The cause is a binutils detail. After `objcopy --strip-debug`, the `.init_array` EntSize field changes from 00 to 08, and the driver compares that column. The same defect made the node branch's own binding job fail before the port.

The rocq-build job with the wire correspondence check was not created, because it needs the binding job. The retained artifact holds the build logs, the input digests, and the section tables.

Native macOS runs of the shared and block-storage suites passed 303 tests and failed 4. The four failures are the known `/private/var` path comparisons that PR #447 corrects. The node observer suites are Linux-only.

Kani did not run in this cycle. The eight recorded harnesses keep their prior results from the handoff cycle.

Lint with warnings denied and formatting passed on the clean export.

### Records and package

The package `casper-node-claim-gate-00f91ca11-01` keeps four compact files. The 58 scoped records now pin the merged revision, the refreshed claim digests, and this package. Two new records cover `InterfaceSafety.v` and `CaptureIntegrity.v`.

The strict audit returned exit 4 before the refresh with the two new modules unrecorded. It is expected to return exit 4 after the refresh with all 62 mandatory records pending.

Both claims remain pending. Acceptance still needs named maintainer review, the B11 canonical-schema construction, and a hosted binding run after the driver correction.

## Hosted binding strip correction

The hosted section tables differ only in the `.init_array` entry size, from `00` to `08`. Their program segments and section-content hashes match.

The driver now normalizes zero entry sizes for `PREINIT_ARRAY`, `INIT_ARRAY`, and `FINI_ARRAY` to the ELF pointer width. Other entry sizes remain checked.

The [ELF specification](https://gabi.xinuos.com/elf/09-dynamic.html#initialization-and-termination-functions) defines these arrays as function pointers. Dynamic entries supply their addresses and total sizes.

The driver retains original section headers and compares normalized allocated-section metadata, program segments, and section-content hashes. It rejects empty allocated-section inventories.

The new fixture builds a Rust executable and reproduces the original entry-size mismatch with GNU objcopy. Both original and stripped executables run successfully.

Twelve controls alter code bytes, array bytes, flags, sizes, alignment, entry sizes, program flags, or the entry point. Each control requires exit one.

CI runs the fixture before the binding driver. Both claim inventories include the fixture, and both claims remain pending.

The native ARM64 Linux binding driver passed 332 tests and two B11 export tests. The strip fixture rejected all twelve mutations.

Actionlint and all 47 formal gate refusal controls passed. The first binding rerun failed during linking because the container disk was full.

The rerun passed after disposable incremental build cache was removed. Retained logs are under `target/strip-equivalence-fix/`.

The hosted x86_64 rerun and combined evidence refresh remain pending. This correction changes no claim acceptance status.

## Combined B11 evidence cycle 03: 38e576041

The current package is `docs/cbc-evidence/runs/casper-node-claim-gate-38e576041-01/report.json`. Its execution base is `38e57604187feab97cb45f000f95270b12a9f8bf`.

Both node projects pass local and hosted Rocq checking. The gate checks 25 parent exports and eight B11 exports, with 33 closed assumption sets.

The complete formal gate checks 44 closed assumption sets across five projects. The package lists every node export and retains the kernel logs.

B11 verifies 75 production wire cases and six rejection controls. These finite cases do not establish universal refinement of the Rust implementation.

The local bounded TLA gate passes 16 configurations and 88 expected violations. Hosted run 35906410283 passes 31 configurations and the same controls.

Both node models pass, with all 17 node controls. Earlier local attempts failed because of the Java environment and sandbox socket restrictions.

The hosted x86_64 and native ARM64 binding drivers each pass 332 tests and two B11 export tests. One ignored helper executes through its parent test.

The hosted strip fixture rejects all 12 executable mutations. The formal gate fixture passes 47 refusal controls.

Kani 0.67.0 and CBMC 6.8.0 verify all eight harnesses in the current source. The package records symbolic inputs, prefix bounds, assumptions, and compiler flags.

The Kani compiler uses nightly-2025-11-21 on ARM64 Linux. Explicit AES and NEON configuration permits dependency compilation without disabling verification checks.

Kani does not verify complete decompression, allocation behavior at arbitrary sizes, or the complete observer. Native tests and conditional model proofs retain their separate scopes.

The package retains unsuccessful Kani setup attempts. The earlier nine-harness result describes a different source revision and does not supply this cycle's count.

The hosted binding, TLA, Rocq correspondence, and formal gate jobs all pass. Other workflow jobs do not determine these node claim results.

The current inventory contains 81 artifacts, including 70 mandatory artifacts. The refresh updates 60 node records and creates eight records for B11 and the strip fixture.

The workflow and TLA gate keep their primary governance records. The package supplies separate node registration views for those two files.

Source hashes bind the current files. Changes after execution affect claim text, the applicability table, and binding metadata only.

The package retains the original execution manifests and identifies those metadata differences. Every tested Rust, proof, model, and driver file matches its execution source.

The strict audit returns exit four with 70 pending records and no missing records. A separate digest check verifies artifact, claim, and report identities.

Both claims remain pending. Acceptance requires named maintainer review of applicability decisions, conditional assumptions, and the sufficiency of finite Rust correspondence.

Bulk logs and previous records remain in the ignored archive named by the report. No claim acceptance, commit, or push occurs in this cycle.

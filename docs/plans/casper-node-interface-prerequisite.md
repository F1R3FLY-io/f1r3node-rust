# Casper Node Interface Prerequisite

**Status:** Batch A and its shutdown correction are implemented on the separate node branch. Its claim remains pending. Later batches require separate approval.

**Consumer:** TASK-017-12. This prerequisite is separate from EPIC-017 and EPIC-018 harness verification.

**Reviewed checkout:** `859cbc36cb58ebac06b2097a256a1cd7b6740afb`.

**Reviewed node:** `6940a5beb4aa806d3d75f6df3be9f238512fcc2f`, the last queried `dev` revision.

The inspected `casper/src`, `node/src`, `block-storage/src`, and `models/src` trees match that node revision. This is source equality, not executed node qualification.

The user authorized this prerequisite after the Linux completion review. Authorization does not discharge claims, approve publication, authorize a merge, or change the campaign resource limits.

The [ratified plan](casper-ratified-soak-2026-09-16.md) continues to exclude node implementation from the harness epics. This separate prerequisite does not amend that boundary.

## Independent delivery and current status

[PR #447](https://github.com/F1R3FLY-io/f1r3node-rust/pull/447) targets `dev` from `feature/casper-node-observation`. It can merge independently after its own approval and verification gates pass.

[PR #436](https://github.com/F1R3FLY-io/f1r3node-rust/pull/436) temporarily targets the node branch. After the prerequisite merges, the harness pull request can return to `dev`.

This dependency order does not include node implementation in the harness scope. The node prerequisite must not wait for the harness pull request to merge.

A pull request target does not establish commit ancestry or successful integration. The inspected harness head does not contain the published node commits.

Batch A and its shutdown correction are published through `799e2136adc6e0100b289945d9a5a6851e81c91f`. The correction has a source-order regression, not an end-to-end node shutdown test.

The [revised Batch B proposal](https://github.com/F1R3FLY-io/f1r3node-rust/blob/799e2136adc6e0100b289945d9a5a6851e81c91f/docs/plans/casper-node-observation-batch-b.md) separates detached capture from later evaluator wiring. Batch B1, Batch B2, and Batch C remain unapproved.

The prerequisite is incomplete and not merge-ready. Local tests do not discharge its claim or qualify authority and publication capabilities.

Independent harness execution-control work can proceed now. Live qualification and campaign execution still require the completed prerequisite and all existing gates.

## Goal

Expose bounded observations from the actual candidate node. Supply the evidence that the authority and publication profiles require without substituting fixture records for node observations.

Keep consensus rules, threshold comparisons, deploy identity, retention rules, settlement behavior, and default node operation unchanged.

Do not repair product defects through an observation interface. Record each discovered defect as separate node work.

## Source findings

| Source | Observed constraint | Design consequence |
| --- | --- | --- |
| `block-storage/src/rust/dag/block_dag_key_value_storage.rs::get_representation_internal` | The representation copies maps but shares metadata, cache, lifecycle, and carrier stores. | A cloned representation is not an immutable evaluation snapshot. |
| `casper/src/rust/estimator.rs::tips_with_latest_messages` | Estimation reads the supplied DAG representation. | Both comparison modes need the same retained input bytes and independent scratch state. |
| `casper/src/rust/safety/clique_oracle.rs::ft_witnessed_exact` | Exact stake values and the decision exist inside the oracle. | Capture these values at their calculation boundary instead of reconstructing them from abbreviated logs. |
| `casper/src/rust/engine/multi_parent_casper/finalization_runner.rs::compute_last_finalized_block` | The finalizer distinguishes advance, containment hold, absence hold, and incompatibility hold. | Preserve each decision and its original context. An absent response is not a hold. |
| `block-storage/src/rust/dag/block_dag_key_value_storage.rs::record_directly_finalized` | Effects run outside the storage lock before metadata persistence. | Independent effect and metadata reads cannot establish atomic publication. |
| `casper/src/rust/engine/multi_parent_casper/block_admission.rs::admit_handle_valid_block` | DAG insertion, lifecycle evaluation, pool removal, events, and finalization are separate steps. | Define each cut point against an exact step. Do not rename every step as one atomic transaction. |
| `casper/src/rust/engine/multi_parent_casper/block_admission.rs::admit_list_pending_deploys` | The method filters two signature-keyed stores through a snapshot and the current time. | This query is not a complete raw durable-work inventory. |
| `block-storage/src/rust/dag/deploy_lifecycle_types.rs` | Existing lifecycle records use deploy signatures. | A deploy signature must not become an invented occurrence identity. |

These findings identify implementation boundaries. They do not prove that every possible interface is absent.

## Proposed interface

### Local access and activation

Use a Unix-domain socket for Linux qualification. Do not add fault controls to the public HTTP or gRPC routes.

Require explicit startup configuration and an owner-only socket directory. Reject symlinks, unsafe ownership, and group or public access.

Check the connecting process identity. Bind each session to the approved request digest, candidate executable digest, configuration digest, and fresh process incarnation.

Keep the observer disabled by default. Default startup must create no socket, observation journal, background evaluator, or armed fault control.

Separate read-only observation permission from fault-control permission. Read-only access must never arm a pause or terminate a process.

Use the same candidate executable for enabled and disabled qualification tests. A separately built executable has a different candidate identity.

### Records and bounds

Version every request and response. Record source identity, request identity, event sequence, clock identity, process incarnation, and artifact digests.

Represent missing values with a presence state and reason. Do not substitute zero, an empty inventory, or a successful decision.

Propose one active session and one active evaluation per node. Limit requests and individual response pages to 1 MiB.

Require explicit limits for snapshot bytes, block count, validator count, evaluation work, retained records, and control lifetime.

Reject requests above a limit before expensive work. Reject incomplete pagination instead of treating a truncated inventory as complete.

Do not log credentials, private keys, or authentication material. Export only the fields required by the approved observation contract.

Use an explicit allowlist for configuration evidence. Do not serialize the complete node configuration into an observation record.

### Authority observations

Capture the required metadata, block bytes, electorate, latest messages, configuration, and availability state into a detached snapshot.

Every field used by paired evaluations must come from that snapshot. Record its canonical digest and explicit coverage boundary.

Do not hold a production storage lock across asynchronous evaluation. Bound the capture operation and reject unstable or incomplete captures.

Run bounded and reference evaluation against separate scratch views of the same snapshot. A second call to the same path is not reference coverage.

Preserve the actual exact-threshold decision, original fault-tolerance value, and projection separately. Keep comparison results distinct from live finalizer decisions.

Capture traversal counters at the operations they count. If a required counter is unavailable, report that limitation rather than estimating work.

Verify that observation evaluation cannot write production caches or advance consensus state.

### Publication observations and fault receipts

Name cut points by their exact operation and source binding. Bind an armed request to one publication attempt, node incarnation, and deadline.

Retain a reached-boundary receipt before the external controller requests process termination. A request acknowledgment alone does not prove that the boundary was reached.

Use a bounded pause lease. A disconnected controller must not leave an indefinite pause or permit reuse of an expired request.

The external controller records the actual process exit and the linked restart. The node must not claim its own unobserved future exit.

Read raw durable records after restart. Keep reported block bytes, state roots, effects, terminal records, and unresolved work tied to their actual storage identities.

An observation journal is evidence transport, not an authoritative replacement for node storage. Journal presence cannot prove that a node publication survived.

Do not repair storage atomicity to manufacture a passing observation. Report a torn publication as a product failure when the required evidence supports that conclusion.

Keep unavailable occurrence-level capabilities unsupported before the actual PR #216 merge. Signature-keyed records cannot satisfy D-07 Reading A.

On 2026-09-22, the user confirmed that this branch merges before PR #216 integrates.

Occurrence-dependent publication qualification waits in EPIC-018. This deferred qualification does not make PR #216 a prerequisite for this branch.

Before implementing publication capture, define the existing effect identity and the supported publication consistency boundary. If either is absent, report the capability as unsupported.

## File-level implementation sequence

Each batch requires its listed source bodies and callers to be reviewed before edits. Additional files require an updated plan before implementation.

### Batch A: Local protocol and disabled-by-default access

The user approved this nine-file batch, its pending claim, and both mandatory tags. The table records that approved scope.

This batch adds transport, identity checks, bounds, and explicit capability reporting. It does not claim authority or publication qualification.

| File | Proposed change |
| --- | --- |
| `node/src/rust/soak_observer.rs` | Add the local protocol, session validation, bounded transport, and permission checks. |
| `node/src/rust/mod.rs` | Register the observer module. |
| `node/src/rust/configuration/model.rs` | Add an optional observer configuration with a disabled default. |
| `node/src/rust/configuration/commandline/options.rs` | Add explicit observer startup options. |
| `node/src/rust/configuration/commandline/config_mapper.rs` | Map the options without changing existing defaults. |
| `node/src/rust/configuration/mod.rs` | Validate the observer configuration and reject unsupported platforms or invalid bounds. |
| `node/src/rust/runtime/node_runtime.rs` | Start and stop the observer only after explicit activation. |
| `node/src/rust/diagnostics/tests.rs` | Keep its explicit configuration fixture disabled by default. |
| `node/tests/soak_observer.rs` | Test access, bounds, identities, disabled behavior, and session cleanup. |

The workspace syntax search found configuration literals in the mapper tests, diagnostics tests, and `check_dev_mode`. The last uses struct update syntax.

The mapper also contains an explicit `RunOptions` fixture. Batch A includes that fixture update.

Reuse existing dependencies for transport, serialization, identifiers, and hashing. No dependency or lockfile change is proposed.

No public API route or protobuf schema change is proposed for this batch.

### Batch B: Detached authority capture and evaluation

The revised Batch B proposal supersedes this original file list. This historical list does not authorize implementation or replace the revised approval gates.

| File | Proposed change |
| --- | --- |
| `block-storage/src/rust/dag/soak_snapshot.rs` | Add bounded detached snapshot data and capture checks. |
| `block-storage/src/rust/dag/mod.rs` | Register the snapshot module. |
| `block-storage/src/rust/dag/block_dag_key_value_storage.rs` | Expose a checked capture boundary without changing normal storage behavior. |
| `casper/src/rust/soak_observer.rs` | Add typed authority observations and isolated evaluation requests. |
| `casper/src/rust/mod.rs` | Register the Casper observer module. |
| `casper/src/rust/estimator.rs` | Expose source-bound evaluation observations and actual work counters. |
| `casper/src/rust/safety/clique_oracle.rs` | Expose exact decision inputs and results without changing the decision rule. |
| `casper/src/rust/finality/floor.rs` | Expose floor decisions and traversal observations without changing containment rules. |
| `casper/src/rust/engine/multi_parent_casper/finalization_runner.rs` | Capture actual finalizer decisions separately from diagnostic evaluation. |
| `node/src/rust/soak_observer.rs` | Connect the local protocol to the checked authority interface. |
| `block-storage/tests/soak_snapshot.rs` | Test snapshot independence, concurrent mutation, limits, and incomplete capture. |
| `casper/tests/soak_observer.rs` | Test identical inputs, exact boundaries, counters, and unchanged consensus results. |

Node-to-Casper wiring must use an explicit observer handle. No global singleton or unrestricted access to validator identity is proposed.

The exact handle wiring requires a follow-up review of constructors and trait implementations before Batch B approval.

### Batch C: Publication boundaries and restart evidence

This batch depends on a reviewed publication consistency contract. It cannot promise unavailable occurrence semantics or cross-store transactions.

The source review identified these boundaries for the next file-level plan:

- `casper/src/rust/engine/multi_parent_casper/block_admission.rs`
- `casper/src/rust/engine/multi_parent_casper/finalization_runner.rs`
- `block-storage/src/rust/dag/block_dag_key_value_storage.rs`
- `block-storage/src/rust/dag/deploy_lifecycle_types.rs`
- `block-storage/src/rust/deploy/key_value_deploy_storage.rs`
- `block-storage/src/rust/deploy/key_value_rejected_deploy_buffer.rs`

A separate source review must identify every writer needed for a consistent raw inventory. Batch C cannot infer that coverage from these six files alone.

### Later harness integration

Only qualified capabilities may replace the current refusal path. The existing guards remain until a real adapter and its negative controls pass.

Review `scripts/casper-soak/src/runtime.rs`, both profile modules, their tests, and their claim inventories as a separate harness batch.

That batch requires fresh source-bound verification and explicit acceptance. Existing controlled-transcript discharges do not cover a new live path.

## Required tests

1. Verify that default startup creates no observer resources and preserves existing behavior.
2. Reject unauthorized peers, unsafe paths, wrong incarnations, stale requests, and replayed fault commands.
3. Reject oversized requests, excessive work, incomplete inventories, and exceeded deadlines.
4. Mutate live storage after capture and verify that detached observations remain unchanged.
5. Compare enabled and disabled node results for the same ordinary workload.
6. Verify exact threshold boundaries, strict-majority rejection, and distinct finality hold reasons.
7. Verify independent scratch state and real counter increments for paired evaluations.
8. Disconnect the controller at each pause state and verify bounded recovery.
9. Kill and restart an isolated test node at each supported cut point.
10. Reject missing exit evidence, unrelated restarts, torn records, and fabricated occurrence identities.

Test source changes require their own retained failing controls and source identities. Passing transport tests do not qualify a node profile.

Use local isolated qualification first. Preserve failed attempts and do not charge unapproved replacement machines to the baseline budget.

## Verification and delivery gates

`casper/src/rust/finality/floor.rs` and `block-storage/src/rust/dag/block_dag_key_value_storage.rs` already have mandatory CbC attributes.

Identify all affected claims before changing those files. Propose mandatory tags for new observation and fault-control artifacts for human ratification.

Batch A has approved `.gitattributes` entries for the observer module and its dedicated tests. Both entries use `cbc=mandatory` and `cbc-weight=high`.

The node branch registers the pending interface claim in `docs/claims/casper-node-observation.md`. Its inventory includes all nine Batch A source and test files.

Retain per-artifact verification records under `docs/cbc-evidence/`. Neither registration nor a passing unit test discharges this claim.

Do not replace existing node obligations with a harness-only claim. Record new interface claims separately and keep them pending until their verification requirements pass.

Keep node implementation, verification, and delivery separate from PR #436, diagnostic PR #441, and the post-merge EPIC-018 branch. A temporary dependent target preserves this separation.

A local instrumented build is a new candidate. It is not execution of the currently pinned CI images.

The required interfaces must reach the selected `dev` candidate before a current-dev baseline can use them. Merging, publishing images, and repinning remain separate actions.

After qualification, TASK-017-12 still needs campaign execution controls, claim renewal, separate preflight, both full baselines, and evidence delivery.

The later 60-hour phase retains its separate candidate-count and runner-lifetime decision. No interface test satisfies that resource decision.

## Approval boundary

Batch A approval is complete. Batch B1 requires confirmation of its revised scope, claim, and tags. Batch B2 and Batch C require later review and approval.

This plan does not authorize storage-policy changes, protocol changes, occurrence-store implementation, production fault injection, cloud launches, commits, pushes, or claim acceptance.

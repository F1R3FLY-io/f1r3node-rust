# Casper Node Observation: Batch E

```yaml
status: scope_confirmed_implementation_blocked
scope_confirmed_by: user
scope_confirmed_at: 2026-09-30T15:38:36.579734+00:00
approval_checkpoint: 06f0d3e8cf85f7072769960cde3746dc0830a68f
reviewed_checkpoint: 030384f5f31613c64552f7d5b517ef1fe2552d11
task: TASK-019-10
blocked_by: TASK-019-9
handoff_milestone: TASK-019-9_step_13
handoff_received: false
requires_batch_d_task_completion: false
main_files: 39
ledger_records: 31
run_files: 2
total_files: 72
new_files: 33
changed_files: 39
cbc_tags_ratified: true
cbc_tags_applied: false
claim_registered: false
implementation_started: false
```

## Authorization and sequencing

The user approved the 72-file scope and the documented recommendations on 2026-09-30.
The approval includes all sixteen decisions, the complete shared arithmetic, five mandatory tags, and the 4,096-row ceiling.
Batch D is at step 8 of 16 at the approval checkpoint.

Agent A will hand over the final Batch D files after step 13.
Batch E implementation remains blocked until that explicit handoff and the final shared-source check.
The file handoff does not establish completion, verification, or acceptance of the Batch D task.
An additional implementation path requires a scope amendment.

The review used checkpoint `030384f5f31613c64552f7d5b517ef1fe2552d11`.
Agent A changed three shared observer files during that review.
The implementation baseline will use the step-13 handoff revision, not the earlier review hashes.
No claim, tag, or ledger registration occurred through this plan promotion.

## Approved design

Use one capture interval, compact tracker records, shared production arithmetic, and the proposed bounded model.
Keep the existing snapshot schema and the default response unchanged.
Require explicit limits and explicit unavailable results.
Keep all acceptance, source-binding, and live-qualification gates separate.

The complete shared-arithmetic scope contains **72 files: 39 main files, 31 records, and two run files**.
It contains **33 new files and 39 changed files**.
The user ratified the five `cbc=mandatory cbc-weight=high` tags.
No tag was applied.

## Corrections to the earlier draft

1. The tracker lists six decisions, but `open-questions.md` contains sixteen.
2. The earlier recommended scope contains 69 files, not thirty.
3. The pure initial-fault helper does not share the final subtraction at `BlockAPI::get_block_info_with_dag`.
4. The earlier scope leaves the changed production dispatch file untagged.
5. The old gate counts and CI-failure descriptions do not describe the current branch.

Sharing the complete arithmetic requires one additional source path: `casper/src/rust/api/block_api.rs`.
The public block API and the observer must call the same final-subtraction helper.
The initial-fault helper stays in the proposed `initial_fault.rs` module.
These two helpers preserve the current `f32` operation order.

The dispatch file and the block API file require their ratified mandatory tags and separate records.
Neither source path currently has a default ledger record.
Compared with the 69-file draft, this adds one source change and two new records.
It adds no dependency, lockfile, module, workflow, or model beyond the draft.

## Six primary decisions

| Decision | Recommendation | Required boundary |
| --- | --- | --- |
| Capture consistency | K1: one guarded capture, with a separate sealed tracker record | Use the same DAG reader transaction and validation. Do not make a second capture. |
| Retained tracker content | R2: key, hash count, and raw-value digest | Bind the complete stored row bytes without retaining all detected block hashes. |
| Arithmetic source | A, extended to share the final subtraction | Keep duplicate-record terms, signed-to-unsigned casts, zero-total behavior, and ordinary overflow behavior. |
| Tracker read failure | Reject the complete display-request capture | Do not return a partial snapshot after a failed required read. |
| Missing-block base | Use the live minimum with the explicit source label `missing_history_minimum` | Preserve the typed error. Other errors remain unavailable. Do not replace the original-oracle result. |
| Finalized test | Use the captured finalized set or metadata flag | Report both inputs. Keep the existing persisted-field availability rule separate. |

## Remaining draft decisions

| Draft question | Recommendation |
| --- | --- |
| 7: Refutation | Add one bounded model, one positive configuration, and three negative controls. |
| 8: Earlier claims | Keep historical acceptance at its recorded revision. Return changed records to pending. |
| 9: Response scope | Keep the outer scope and add an optional display scope. |
| 10: No display option | Keep `equivocation_snapshot_unavailable` and omit new optional fields. |
| 11: Multiplicity | Report matched records and distinct equivocators. Do not replace record terms with validator terms. |
| 12: Shared lock | Use the raw bounded reader under the capture guards, subject to contract-owner approval. |
| 13: Code and tags | Keep three new modules. Add tags for those modules and the two changed production arithmetic files. |
| 14: Row limit | Require a positive request limit with a proposed ceiling of 4,096. No production limit changes. |
| 15: Reference | Do not add another arithmetic implementation. Test against frozen behavior and the actual shared production path. |
| 16: Identity | Use `TASK-019-10` and `CLAIM-CASPER-NODE-OBSERVATION-005`. The task already exists. The claim remains unregistered. |

The user approved the record ceiling and the guarded raw read.
The user approved all sixteen decisions and the complete-arithmetic extension.
Historical acceptance does not cover new source bytes.

## Ratified attribute paths

Each path below receives `cbc=mandatory cbc-weight=high` after the final Batch D source check and pending claim registration.
The approval does not mark any source as discharged.

- `block-storage/src/rust/dag/soak_equivocations.rs`
- `casper/src/rust/safety/initial_fault.rs`
- `casper/src/rust/soak_observer/display.rs`
- `casper/src/rust/engine/multi_parent_casper/dispatch.rs`
- `casper/src/rust/api/block_api.rs`

## Required design details

### Capture and identity

The capture already holds the shared global guard and the metadata guard.
`access_equivocations_tracker` takes the exclusive global lock.
Calling that function inside the capture would deadlock.
Use a read-only accessor on `SoakCaptureAccess` and the existing bounded reader instead.

The tracker store belongs to the DAG environment.
Its public typed-store field permits the accessor without changing `equivocation_tracker_store.rs`.
Tracker writes do not increment the insertion generation.
Transaction validation must therefore cover the tracker read independently of the generation check.

The existing limited scan removes an eight-byte, little-endian outer length prefix.
A digest described as a raw-value digest must include that prefix.
The implementation can hash the validated reconstructed prefix and payload without another shared-reader API.
Tests must compare that digest with the bytes that the production writer stores.

Keep the existing capture wrappers and snapshot data layout compatible with the downstream B11 fixture module.
A display capture can change existing usage counters and the snapshot digest.
Schema compatibility does not mean identical snapshot bytes when the display option is present.

### Arithmetic and metering

Preserve one weight term for each tracker record.
Do not deduplicate validator weights or repair negative stakes in this batch.
The observer checks both integer sums before it invokes the shared unchecked production arithmetic.
The same-bits requirement applies to accepted, non-overflowing observer inputs.

Charge all observer passes before their operations.
This includes validation, hashing, sorting, checked sums, shared-helper scans, and multiplicity counts.
Use the existing aggregate budget and measured path.
A new work path requires a separate scope decision.

### Admission and default compatibility

An absent display option must preserve the current serialized request and response.
An explicit `null` display option should fail admission.
Reject zero, excessive, fractional, duplicate, and unknown display fields before store access.
Do not add tracker rows to the response.

A non-finalized target without a requested original calculation remains unavailable.
A finalized-set entry can select persisted metadata even when the metadata flag is false.
That difference must remain visible rather than changing the old persisted result.

### Formal and CI boundaries

The current node model manifest contains 25 entries after Batch D's model registration.
The proposed display model adds four entries.
Derive final gate counts from the step-13 handoff manifest rather than the older 19-entry baseline.

The existing binding driver runs node tests and omits several Batch E Rust and proof inputs from its hash list.
Do not treat that job as complete Batch E test or source coverage.
Keep its current narrow scope and the workflow files unchanged in this proposal.
Record complete Batch E sources and isolated Rust and formal checks in the new run package.
A broader hosted job requires a separate scope amendment.

The new Rocq theory covers integer sums, multiplicity, availability, and identity conditions.
It does not prove IEEE 754 operations or the new tracker encoding.
The matched-weight bound requires the stated no-repeated-validator condition.
Do not apply that bound to general tracker records.

Correctness by Construction (CbC) records require independent source, claim, and report hash checks.
The generic status gate does not perform those source checks.
Preserve other claim obligations, including `CLAIM-FINALITY-002` and the formal-gate claim.
Create new-file records only after those files exist.

## Scope alternatives

| Scope | Main files | Records and run files | Total | Effect |
| --- | ---: | ---: | ---: | --- |
| Original draft | 38 | 31 | 69 | Omits dispatch tagging and shares only initial-fault arithmetic. |
| Shared initial fault only | 38 | 32 | 70 | Adds the dispatch record. Requires a narrower arithmetic acceptance criterion. |
| Complete shared arithmetic, recommended | 39 | 33 | 72 | Adds the block API path and both production arithmetic records. |
| Old minimum draft | 23 | 18 | 41 | Duplicates arithmetic and omits the new bounded model. It needs a separate verification decision. |

The recommended scope preserves the current single-arithmetic-source acceptance criterion.
The 70-file alternative does not share the final subtraction.
The 41-file alternative is not an equivalent assurance reduction.
Combining ledger files would change the current per-artifact lookup and is outside this review.

## Exact approved main-file list

The state column compares each path with the reviewed Git tree.
The local review inventory carries the exact reviewed attributes and source hashes.

| Number | Path | State |
| ---: | --- | --- |
| 1 | `block-storage/src/rust/dag/soak_equivocations.rs` | new |
| 2 | `block-storage/src/rust/dag/mod.rs` | changed |
| 3 | `block-storage/src/rust/dag/soak_snapshot.rs` | changed |
| 4 | `block-storage/src/rust/dag/block_dag_key_value_storage.rs` | changed |
| 5 | `block-storage/tests/soak_snapshot.rs` | changed |
| 6 | `casper/src/rust/safety/initial_fault.rs` | new |
| 7 | `casper/src/rust/safety/mod.rs` | changed |
| 8 | `casper/src/rust/engine/multi_parent_casper/dispatch.rs` | changed |
| 9 | `casper/src/rust/soak_observer/display.rs` | new |
| 10 | `casper/src/rust/soak_observer.rs` | changed |
| 11 | `casper/src/rust/soak_observer/evaluation.rs` | changed |
| 12 | `casper/tests/soak_observer.rs` | changed |
| 13 | `node/tests/soak_observer.rs` | changed |
| 14 | `casper/src/rust/api/block_api.rs` | changed |
| 15 | `formal/tlaplus/node_observation/DisplayProjection.tla` | new |
| 16 | `formal/tlaplus/node_observation/MC_DisplayProjection.tla` | new |
| 17 | `formal/tlaplus/node_observation/MC_DisplayProjection.cfg` | new |
| 18 | `formal/tlaplus/node_observation/MC_DisplayProjection_fabricated_unsafe.tla` | new |
| 19 | `formal/tlaplus/node_observation/MC_DisplayProjection_fabricated_unsafe.cfg` | new |
| 20 | `formal/tlaplus/node_observation/MC_DisplayProjection_source_unsafe.tla` | new |
| 21 | `formal/tlaplus/node_observation/MC_DisplayProjection_source_unsafe.cfg` | new |
| 22 | `formal/tlaplus/node_observation/MC_DisplayProjection_interval_unsafe.tla` | new |
| 23 | `formal/tlaplus/node_observation/MC_DisplayProjection_interval_unsafe.cfg` | new |
| 24 | `formal/tlaplus/node_observation/verification-plan.json` | changed |
| 25 | `formal/tlaplus/node_observation/bindings.json` | changed |
| 26 | `formal/tlaplus/node_observation/README.md` | changed |
| 27 | `formal/rocq/node_authority/theories/DisplayProjection.v` | new |
| 28 | `formal/rocq/node_authority/theories/MainTheorem.v` | changed |
| 29 | `formal/rocq/node_authority/_CoqProject` | changed |
| 30 | `formal/rocq/node_authority/README.md` | changed |
| 31 | `scripts/ci/check-tla-invariants.sh` | changed |
| 32 | `scripts/ci/test-check-tla-invariants.sh` | changed |
| 33 | `scripts/ci/check-formal-invariants.sh` | changed |
| 34 | `docs/plans/casper-node-observation-batch-e.md` | new |
| 35 | `docs/claims/casper-node-display-projection.md` | new |
| 36 | `docs/work-logs/task-019-10-display-projection.md` | new |
| 37 | `docs/plans/casper-node-observation-batch-b.md` | changed |
| 38 | `docs/ToDos.md` | changed |
| 39 | `.gitattributes` | changed |

## Exact approved ledger list

Each mandatory artifact keeps one default lookup record.
The source records remain unregistered until the final Batch D source check.

| Path | State |
| --- | --- |
| `docs/cbc-evidence/block-storage-src-rust-dag-soak-equivocations-rs.md` | new |
| `docs/cbc-evidence/block-storage-src-rust-dag-soak-snapshot-rs.md` | changed |
| `docs/cbc-evidence/block-storage-src-rust-dag-block-dag-key-value-storage-rs.md` | changed |
| `docs/cbc-evidence/block-storage-tests-soak-snapshot-rs.md` | changed |
| `docs/cbc-evidence/casper-src-rust-safety-initial-fault-rs.md` | new |
| `docs/cbc-evidence/casper-src-rust-engine-multi-parent-casper-dispatch-rs.md` | new |
| `docs/cbc-evidence/casper-src-rust-soak-observer-display-rs.md` | new |
| `docs/cbc-evidence/casper-src-rust-soak-observer-rs.md` | changed |
| `docs/cbc-evidence/casper-src-rust-soak-observer-evaluation-rs.md` | changed |
| `docs/cbc-evidence/casper-tests-soak-observer-rs.md` | changed |
| `docs/cbc-evidence/node-tests-soak-observer-rs.md` | changed |
| `docs/cbc-evidence/formal-tlaplus-node-observation-DisplayProjection-tla.md` | new |
| `docs/cbc-evidence/formal-tlaplus-node-observation-MC-DisplayProjection-tla.md` | new |
| `docs/cbc-evidence/formal-tlaplus-node-observation-MC-DisplayProjection-cfg.md` | new |
| `docs/cbc-evidence/formal-tlaplus-node-observation-MC-DisplayProjection-fabricated-unsafe-tla.md` | new |
| `docs/cbc-evidence/formal-tlaplus-node-observation-MC-DisplayProjection-fabricated-unsafe-cfg.md` | new |
| `docs/cbc-evidence/formal-tlaplus-node-observation-MC-DisplayProjection-source-unsafe-tla.md` | new |
| `docs/cbc-evidence/formal-tlaplus-node-observation-MC-DisplayProjection-source-unsafe-cfg.md` | new |
| `docs/cbc-evidence/formal-tlaplus-node-observation-MC-DisplayProjection-interval-unsafe-tla.md` | new |
| `docs/cbc-evidence/formal-tlaplus-node-observation-MC-DisplayProjection-interval-unsafe-cfg.md` | new |
| `docs/cbc-evidence/formal-tlaplus-node-observation-verification-plan-json.md` | changed |
| `docs/cbc-evidence/formal-tlaplus-node-observation-bindings-json.md` | changed |
| `docs/cbc-evidence/formal-tlaplus-node-observation-README-md.md` | changed |
| `docs/cbc-evidence/formal-rocq-node-authority-theories-DisplayProjection-v.md` | new |
| `docs/cbc-evidence/formal-rocq-node-authority-theories-MainTheorem-v.md` | changed |
| `docs/cbc-evidence/formal-rocq-node-authority-CoqProject.md` | changed |
| `docs/cbc-evidence/formal-rocq-node-authority-README-md.md` | changed |
| `docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md` | changed |
| `docs/cbc-evidence/scripts-ci-test-check-tla-invariants-sh.md` | changed |
| `docs/cbc-evidence/scripts-ci-check-formal-invariants-sh.md` | changed |
| `docs/cbc-evidence/casper-src-rust-api-block-api-rs.md` | new |

## Proposed run package

The package name is reserved for the first Batch E verification cycle.
No package was created.

- `docs/cbc-evidence/runs/casper-node-display-projection-batch-e-01/report.json`
- `docs/cbc-evidence/runs/casper-node-display-projection-batch-e-01/validation.json`

## Handoff gate

Agent A specified the file handoff after step 13 on 2026-09-30.
The handoff must identify the revision and final state of these shared files:

- `casper/src/rust/soak_observer.rs`
- `casper/src/rust/soak_observer/evaluation.rs`
- `casper/tests/soak_observer.rs`
- `node/tests/soak_observer.rs`

The handoff also identifies the post-Batch-D record refresh scope and the exact nextest filters.
The record inventory must distinguish Batch D obligations from the approved Batch E records.
Additional implementation paths require a scope amendment.

`CasperShardConf::new()` has a parent limit of zero.
Tests must select their intended limit explicitly rather than fabricate a selected head.
Run rustfmt before requesting each commit.
Commits still require separate authorization.

The remaining Batch D evidence and acceptance work stays separate from this file-ownership transfer.
Source-specific verification must bind the pinned Batch D revision rather than a later mutable Batch E tree.

1. Retain the confirmed file list, all decisions, and the five ratified tags.
2. Obtain Agent A's explicit final-file handoff after Batch D step 13.
3. Recheck shared source hashes, the metered functions, the response fields, and the formal inventories.
4. Amend this scope if that check identifies an additional path.
5. Claim the task with the user-assigned Agent B identity.
6. Promote the confirmed plan.
7. Register the pending claim before implementation.
8. Verify changed source bytes.
9. Request named maintainer acceptance.

Keep final cleanup on stack branch 4, `fix/soak-finalization-attribution`, PR #441.
This approval does not authorize staging, commits, pushes, merges, branch changes, live runs, or removals.

# Casper Node Observation: Batch D Plan

**Status:** File scope confirmed by the user on 2026-09-30. The 12 open questions are answered with the recommendations of the draft. Steps 1 to 4 of the plan run before a production file changes.

**Branch for the implementation:** `feature/casper-node-observation`, base `e90e4cffa` at the confirmation.

**Reviewed base of the draft:** `670037c2511abd5f576063b3153681a873244a18`. The 16 existing files of the plan have no change between that base and `e90e4cffa`.

**Epic and owner:** EPIC-019. Task owner `claude-session-f3cbc961` (agent A of the split of 2026-09-30). Epic owner `claude-session-7015f552`.

**Consumer:** TASK-017-12 on `formal/soak-casper-consensus`.

**Task ID:** TASK-019-9. **Claim ID:** `CLAIM-CASPER-NODE-OBSERVATION-004`, registered as pending in [the claim file](../claims/casper-node-fork-choice-observation.md).

**Work log:** [task-019-9-paired-fork-choice.md](../work-logs/task-019-9-paired-fork-choice.md).

## Decisions of 2026-09-30

The user confirmed the file scope of the section "Exact file list" and accepted these answers to the 12 open questions of the draft.

| # | Question | Decision |
|---|----------|----------|
| 1 | Lower bound of the `reference` evaluation | The approved block. The field `bounds_differ` shows when the 2 bounds are different. |
| 2 | Independent reference or a second estimator call | Independent implementation in `casper/src/rust/soak_observer/fork_choice.rs`. |
| 3 | Filters of the production caller | The observer has a copy of the 2 filters. A differential test compares the `bounded` head with the first parent of a production snapshot. The claim records the copy as a limit. |
| 4 | Operation | Extend `authority_snapshot` with an optional selection. |
| 5 | Frame schema version | Stays 1. The capability list gets the entry `fork_choice`. |
| 6 | Counts of visited blocks and examined edges | Yes, in the fork-choice results only. A visited block is a distinct block hash that the evaluation reads. An examined edge is a parent or child link that the evaluation follows. TASK-017-12 confirms the definition against its measurement contract. |
| 7 | Bounded model | Yes, 1 model with 5 properties and 5 negative controls. |
| 8 | Tags of `estimator.rs`, `dag_operations.rs`, `proto_util.rs` | Mandatory tag and 3 ledger records. |
| 9 | Identifiers and sequence | Batch D first, claim 004 and TASK-019-9. Batch E takes claim 005 and TASK-019-10. |
| 10 | The `showMainChain` finding | Outside this batch. A separate task or issue records it. |
| 11 | Work paths | 6 in the shared budget. |
| 12 | Score output | Tip scores, a count, and a digest of the score map. |

The names that the draft marked as "(proposal)" are the names of this plan.

## Purpose

The harness profile compares the selected head of a `bounded` member and a `reference` member on equal inputs. The observer does not supply a selected head.

Batch D adds a paired fork-choice observation to the `authority_snapshot` operation. One capture supplies the inputs of the 2 evaluations.

The node stays the system under test. Batch D adds no new consensus rule and no new production limit.

## Source findings

| Finding | Effect on the batch | Reference |
|---------|---------------------|-----------|
| The observer evaluation does not import or call the estimator. | The batch adds a new evaluation section. | `casper/src/rust/soak_observer/evaluation.rs:22-23` |
| The response has no selected head field. | The batch adds a response field. | `casper/src/rust/soak_observer/evaluation.rs:279-299` |
| The estimator and its helper functions have no work meter. | The batch adds metered functions to 4 consensus files. | `casper/src/rust/estimator.rs:62-69`, `casper/src/rust/util/dag_operations.rs:53-57`, `casper/src/rust/util/proto_util.rs:162-166`, `casper/src/rust/finality/floor.rs:652-664` |
| The endpoint does not bind `max_number_of_parents` or the approved block number. | The batch adds a second input record. | `casper/src/rust/soak_observer.rs:20-27` |
| One budget has 4 work paths, and all 4 are in use. | The batch adds 2 work paths. | `shared/src/rust/dag/observation_work.rs:101`, `shared/src/rust/dag/observation_work.rs:158-161` |
| The production caller filters the latest messages before it calls the estimator. | The measured evaluation must apply the same filter. | `casper/src/rust/engine/multi_parent_casper/snapshot.rs:132-174` |
| `compute_snapshot` updates production caches. | The observer must not call it. | `docs/plans/casper-node-observation-batch-b.md:32` |
| The public main chain API does not run the GHOST function. | The API result cannot be a reference for the head. | `casper/src/rust/engine/multi_parent_casper/snapshot.rs:539-570` |
| The floor derivation writes cached rows to its DAG representation. | Each evaluation needs its own scratch view. | `casper/src/rust/finality/floor.rs:1122` |

## Scope

### Included

1. A request selection for the paired fork-choice observation.
2. One input record and one input digest for the fork-choice evaluations.
3. A measured evaluation that runs the production fork-choice functions on a scratch view.
4. A reference evaluation that reads the immutable capture.
5. A comparison of the 2 results, with separate availability states.
6. Work counters for each of the 2 evaluations, in the shared budget.
7. A capability entry for the new observation.
8. Tests, a bounded model with negative controls, binding records, and a pending claim.

### Excluded

| Excluded item | Owner or cause |
|---------------|----------------|
| Equivocation input capture and the display projection | Batch E |
| A change to the capture content or to the capture schema version | Not necessary for this batch |
| A change to `compute_snapshot`, to parent selection, or to a proposal | Production behavior must not change |
| A change to the public `showMainChain` API | Separate decision. Decision 10 of the section above records the finding. |
| The harness mapping and the harness client | TASK-017-12 on the soak branch |
| Qualification of a live authority profile | The response keeps `live_profile_qualified` equal to `false` |
| A proof that the floor bound never changes the head | The batch reports each comparison. It does not supply the proof. |
| Export of the complete DAG, electorate, or justification artifacts | A separate contract is necessary first |
| A bounded-window capture for a DAG above the capture limits | Batch B1 rule, not changed |
| A retry loop, a pause of production writers, or a fault control | Not part of the observation |

## Exact file list for the file-scope confirmation

The column "CbC tag now" shows the result of `git check-attr cbc` in the checkout on 2026-09-28. A new file shows the result for its proposed path.

### Rust source and tests

| # | File | State | CbC tag now | Planned responsibility |
|---|------|-------|-------------|------------------------|
| 1 | `casper/src/rust/soak_observer.rs` | Changed | mandatory | Bind the fork-choice inputs in the endpoint. Register the new module. |
| 2 | `casper/src/rust/soak_observer/evaluation.rs` | Changed | mandatory | Add the request selection, the response field, the input digest, the 2 evaluations, and the work fields. |
| 3 | `casper/src/rust/soak_observer/fork_choice.rs` | New | unspecified | Hold the reference evaluation and the result types. |
| 4 | `casper/src/rust/estimator.rs` | Changed | unspecified | Add a metered entry point. The current entry point calls it with `NoopWork`. |
| 5 | `casper/src/rust/util/dag_operations.rs` | Changed | unspecified | Add a metered common ancestor function. |
| 6 | `casper/src/rust/util/proto_util.rs` | Changed | unspecified | Add a metered weight read. |
| 7 | `casper/src/rust/finality/floor.rs` | Changed | mandatory | Add a metered `fork_choice_floor` function. |
| 8 | `shared/src/rust/dag/observation_work.rs` | Changed | mandatory | Increase the number of work paths from 4 to 6. |
| 9 | `node/src/rust/soak_observer.rs` | Changed | mandatory | Add the capability entry `fork_choice`. |
| 10 | `node/tests/soak_observer.rs` | Changed | mandatory | Test the capability entry and the request admission. |
| 11 | `casper/tests/soak_observer.rs` | Changed | mandatory | Test the 2 evaluations, the comparison, the controls, and the limits. |

### Formal files

| # | File | State | CbC tag now | Planned responsibility |
|---|------|-------|-------------|------------------------|
| 12 | `formal/tlaplus/node_observation/PairedForkChoice.tla` | New | mandatory | Model the pairing, the availability states, and the comparison rule. |
| 13 | `formal/tlaplus/node_observation/MC_PairedForkChoice.tla` | New | mandatory | Positive model configuration module. |
| 14 | `formal/tlaplus/node_observation/MC_PairedForkChoice.cfg` | New | mandatory | Positive configuration. |
| 15 to 24 | 5 negative controls, each with one `.tla` file and one `.cfg` file | New | mandatory | One control for each model property. |
| 25 | `formal/tlaplus/node_observation/verification-plan.json` | Changed | mandatory | Add 6 model entries. |
| 26 | `formal/tlaplus/node_observation/bindings.json` | Changed | mandatory | Add the entry for the new claim. |
| 27 | `formal/tlaplus/node_observation/README.md` | Changed | mandatory | Add the model description and the applicability review. |

The proposed control suffixes are `digest_unsafe`, `floor_unsafe`, `absent_unsafe`, `compare_unsafe`, and `budget_unsafe`.

### Gates, tags, and documents

| # | File | State | CbC tag now | Planned responsibility |
|---|------|-------|-------------|------------------------|
| 28 | `scripts/ci/check-tla-invariants.sh` | Changed | mandatory | Register the positive model and the 5 controls. |
| 29 | `scripts/ci/test-check-tla-invariants.sh` | Changed | mandatory | Add the new model family to the test of unregistered controls. |
| 30 | `.gitattributes` | Changed | unspecified | Add the mandatory tag for the new and the newly tagged files. |
| 31 | `docs/plans/casper-node-observation-batch-d.md` | New | unspecified | The accepted form of this plan. |
| 32 | `docs/claims/casper-node-fork-choice-observation.md` | New | unspecified | The pending claim. |
| 33 | `docs/work-logs/task-019-9-paired-fork-choice.md` | New | unspecified | The work log. |
| 34 | `docs/ToDos.md` | Changed | unspecified | Add the task record to EPIC-019. |

### Evidence records

Each mandatory file has one ledger record in `docs/cbc-evidence/`. These records follow from the list above. They are not separate design decisions.

| Group | Count | Note |
|-------|-------|------|
| Changed ledger records | 12 | Files 1, 2, 7, 8, 9, 10, 11, 25, 26, 27, 28, and 29 |
| New ledger records | 14 | File 3 and the 13 new model files |
| New ledger records, conditional | 3 | Files 4, 5, and 6, if the maintainer gives them the mandatory tag |
| New run package | 1 directory | `docs/cbc-evidence/runs/` with the compact report of the verification |

### Conditional file

| File | Condition | CbC tag now |
|------|-----------|-------------|
| `formal/rocq/node_authority/theories/AuthorityWork.v` | Changed only if the work model contains the number of paths. | mandatory |

### Totals

| Group | New | Changed |
|-------|-----|---------|
| Rust source and tests | 1 | 10 |
| Formal files | 13 | 3 |
| Gates, tags, and documents | 3 | 4 |
| **Scope for the confirmation** | **17** | **17** |
| Evidence records | 14 to 17, and 1 run package | 12 |

## Inputs that one capture must bind

### Current behavior

The authority digest covers the capture digest, the 6 authority inputs, and the request. The reference is `casper/src/rust/soak_observer/evaluation.rs:465-473`.

The live fork choice reads 2 values that the endpoint does not bind. The section "Source findings" holds the references.

### Proposal

The endpoint binds a second record, `ForkChoiceInputs`, at attachment. `AuthorityInputs` does not change.

| Field | Source | Cause |
|------------------|--------|-------|
| `max_number_of_parents` | The adopted shard configuration of the instance | Input of the estimator |
| `approved_block_number` | The approved block of the instance | The estimator reads the hash and the number of its floor |
| `latest_message_depth` | The constant of the estimator | The reference needs the same filter value |

The threshold, `max_parent_depth`, and the approved block hash stay in `AuthorityInputs`.

The fork-choice input digest has the domain text `batch-d-fork-choice-v1`. It covers these items:

1. The authority digest, which contains the capture digest.
2. The `ForkChoiceInputs` record.
3. The name of the latest message scope, `captured_latest_messages`.
4. The name of the lower bound rule of each evaluation.

Each result carries this digest. A comparison uses 2 results only when their digests are equal.

The 2 evaluations read the latest messages of the capture. They do not read the live DAG.

## The 2 evaluations and their outputs

### Current behavior

The production caller does these steps in sequence. The section "Source findings" holds the references.

1. Remove the invalid latest messages.
2. Remove each latest message that is not the testimony of its validator.
3. Calculate the fork-choice floor from the remaining latest messages.
4. Call the estimator with the floor, the 2 parent limits, and the remaining latest messages.
5. Use the first tip as the head.

### Proposal: `bounded` evaluation

The `bounded` evaluation runs the production functions with the checked meter on its own scratch view.

1. Build a new scratch view from the capture.
2. Apply the 2 filters of the production caller with the metered lookup functions.
3. Call the metered `fork_choice_floor` function with the approved block metadata from the capture.
4. Call the metered estimator entry point with the adopted parent limits.
5. Record the lower bound, the common ancestor, the head, the tips, and the scores of the tips.

The filter in step 2 is a copy of the caller logic. It is not the same code. Decision 3 of the section above applies.

The metered entry points must give the same result and the same error as the current functions. A test must show this for each changed function.

### Proposal: `reference` evaluation

The `reference` evaluation reads the immutable maps of the capture. It calls no production estimator, common ancestor, floor, or traversal function.

1. Validate the held metadata, the parent order, and the block numbers.
2. Apply rule R-FILTER and the testimony filter with independent code.
3. Apply the depth filter of rule R-LCA with the bound value from the input record.
4. Use the approved block as the lower bound. Decision 1 of the section above applies.
5. Calculate the common ancestor with an independent walk.
6. Add the weights along the main-parent chains, with checked integer arithmetic.
7. Select the head with the descent of rule R-GHOST and the order of rule R-TOTAL.
8. Apply the limits of rules R-COUNT and R-DEPTH to the other tips.

The specification of these rules is `docs/casper/theory/fork-choice/fork-choice-specification.md:26-84`.

The reference gives an unavailable result when the captured history does not reach its lower bound.

### Outputs

The response gets one new field, `fork_choice`, with the type `Value<ForkChoiceObservation>`.

| Field | Content |
|------------------|---------|
| `input_digest` | The fork-choice input digest |
| `inputs` | The `ForkChoiceInputs` record |
| `latest_messages` | 5 counts: captured, invalid, not held, not own testimony, and used |
| `bounded` | `Value<ForkChoiceResult>` |
| `reference` | `Value<ForkChoiceResult>` |
| `comparison` | `Value<ForkChoiceComparison>` |

| Field of `ForkChoiceResult` | Content |
|----------------------------------------|---------|
| `mode` | `bounded` or `reference` |
| `lower_bound` | Hash, block number, and the rule name `finalized_floor` or `approved_block` |
| `common_ancestor` | Hash |
| `head` | Hash of the first tip |
| `tips` | The ordered tips, with a maximum of 64 entries |
| `tip_scores` | The score of each tip |
| `score_count` and `score_digest` | The size and the digest of the complete score map |
| `visited_blocks` and `examined_edges` | Counts from the operation sites. Decision 6 of the section above applies. |

| Field of `ForkChoiceComparison` | Content |
|--------------------------------------------|---------|
| `algorithm` | `immutable-ghost-reference-v1` |
| `head_matches` | Boolean |
| `tips_match` | Boolean |
| `bounds_differ` | Boolean. It is true when the 2 lower bounds are different blocks. |

A head field never contains a finalized floor or an oracle result as a substitute. A missing head has the state unavailable or failed, with a reason.

A mismatch is an available comparison with `head_matches` equal to `false`. It is not an error of the request.

## Work bounds and deadline rules

### Current behavior

One request has one budget with 4 paths and one deadline. The controller limits the deadline to 30 seconds. The section "Source findings" holds the references.

### Proposal

| Rule | Proposal |
|------|----------|
| Budget | The 2 evaluations use the budget of the request. No second budget exists. |
| Work paths | Path 4 is the `bounded` evaluation. Path 5 is the `reference` evaluation. |
| Work report | `WorkReport` and `PartialWork` get the fields `fork_choice_bounded` and `fork_choice_reference`. |
| Limits | The ceilings of `WorkLimits` and `CaptureOptions` do not change. |
| Deadline | The 2 evaluations use the deadline of the request. The 30-second maximum does not change. |
| Sequence | The fork-choice evaluations run after the oracle and floor evaluations. |
| Charge sites | Each visited block, each examined edge, each metadata read, and each score addition gets a charge before the operation. |
| Allocation | Each scratch view, each score map, and each visited set gets an allocation charge before its construction. |
| Validators | The capture limit of 64 validators applies. The reference sets no smaller limit. |
| Response size | The result has a fixed maximum size. It must stay in the 1 MiB frame together with 16 target results. |

A metered loop must stop at the limit or at the deadline. A task that continues after the deadline is not permitted.

## Failure and refusal results

| Condition | Result |
|-----------|-------------------|
| The request has no fork-choice selection. | `fork_choice` has the state `not_requested`. The other results do not change. |
| The selection does not request the reference. | `reference` and `comparison` have the state `not_requested`. |
| The capture does not hold the approved block. | Unavailable, reason `approved_block_not_captured` |
| The bound approved block number and the captured number are different. | Unavailable, reason `approved_block_mismatch` |
| The adopted threshold is invalid. | The existing refusal `invalid_adopted_threshold` |
| The floor derivation needs a body that the capture does not hold. | `bounded` is unavailable, reason `missing_body_coverage` |
| The captured history does not reach the lower bound of the reference. | `reference` is unavailable, reason `history_incomplete` |
| The score addition exceeds the integer range. | Failed, reason `score_overflow` |
| The work limit or the deadline stops an evaluation. | Unavailable, with the limit reason. The work report is incomplete and keeps the partial counts. |
| A production function returns a different error. | Failed, with the error class as the reason |
| One of the 2 results is not available. | `comparison` is unavailable, with the reason of the missing result |
| The 2 results have different input digests. | `comparison` is unavailable, reason `input_digest_mismatch` |
| A second request arrives during an evaluation. | The existing refusal `busy` |
| The attached instance changes during the request. | The existing refusal `instance_changed` |

No condition gives a zero, an empty list, or a successful comparison as a substitute for a missing value.

## Wire schema changes and compatibility with the B11 byte schema

### Current behavior

The B11 proofs cover the canonical bytes of the capture. They follow `SnapshotData::encode` and the final work field. The files exist on the soak branch only.

The observer frames are JSON. The B11 proofs do not cover the JSON frames.

### Proposal

| Item | Change | Compatibility |
|------|--------|---------------|
| Capture content and canonical bytes | None | The B11 schema model and its 8 theorems stay applicable. |
| `SNAPSHOT_SCHEMA_VERSION` | None, stays 2 | No effect |
| B11 source manifest | The digests of the changed Rust files change. | The B11 evidence needs a new export on the soak branch after the merge. |
| Observer frame `schema_version` | None, stays 1. Decision 5 of the section above applies. | The change is additive. |
| `AuthorityRequest` | New optional field `fork_choice` with a default of none | A request with no such field stays valid. |
| Request copy in the response | The field is absent when the request has none. | The client check of the request copy stays valid. |
| Authority digest | The digest input omits the absent field. | The digest of a request with no selection does not change. A test must show this. |
| `AuthorityResponse` | New field `fork_choice` | A client that reads named fields is not affected. |
| `WorkReport` and `PartialWork` | 2 new fields | The harness mapping reads named fields. |
| Capability list | New entry `fork_choice` | A client can find an observer with no support before it sends a request. |
| Response scope text | None. The new field has its own scope text `batch-d-paired-fork-choice`. | No effect on Batch B2 consumers |

An observer without Batch D rejects a request that has the new field. The cause is the unknown field rule of the request type.

## Test plan

### Unit tests

| Test subject | Expected result |
|--------------|-----------------|
| Request with no selection | The response bytes of the Batch B2 fields and the authority digest are equal to the results before the change. |
| Linear chain with one validator | The 2 heads are equal to the chain tip. |
| Fork with a strict weight majority | The 2 heads are on the majority branch. |
| Equal weights | The head follows the ascending hash order. |
| Invalid latest message | The validator adds no weight in the 2 evaluations. |
| Latest message that is not own testimony | The validator adds no weight in the 2 evaluations. |
| No latest messages | The only tip is the lower bound. |
| Threshold of 0 | The `bounded` lower bound is the approved block. |
| Threshold above 0 with a finalized floor above the approved block | The 2 lower bounds are different, and `bounds_differ` is true. |
| Parent count limit and the 2 values for no limit | The head stays in the tips. |
| Parent depth limit | The head stays in the tips. |
| Each refusal and failure condition of the table above | The named state and reason |
| Production stores | The bytes before and after the request are equal. |
| Scratch views | The floor rows that one evaluation writes are absent from the other view. |
| Each changed production function | The result and the error are equal to the results of the current function. |

### Property tests

| Property | Domain |
|----------|--------|
| The 2 heads are equal when the 2 lower bounds are equal. | Random DAGs in the capture limits |
| The 2 results do not depend on the insertion order of the latest messages. | Random permutations |
| The head is always a member of the tips. | Random parent limits |
| The work counts of 2 equal requests on equal captures are equal. | Random DAGs |
| The aggregate work is the sum of the path counts. | Random limits |

A head mismatch with different lower bounds is a recorded observation. The property tests do not assume that it cannot occur.

### Negative controls

Each control changes the reference or the binding. The test must then report the named result.

| Control | Required result |
|---------|-----------------|
| Rank the tips by their own scores | Comparison mismatch |
| Add the weight to all parents | Comparison mismatch |
| Reverse the hash order in a tie | Comparison mismatch |
| Omit the invalid message filter | Comparison mismatch |
| Omit the testimony filter | Comparison mismatch |
| Use a lower bound above the correct one | Comparison mismatch or an unavailable result |
| Give the 2 evaluations different captures | `input_digest_mismatch` |
| Put the floor hash in the head field | The schema test fails. |
| Share one scratch view | The scratch view test fails. |
| Remove one charge site | The work limit test fails. |

### Model controls

| Property | Control |
|---------------------|---------|
| `OneCapture` | `digest_unsafe` |
| `HeadNotFloor` | `floor_unsafe` |
| `NoFabricatedHead` | `absent_unsafe` |
| `CompareSameInput` | `compare_unsafe` |
| `SharedBudget` | `budget_unsafe` |

The model has a bounded domain. It does not show the properties for each permitted DAG.

## Implementation steps in order

Steps 1 to 4 must be complete before a production file changes.

1. Get the file-scope confirmation and the answers to the open questions. Done on 2026-09-30.
2. Add the task record to `docs/ToDos.md` and start the work log. Done on 2026-09-30.
3. Register the pending claim and the mandatory tags.
4. Create the pending ledger records for the mandatory files.
5. Write the bounded model and its 5 controls. Register them in the gate.
6. Add the 2 work paths. Run the existing observer tests.
7. Add the metered functions to the 4 consensus files. Run the fork-choice tests and the floor tests.
8. Add the input record and the input digest.
9. Add the `bounded` evaluation and its tests.
10. Add the `reference` evaluation and its tests.
11. Add the comparison, the response field, and the negative controls.
12. Add the capability entry and the node tests.
13. Add the binding entries and the applicability review.
14. Run the formal gate, the binding check, and the test suites. Record the results.
15. Make the compact evidence package in `docs/cbc-evidence/`.
16. Give the package to the named maintainer for the acceptance decision.

## Dependencies

| Dependency | State |
|------------|-------|
| TASK-017-12 session in the shared checkout | Complete for the delivery revision. |
| Checkout of `feature/casper-node-observation` | Done. |
| Correction of the 2 CI failures on the branch | Done in `324146230`. |
| Batch E | Separate batch. It changes the capture. Batch D does not. |
| Merge sequence of Batch D and Batch E | Batch D first, decision 9. Batch E (TASK-019-10) starts after Batch D. |

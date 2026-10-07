---
kind: tdd-plan
scope: issue-102-consensus-stack
produced_by: /task-tdd
produced_at: 2026-08-14T16:14:04Z
glossary: docs/Glossary.md
test_runner: cargo-test
system_boundaries:
  - time
  - cryptographic-signature-generation
conformance_audit:
  status: warnings
  notes:
    - "The issue #102 fix predates this plan, so B1 records a GREEN baseline without a RED result."
    - "The project glossary does not define all consensus terms that these behaviors require."
behaviors:
  - id: B1
    statement: "A canonically rejected deploy remains available and enters a later canonical block."
    priority: must
    deep_module: true
    done: true
    cycle_log:
      - completed_at: 2026-08-14T16:20:44Z
        tracer: true
        test_name: finalized_noncanonical_deploy_is_reproposed_after_canonical_rejection
        test_file: casper/tests/batch2/finalized_win_pending_rejection_spec.rs
        red: "Not applicable because the issue #102 fix predates this regression."
        green: "Focused test passed: 1 passed, 0 failed."
        full_suite: "Casper passed: 837 passed, 0 failed, 10 ignored."
  - id: B2
    statement: "Duplicate sibling inclusions produce one canonical effect."
    priority: must
    deep_module: true
    done: true
    cycle_log:
      - completed_at: 2026-08-14T16:35:08Z
        tracer: false
        test_name: duplicate_sibling_inclusions_must_reconcile_to_one_effect
        test_file: casper/tests/batch2/exactly_once_spec.rs
        red: "The merged state contained two datums instead of one."
        green: "Focused test passed: 1 passed, 0 failed."
        full_suite: "Casper passed: 838 passed, 0 failed, 10 ignored."
  - id: B3
    statement: "Consensus rejects a rejection record that names the wrong carrier."
    priority: must
    deep_module: true
    done: true
    cycle_log:
      - attempted_at: 2026-08-14T16:37:13Z
        blocker: "Git still marks casper/src/rust/api/block_api.rs as unmerged after the resolved B2 conflict."
      - completed_at: 2026-08-14T16:51:59Z
        tracer: false
        test_name: record_carrier_is_consensus_checked
        test_file: casper/tests/batch2/carrier_record_spec.rs
        red: "Checkpoint validation accepted the forged carrier."
        green: "Focused test passed: 1 passed, 0 failed."
        full_suite: "Casper passed: 839 passed, 0 failed, 10 ignored."
  - id: B4
    statement: "Merge application rejects an absent removal and accepts a fully netted no-op."
    priority: must
    deep_module: true
    done: true
    cycle_log:
      - completed_at: 2026-08-14T17:38:19Z
        tracer: false
        test_name: "removal_absent_from_base_is_an_error_not_a_noop; netted_install_fire_pair_merges_as_a_noop"
        test_file: "rspace++/src/rspace/merger/state_change_merger.rs; casper/tests/batch2/netted_noop_merge_spec.rs"
        red: "The absent removal returned success, and the netted change returned a merge error."
        green: "Both focused tests passed."
        full_suite: "RSpace passed 305 tests and Casper passed 840 tests with no failures."
  - id: B5
    statement: "Mergeable-channel garbage collection preserves each block above the finalized floor."
    priority: must
    deep_module: true
    done: true
    cycle_log:
      - attempted_at: 2026-08-14T17:40:30Z
        blocker: "The B4 worktree changes are not staged."
      - completed_at: 2026-08-14T17:51:33Z
        tracer: false
        test_name: a_block_above_the_floor_is_never_collected_however_far_the_tip_has_run
        test_file: casper/src/rust/util/mergeable_channels_gc.rs
        red: "The tip-based rule marked a block above the floor as safe to delete."
        green: "Focused test passed: 1 passed, 0 failed."
        full_suite: "Casper passed: 840 passed, 0 failed, 10 ignored."
  - id: B6
    statement: "A carrier outside the floor window is rejected without applying its effect."
    priority: must
    deep_module: true
    done: true
    cycle_log:
      - completed_at: 2026-08-14T18:53:57Z
        tracer: false
        test_name: late_carrier_past_window_is_rejected_with_record_and_without_effect
        test_file: casper/tests/batch2/merge_window_spec.rs
        red: "The late carrier merged without a rejection record."
        green: "Focused test passed: 1 passed, 0 failed."
        full_suite: "Casper passed: 841 passed, 0 failed, 10 ignored."
---

# TDD Plan -- Issue #102 and Consensus Stack

This plan verifies issue #102 and the ordered changes in PRs #248 through #252. Each stack behavior runs against its immediate parent.

## Public Interface

- **Name:** Consensus block processing
- **Signature surface:** `TestNode` block proposal, block admission, finalization, and observable post-state queries
- **Invariants:** Canonical state contains each accepted effect once. Rejected effects remain recoverable during their valid window.
- **Error modes:** Invalid records and incoherent merge changes return explicit errors. Invalid effects do not enter canonical state.
- **Performance characteristics:** These focused tests do not measure performance.

## System Boundaries

Mocks are permitted only at these boundaries:

- **Time:** Tests can control deploy timestamps and short scheduling delays.
- **Cryptographic signature generation:** Tests can use deterministic repository test keys.

Repository consensus, storage, merge, and execution components are inside the system boundary. Tests must use real repository implementations.

## Behavior Checklist

- [x] **B1** -- A canonically rejected deploy remains available and enters a later canonical block.
- [x] **B2** -- Duplicate sibling inclusions produce one canonical effect.
- [x] **B3** -- Consensus rejects a rejection record that names the wrong carrier.
- [x] **B4** -- Merge application rejects an absent removal and accepts a fully netted no-op.
- [x] **B5** -- Mergeable-channel garbage collection preserves each block above the finalized floor.
- [x] **B6** -- A carrier outside the floor window is rejected without applying its effect.

## Layer Sequence

| Behavior | RED base | GREEN change | Focused test |
|---|---|---|---|
| B1 | Not applicable | Current `dev` | `finalized_noncanonical_deploy_is_reproposed_after_canonical_rejection` |
| B2 | Current `dev` | PR #248 | `duplicate_sibling_inclusions_must_reconcile_to_one_effect` |
| B3 | PR #248 | PR #249 | `record_carrier_is_consensus_checked` |
| B4 | PR #249 | PR #250 | `removal_absent_from_base_is_an_error_not_a_noop` and `netted_install_fire_pair_merges_as_a_noop` |
| B5 | PR #250 | PR #251 | `a_block_above_the_floor_is_never_collected_however_far_the_tip_has_run` |
| B6 | PR #251 | PR #252 | `late_carrier_past_window_is_rejected_with_record_and_without_effect` |

## Cycle Rules

1. Apply one acceptance test to its RED base.
2. Run the focused test.
3. Require an assertion failure that matches the behavior.
4. Do not accept a compile failure as RED evidence.
5. Apply the matching PR change.
6. Run the focused test again.
7. Require GREEN before the next behavior.
8. Record the command and result in this plan.

## Cycle Log

### B1 -- Canonically Rejected Deploy Recovery -- 2026-08-14T16:20:44Z

- **tracer:** true
- **test_name:** `finalized_noncanonical_deploy_is_reproposed_after_canonical_rejection`
- **test_file:** `casper/tests/batch2/finalized_win_pending_rejection_spec.rs`
- **files_changed:**
  - `casper/tests/batch2/finalized_win_pending_rejection_spec.rs`
- **runner_output:**
  - RED: Not applicable because the issue #102 fix predates this regression.
  - GREEN: The focused test passed with one test and no failures.
  - Full suite: Casper passed 837 tests with no failures and 10 ignored tests.
- **observations:**
  - The ordinary deploy path logged `FILTERED (already in scope)` before the recovery path re-proposed the deploy.
  - Finalization of the rejected carrier did not remove the recovery copy.
- **refactor_performed:**
  - None.
- **refactor_deferred:**
  - None.
- **new_behaviors_surfaced:**
  - None.
- **blockers:**
  - None.

### B2 -- Duplicate Sibling Inclusion -- 2026-08-14T16:35:08Z

- **tracer:** false
- **test_name:** `duplicate_sibling_inclusions_must_reconcile_to_one_effect`
- **test_file:** `casper/tests/batch2/exactly_once_spec.rs`
- **files_changed:**
  - `casper/src/rust/blocks/proposer/block_creator.rs`
  - `casper/src/rust/merging/dag_merger.rs`
  - `casper/src/rust/util/rholang/interpreter_util.rs`
  - `casper/src/rust/util/rholang/runtime_manager.rs`
  - `casper/tests/batch2/dedup_orphan_recovery_spec.rs`
  - `casper/tests/batch2/exactly_once_spec.rs`
  - `casper/tests/batch2/mod.rs`
  - `casper/tests/batch2/multi_validator_recovery_spec.rs`
  - `casper/tests/batch2/slash_recovery_spec.rs`
  - `casper/tests/block_creator_memory_profile_spec.rs`
  - `casper/tests/compute_parents_post_state_regression_spec.rs`
  - `casper/tests/util/rholang/runtime_manager_test.rs`
- **runner_output:**
  - RED: The merged state contained two datums instead of one.
  - GREEN: The focused test passed with one test and no failures.
  - Full suite: Casper passed 838 tests with no failures and 10 ignored tests.
- **observations:**
  - The RED result matched the duplicate-effect behavior.
  - The settled-in-base rule removed the second effect without a rejection record.
  - Current exploratory-deploy behavior replaced an obsolete PR #248 call site during conflict resolution.
- **refactor_performed:**
  - Reduced the imported PR test to the single B2 behavior.
- **refactor_deferred:**
  - The second PR #248 acceptance behavior remains outside this plan.
- **new_behaviors_surfaced:**
  - None.
- **blockers:**
  - None.

### B3 -- Wrong Rejection-Record Carrier -- 2026-08-14T16:37:13Z

- **status:** blocked before RED
- **blocker:**
  - Git still marks `casper/src/rust/api/block_api.rs` as unmerged after the resolved B2 conflict.
- **required action:**
  - Stage the resolved worktree before another B3 invocation.

### B3 -- Wrong Rejection-Record Carrier -- 2026-08-14T16:51:59Z

- **tracer:** false
- **test_name:** `record_carrier_is_consensus_checked`
- **test_file:** `casper/tests/batch2/carrier_record_spec.rs`
- **files_changed:**
  - `block-storage/src/rust/key_value_block_store.rs`
  - `casper/src/rust/blocks/proposer/block_creator.rs`
  - `casper/src/rust/engine/multi_parent_casper/snapshot.rs`
  - `casper/src/rust/merging/dag_merger.rs`
  - `casper/src/rust/test_utils/helper/block_generator.rs`
  - `casper/src/rust/util/proto_util.rs`
  - `casper/src/rust/util/rholang/interpreter_util.rs`
  - `casper/src/rust/util/rholang/runtime_manager.rs`
  - `casper/tests/api/deploy_finalization_status_test.rs`
  - `casper/tests/batch2/carrier_record_spec.rs`
  - `casper/tests/batch2/dedup_orphan_recovery_spec.rs`
  - `casper/tests/batch2/mod.rs`
  - `casper/tests/batch2/multi_validator_recovery_spec.rs`
  - `casper/tests/batch2/recovery_repeat_deploy_misfire_spec.rs`
  - `casper/tests/batch2/validate_test.rs`
  - `casper/tests/compute_parents_post_state_regression_spec.rs`
  - `casper/tests/helper/block_generator.rs`
  - `casper/tests/slashing/integration_helpers.rs`
  - `casper/tests/util/rholang/interpreter_util_test.rs`
  - `casper/tests/util/rholang/runtime_manager_test.rs`
  - `models/src/main/protobuf/CasperMessage.proto`
  - `models/src/rust/casper/protocol/casper_message.rs`
- **runner_output:**
  - RED: Checkpoint validation accepted the forged carrier.
  - GREEN: The focused test passed with one test and no failures.
  - Full suite: Casper passed 839 tests with no failures and 10 ignored tests.
- **observations:**
  - The RED test injected the future carrier field through protobuf wire bytes.
  - PR #248 discarded the unknown field and accepted the block.
  - PR #249 retained the field and compared the full rejection record.
  - Rust Analyzer retained types from before the protobuf change. Cargo check and the full suite compiled the new schema.
- **refactor_performed:**
  - Kept only the carrier-validation behavior from the PR #249 test file.
- **refactor_deferred:**
  - The dropped-record behavior remains outside this plan.
- **new_behaviors_surfaced:**
  - None.
- **blockers:**
  - None.

### B4 -- Merge-Application Hardening -- 2026-08-14T17:38:19Z

- **tracer:** false
- **test_name:**
  - `removal_absent_from_base_is_an_error_not_a_noop`
  - `netted_install_fire_pair_merges_as_a_noop`
- **test_file:**
  - `rspace++/src/rspace/merger/state_change_merger.rs`
  - `casper/tests/batch2/netted_noop_merge_spec.rs`
- **files_changed:**
  - `casper/tests/batch2/mod.rs`
  - `casper/tests/batch2/netted_noop_merge_spec.rs`
  - `rspace++/src/rspace/merger/state_change_merger.rs`
- **runner_output:**
  - RED: An absent removal returned success instead of an error.
  - RED: A fully netted change returned an empty-consume merge error.
  - GREEN: Both focused tests passed.
  - Full RSpace suite: 305 passed, no failures, and 5 ignored.
  - Full Casper suite: 840 passed, no failures, and 10 ignored.
- **observations:**
  - Residual removals now fail when the base does not contain the removed value.
  - Empty datum and continuation changes now produce no trie action.
  - The first parallel Casper run timed out. The isolated rerun passed.
- **refactor_performed:**
  - Reduced the netted-change test to its observable merge and state assertions.
- **refactor_deferred:**
  - None.
- **new_behaviors_surfaced:**
  - None.
- **blockers:**
  - None.

### B5 -- Finalized-Floor Garbage Collection -- 2026-08-14T17:40:30Z

- **status:** blocked before RED
- **blocker:**
  - The B4 worktree changes are not staged.
- **required action:**
  - Stage the B4 worktree before another B5 invocation.

### B5 -- Finalized-Floor Garbage Collection -- 2026-08-14T17:51:33Z

- **tracer:** false
- **test_name:** `a_block_above_the_floor_is_never_collected_however_far_the_tip_has_run`
- **test_file:** `casper/src/rust/util/mergeable_channels_gc.rs`
- **files_changed:**
  - `casper/src/rust/util/mergeable_channels_gc.rs`
- **runner_output:**
  - RED: The tip-based rule marked block 12 as safe to delete above floor 10.
  - GREEN: The focused test passed with one test and no failures.
  - Full Casper suite: 840 passed, no failures, and 10 ignored.
- **observations:**
  - A test-only adapter kept the B5 assertion stable across the private signature change.
  - Garbage collection now derives one floor per pass.
  - Deletion depth now uses the floor instead of the tip.
- **refactor_performed:**
  - Kept only the above-floor preservation test from PR #251.
- **refactor_deferred:**
  - The other PR #251 boundary tests remain outside this plan.
- **new_behaviors_surfaced:**
  - None.
- **blockers:**
  - None.

### B6 -- Floor-Window Carrier Rejection -- 2026-08-14T18:53:57Z

- **tracer:** false
- **test_name:** `late_carrier_past_window_is_rejected_with_record_and_without_effect`
- **test_file:** `casper/tests/batch2/merge_window_spec.rs`
- **files_changed:**
  - `casper/src/rust/api/deploy_finalization_status.rs`
  - `casper/src/rust/blocks/proposer/block_creator.rs`
  - `casper/src/rust/engine/multi_parent_casper/finalization_runner.rs`
  - `casper/src/rust/engine/multi_parent_casper/validation_dispatcher.rs`
  - `casper/src/rust/finality/floor_context.rs`
  - `casper/src/rust/finality/mod.rs`
  - `casper/src/rust/merging/block_index.rs`
  - `casper/src/rust/merging/dag_merger.rs`
  - `casper/src/rust/merging/deploy_chain_index.rs`
  - `casper/src/rust/test_utils/helper/block_generator.rs`
  - `casper/src/rust/util/rholang/interpreter_util.rs`
  - `casper/src/rust/validate.rs`
  - `casper/tests/api/deploy_finalization_status_test.rs`
  - `casper/tests/batch2/carrier_record_spec.rs`
  - `casper/tests/batch2/dedup_orphan_recovery_spec.rs`
  - `casper/tests/batch2/merge_window_spec.rs`
  - `casper/tests/batch2/mod.rs`
  - `casper/tests/batch2/multi_validator_recovery_spec.rs`
  - `casper/tests/batch2/recovery_cycle_spec.rs`
  - `casper/tests/batch2/single_parent_casper_spec.rs`
  - `casper/tests/batch2/slash_recovery_spec.rs`
  - `casper/tests/batch2/validate_test.rs`
  - `casper/tests/block_creator_memory_profile_spec.rs`
  - `casper/tests/blocks/block_creator_spec.rs`
  - `casper/tests/compute_parents_post_state_regression_spec.rs`
  - `casper/tests/genesis/genesis_test.rs`
  - `casper/tests/helper/block_generator.rs`
  - `casper/tests/helper/test_node.rs`
  - `casper/tests/merging/merge_number_channel_spec.rs`
  - `casper/tests/slashing/integration_helpers.rs`
  - `casper/tests/util/rholang/interpreter_util_test.rs`
  - `casper/tests/util/rholang/runtime_manager_test.rs`
- **runner_output:**
  - RED: The late carrier merged without a rejection record.
  - GREEN: The focused test passed with one test and no failures.
  - Full Casper suite: 841 passed, no failures, and 10 ignored.
- **observations:**
  - Test-fixture plumbing supplied a five-block deploy lifespan before RED.
  - PR #252 derives one floor context for each propose or validation operation.
  - The merge now rejects a carrier after the floor closes its deploy window.
  - The rejected carrier did not apply its effect or remove valid in-window history.
- **refactor_performed:**
  - Kept one B6 acceptance test and the required compatibility updates.
- **refactor_deferred:**
  - The additional PR #252 tests remain outside this plan.
- **new_behaviors_surfaced:**
  - None.
- **blockers:**
  - None.

## Glossary Anchors Used

The plan uses [LFB convergence spread](../Glossary.md#lfb-convergence-spread) when it discusses agreement on finalized state.

The code and linked PRs currently define the other consensus terms. The conformance warning remains until the glossary defines those terms.

## Completion

All six behaviors are GREEN. The final Casper suite passed with 841 tests, no failures, and 10 ignored tests.

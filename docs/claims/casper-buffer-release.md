# Claim: casper buffer release liveness, no stale hold, and release order

```yaml
claim_id: CLAIM-CASPER-BUFFER-001
artifacts:
  - casper/src/rust/blocks/block_processor.rs                          # dependency check, buffer commit, check_if_of_interest
  - casper/src/rust/engine/multi_parent_casper/buffer_resolver.rs      # release scan (get_dependency_free_from_buffer)
  - node/src/rust/instances/block_processor_instance.rs                # block queue and release re-enqueue
  - block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage.rs  # buffer relations and pendants
  - block-storage/src/rust/dag/buffer_dag_transition.rs              # atomic dependency commit (B6)
  - casper/src/rust/engine/multi_parent_casper/block_admission.rs      # buffer_waits_on_dependency verdict (B5)
  - node/src/rust/instances/release_queue.rs                           # release queue and scheduler (B7, B10, B11)
  - casper/src/rust/casper.rs                                          # buffer_waits_on_dependency trait query (B5)
  - casper/src/rust/engine/block_retriever.rs                          # recovery re-request counter (B4)
  - casper/src/rust/engine/running.rs                                   # integrity check at ingress, before the in-flight mark (C7)
  - casper/src/rust/engine/peer_scoring.rs                              # strike for a peer whose block fails the integrity check (C7)
  - block-storage/src/rust/key_value_block_store.rs                     # compare-and-delete of a refused stored copy (C7)
  - block-storage/src/rust/pending_blocks.rs                            # pending-area eviction tiers (C8)
status: pending
adapter: agentic
mechanization: none   # planned in the discharge plan below
task: TASK-021-12, TASK-021-18
tdd_plan: docs/tdd-plans/issue-24-parked-block-release-2026-10-07.md
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/24   # sustained finalization lag
  - GitHub Actions run 37469364217                          # dev 778cc6754: 8 of 49 test_load failures
```

## Context

[Issue #1188](https://github.com/F1R3FLY-io/f1r3node-rust/issues/1188) reviews this claim after block ingest replaced the Casper buffer and block retriever.
The current conditions describe the owner, dependency tracker, validation workers, and fetch scheduler.
Issue #1196 covers C1, C2, C4, C5, C6, and C8. Their evidence stays pending for maintainer review.
The artifact list and historical discharge plan record the scope of the earlier claim.

A received block parks in the casper buffer while one of its dependencies (a parent or a justification) is not validated. Issue #24 measured that under sustained load 56% of received blocks park, for 7.1 s median and 39.6 s p90, while replay takes 0.5 to 1.1 s. Parked blocks stretch the finality rounds, and the finalization tail fails the `test_load` 45 s gate.

A code trace on 2026-10-07 found three release-path causes. A released block re-enters the tail of the one shared processing queue, behind gossip. The release scan reads every buffered block after every processed block. A block with a stale parent link can be dropped as already processed and stay parked until the 180 s stale prune.

TASK-021-12 changes the release path. This claim states the properties that the change must preserve or establish. The claim is pending: no code change for TASK-021-12 starts before this file exists (EPIC-021 `cbc_policy`).

TASK-021-18 (stall-loop item A7) extends the claim with C5 and C6. A code reading of `dev` on 2026-10-16 found two defects that do not depend on load:

- The overflow prune of the buffer sorts dependency-free entries youngest first and removes the first one. It removes the newest block, which is often the block that just became ready.
- A recovery re-request for a buffered block gets a duplicate copy as its reply. The copy has the verdict `AlreadyProcessed`, and its disposal calls `ack_processed`. That call clears the re-fetch cooldown, the retry counter, and the quarantine of the block. The retry budget then never runs out.

## Historical definitions

- **Validated:** a block hash is in the block DAG or in the invalid-block set.
- **Ready:** every dependency of a buffered block is validated.
- **Released:** a ready block is given to the processing pipeline.
- **Stale link:** a buffer relation from a block to a dependency that is already validated.

## Claim statements

**C1. Release liveness.** The owner puts a retained waiting block in the ready set when it handles the event that clears the block's last wait. A wait can end when a dependency insert completes, a state root is imported, or a retry becomes due.

If a worker owns the inserted dependency, the owner completes the insert when it handles the worker's outcome. The owner checks deferred block shape before dispatch and adds any further dependencies it finds. C3 governs selection for a free validation slot.

**C2. No stale dependency wait.** When the owner completes a dependency insert, it removes that dependency from the missing set of each waiter. If a worker reports a missing dependency and a fresh store read finds it satisfied, the owner retries the block without adding a wait. The retry counts toward the validation failure limit. At that limit, the owner stops the attempt sequence and quarantines the block.

**C3. Bounded dispatch wait.** The block ingest dispatcher selects the lowest-height ready entry when no entry is overdue. An entry becomes overdue after `K = READY_AGE_LIMIT_DISPATCHES = 32` dispatches of other entries. The dispatcher selects overdue entries in the order they entered the ready set.

- A ready entry is a held block that the tracker can select for a worker.
- The count starts when the tracker puts the entry in the ready set. A return to the ready set after waiting or retry starts a new count.
- Let `A` be the number of entries already ahead in ready-set entry order when the entry becomes ready. An entry that leaves and returns receives a new place in that order.
- If the entry stays ready and validation slots keep freeing, the dispatcher selects it within `K + A + 1` dispatch opportunities, including its own dispatch. Each opportunity selects one entry for a free slot.

After `K` other dispatches, the entry is overdue. At most `A` entries can still precede it. Later entries cannot take priority over it while it is overdue. This claim counts dispatches from readiness to selection. Worker run time and wall-clock delay are outside this bound.

**C4. Verdict invariance for released blocks.** For the same initial DAG and validation inputs, changing the dispatch order of independent released blocks preserves each block's verdict, DAG membership, and latest messages. The comparison includes the default order and the order when an overdue block takes priority under C3. Both schedules start with the blocks waiting on dependencies. For each schedule, the owner releases each block after the dependencies it needs are satisfied.

**C5. Eviction order.** When held disk bodies exceed the limit, the owner takes an eligible victim from the sender with the highest held disk count. Equal sender counts sort by validator key. For both disk and pending storage, entries with active workers retain their bodies.

The owner prefers unsolicited victims before solicited victims where both classes are eligible. Another held block waiting on an entry makes that entry solicited for eviction. Within an eligible class, the owner selects untrusted entries first, then higher blocks, then entries held more recently at the same height. C8 defines when the pending policy may select each class.

**C6. Fetch and validation retry pacing.** The fetch scheduler paces unanswered network requests for a need. An announcement may move the next request earlier. The base interval from the last request still applies. A duplicate intake report for a held block leaves the held entry and its validation retry state intact.

A validation failure without a verdict schedules a separate retry. The delay increases up to the configured limit. When the failure count reaches its limit, the owner quarantines the block. A later intake report during quarantine defers its need until quarantine expires. A completed DAG insert clears the need and the validation failure count.

**C7. Integrity before hash-keyed state.** No step reads or changes state keyed by a block hash until the fields, the signature, and the hash of the block are checked against its contents. That state is the in-flight set, the buffer, the pending area, the block store, and the request tracking of the retriever. A message that fails the check from the network changes none of that state for the hash that it claims. A refused body that this node stored is purged and deleted, but only when the stored bytes equal the refused body.

**C8. Pending admission and eviction.** Intake classifies checked blocks as solicited, unsolicited from a known sender, or unsolicited from an unknown sender. A known sender passed the signature check and has a bond at the last finalized block. Class limits on entries and bytes preserve capacity for known senders and solicited blocks. Pending storage counts reservations against the capacity available to other entries.

The owner's proactive passes select unsolicited entries using the class limits and the per-sender low-water mark. To make room for a solicited block that exceeded a global limit, the owner selects unsolicited victims first. It can then select solicited entries only above the refused block's height.

At a sender limit, the owner selects that sender's unsolicited entries first. It can then select solicited entries from that sender that wait on the refused block. When a pass finds enough room, the owner reserves the capacity before it frees the selected entries.

## Block ingest review

The review uses block ingest on `dev` at `adbfc74db` and the dispatch policy from PR #1191. The claim stays pending.

### C3 acceptance

The maintainer accepted the restated C3 on 2026-11-27 at `c7338aa51d3e38e94a46726894a25d751c2e079b`. The acceptance applies to these source bytes:

```text
0bbfc2183a1beeaecbf73e2051d0f4aeabca203bcb9b0be8f555e9f1daab60c1  casper/src/rust/blocks/ingest/dependency_tracker.rs
1a258bd7d0a3efb8bd698c15f51e996e162b9f2aa73550fe95223390b27948fc  casper/src/rust/blocks/ingest/ingest_state_tests.rs
```

A later change to either file needs a new review of C3. The other conditions stay pending: C1, C2, C4, C5, C6, and C8 under issue #1196, and C7 under issue #1194. C4 stays open because the age bound changes the order of released blocks, and the existing test covers only independent ready blocks. The claim status stays `pending` until every condition has a recorded result.

### C3 evidence

The first regression checks that lower heights take dispatches 1 through 32 and the ready height-5 block takes dispatch 33. The second checks two overdue blocks first held ready. They take dispatches 33 and 34 in ready order even though the second has a lower height. [Issue #1188 test results](https://github.com/F1R3FLY-io/f1r3node-rust/issues/1188#issuecomment-7222005932).

- [`a_ready_block_is_dispatched_while_one_sender_keeps_sending_lower_heights`](../../casper/src/rust/blocks/ingest/ingest_state_tests.rs)
- [`overdue_blocks_are_dispatched_in_the_order_they_became_ready`](../../casper/src/rust/blocks/ingest/ingest_state_tests.rs)

At `c286f1ef9`, the tracker broke equal dispatch-count ties by the sequence at first hold. A scratch test held one block waiting and then held a second block ready. The first block became ready before the next dispatch. The second block had `A = 0` and took dispatch 34. The bound requires dispatch by 33 in that case.

The [tracker](../../casper/src/rust/blocks/ingest/dependency_tracker.rs) now assigns a distinct sequence on each entry to the ready set. The added regression reverses hold and ready order within one dispatch interval. It checks that the block with `A = 0` takes dispatch 33.

- [`a_block_held_earlier_but_ready_later_does_not_pass_an_older_ready_block`](../../casper/src/rust/blocks/ingest/ingest_state_tests.rs)

### Other conditions

#### C1. Release liveness

**Pending.** The [owner tests](../../casper/src/rust/blocks/ingest/ingest_state_tests.rs) cover the events that clear waits.

- `a_block_is_dispatched_once_its_parents_are_inserted` and `a_block_is_dispatched_once_its_parent_is_inserted_when_the_outcome_arrives_first` cover the two orders of the insert and worker outcome.
- `a_deferred_block_cites_the_rest_once_its_main_parent_shows_it_fits` covers dependencies found by the deferred shape check.
- `a_block_awaiting_a_state_root_is_released_when_it_is_imported` and `a_failed_validation_waits_before_its_retry` cover root import and the retry timer.
- [`a_child_received_while_its_parent_is_validated_is_validated_without_a_request`](../../node/tests/block_queue_livelock.rs) exercises release through the node.

#### C2. No stale dependency wait

**Pending.** [`a_block_is_ready_once_its_last_dependency_is_inserted`](../../casper/src/rust/blocks/ingest/dependency_tracker.rs) checks that an insert clears the wait. [`a_validation_that_keeps_naming_a_dependency_in_the_dag_is_bounded`](../../casper/src/rust/blocks/ingest/ingest_state_tests.rs) checks that repeated obsolete results reach the failure limit.

#### C4. Verdict invariance for released blocks

**Pending.** [`released_blocks_keep_their_verdicts_and_dag_in_both_dispatch_orders`](../../casper/tests/blocks/release_order_invariance_spec.rs) holds two valid blocks on different parents. The test drives the production dependency tracker, records its selected order, and validates the released blocks in that order. It uses the same blocks and checks their verdicts, DAG membership, and latest messages in both runs.

The second run ages one released block by `READY_AGE_LIMIT_DISPATCHES` dispatches of fixture work. At the next selection, the test asserts that the overdue block takes the slot while a lower entry is ready. The C4 coverage here uses two independent valid blocks and one validation order per schedule.

The test depends on the scheduler and threshold defined by [PR #1191](https://github.com/F1R3FLY-io/f1r3node-rust/pull/1191). Both order-invariance tests passed in the build of that PR at `0f67c82f1`.

The earlier [`ready_blocks_reach_the_same_dag_and_verdicts_in_every_processing_order`](../../casper/tests/blocks/release_order_invariance_spec.rs) remains evidence for three independent blocks that start ready.

#### C5. Eviction order

**Pending.** [`victims_are_untrusted_first_then_highest_first_and_never_in_work`](../../casper/src/rust/blocks/ingest/dependency_tracker.rs) checks victim order and protects working entries. [`a_held_block_another_held_block_waits_on_counts_as_solicited`](../../casper/src/rust/blocks/ingest/dependency_tracker.rs) checks the solicited class. [`a_sender_flooding_the_disk_bound_cannot_evict_other_senders_blocks`](../../casper/src/rust/blocks/ingest/ingest_state_tests.rs) exercises disk eviction under pressure from one sender.

#### C6. Fetch and validation retry pacing

**Pending.** The [fetch scheduler tests](../../casper/src/rust/blocks/ingest/fetch_scheduler.rs) and [owner tests](../../casper/src/rust/blocks/ingest/ingest_state_tests.rs) cover the two types of retry.

- `a_need_is_not_asked_again_before_its_interval`, `announcements_do_not_ask_for_a_need_faster_than_the_base_interval`, and `retries_slow_to_the_slowest_interval_and_never_end` cover network pacing while a need remains.
- `a_block_received_twice_is_held_and_dispatched_once` covers duplicate receipt. `a_failed_validation_waits_before_its_retry` and `a_block_that_keeps_failing_is_dropped_and_quarantined` cover validation.
- `a_quarantined_local_copy_moves_its_need_to_the_network_after_the_quarantine` covers a copy during quarantine. [`an_insert_clears_the_failure_count`](../../casper/src/rust/blocks/ingest/dependency_tracker.rs) checks cleanup after insertion.

#### C7. Integrity before hash-keyed state

The ban on all hash-keyed reads fails. Receipt verification uses a temporary hash slot and reads known-block state before checking integrity. A refused network body does not enter intake.

The deletion rule for a refused body that this node stored needs a separate review before C7 is restated.

Tests cover the known-block fast path and concurrent receipts. [Receipt verification and tests](../../casper/src/rust/blocks/ingest/receipt_verifier.rs).

#### C8. Pending admission and eviction

**Pending.** The [pending-store tests](../../block-storage/src/rust/pending_blocks.rs) verify capacity by admission class. The [owner tests](../../casper/src/rust/blocks/ingest/ingest_state_tests.rs) verify selection and reservations.

- `each_class_is_refused_at_its_own_share_of_the_area` and `the_byte_marks_reserve_room_for_known_senders_and_requested_blocks` cover admission limits.
- `proactive_passes_evict_only_unsolicited_blocks`, `a_refused_requested_block_evicts_unsolicited_before_requested_blocks`, and `a_refused_block_takes_no_room_from_a_lower_requested_block` cover the eviction passes.
- `a_requested_parent_at_its_sender_quota_makes_room_among_its_own_requested_children` covers the sender limit. `the_room_made_for_a_refused_block_is_reserved_before_it_is_freed` covers reservations.

## Historical seam premises

- **Dependency closure.** A block is processed only after all its dependencies are validated. C4 depends on this premise: validation of a block reads only the block and its validated dependencies.
- **Invalid set.** A dependency in the invalid-block set counts as validated for release. The rules that put a hash into that set are outside this claim.
- **Bounded buffer.** The buffer keeps its existing size limits and its stale prune as a safety net. C1 and C2 make the stale prune unnecessary for correct release, but this claim does not remove it.

## Historical discharge plan

This plan covers the implementation before block ingest.

The status of each item is recorded here as the TDD cycles complete.

1. `/cbc identify` classifies the four artifacts. Each one is `cbc=mandatory`, or a maintainer decision records why it stays untagged. **Done 2026-10-07.** The scanner proposed none of the four, so the maintainer tagged all four in `.gitattributes`. `block_processor.rs`, `buffer_resolver.rs`, and `casper_buffer_key_value_storage.rs` are `cbc-weight=high`. `block_processor_instance.rs` is `cbc-weight=medium`. The multi-agent review of PR #653 found five more changed production files without a tag. On 2026-10-07 the maintainer tagged them too: `buffer_dag_transition.rs` and `block_admission.rs` at `cbc-weight=high`, and `release_queue.rs`, `casper.rs`, and `block_retriever.rs` at `cbc-weight=medium`.
2. C2: a test reproduces the stale-link hold on the current code and passes after the fix (plan B5). A test forces the race between a dependency check and the dependency's validation through the public interfaces and shows that no stale link remains (plan B6). **Done 2026-10-30.**
   - B5: `a_released_block_with_a_stale_parent_link_is_processed` (`casper/tests/sync/stale_buffer_link_spec.rs`). It fails when the buffer wait counts any parent relation, also one to a validated parent.
   - B6: `a_child_committed_after_its_parent_enters_the_dag_keeps_no_stale_relation` (`block-storage/tests/atomic_buffer_dag_transition.rs`). The parent enters the DAG between the dependency check of the child and its commit. The test fails when the commit keeps relations to parents in the DAG.
3. C1: a test shows that the validation of the last missing dependency releases its waiting children. A second test shows that a chain of missing ancestors releases level by level without a timer (plans B5 and B8). **Done 2026-10-30.**
   - `after_a_block_is_processed_only_the_blocks_it_released_are_examined`: validating A releases B in the next scan.
   - `a_chain_of_missing_ancestors_releases_one_level_per_validation` (new): a chain A, B, C releases B after A validates and C after B validates. No timer runs between the steps. The test fails when the readiness check always passes and when it never passes.
4. C3: the two-lane release queue has tests for release-first order and for the bounded gossip share (plan B7). A pipeline test asserts processing order through the block processing result channel. **Done 2026-10-30.**
   - `a_released_block_is_processed_before_later_gossip_and_gossip_still_progresses` checks release-first order and the bounded gossip share at the queue.
   - `a_block_released_while_gossip_waits_is_processed_before_that_gossip` drives `run_scheduler`, the scheduler of the processing pipeline, and asserts the processing order.
   - Both tests fail when the queue always serves gossip first.
5. C4: a bounded model or a property test processes one set of ready blocks in every order or in generated orders. It compares the resulting DAG and verdicts with the FIFO order. **Done 2026-10-30 for independent ready blocks.** Order invariance for released blocks is open.
   - `ready_blocks_reach_the_same_dag_and_verdicts_in_every_processing_order` (new, `casper/tests/sync/release_order_invariance_spec.rs`): a fresh node processes three ready blocks from three validators in all six orders. The verdicts, the DAG membership, and the latest messages equal the FIFO order.
   - Scope limit: the ready set holds valid, independent blocks only, not equivocations or invalid blocks.
   - Scope limit: the test gives each block directly to `process_block`. Each block has only genesis as parent, so no block parks and the release queue does not run.
   - A test that sends parked blocks through the buffer and the scheduler would close the open part.
6. Record the evidence in `docs/cbc-evidence/` for each `cbc=mandatory` artifact and cite this claim id. **Done 2026-10-30.**
   - `scripts/cbc.sh claim-set` declared this claim on the nine `cbc=mandatory` artifacts (`docs/cbc-evidence/*.claims.json`). No artifact is discharged or waived yet.
   - The review package for items 2 to 6 is in `docs/cbc-evidence/runs/casper-buffer-release-discharge-20261030-02/`. It repeats the checks of package `-01` on the merge of `dev` at `cf94b4200`, which changed `block_processor.rs` and `block_processor_instance.rs`.
7. Run a comparison soak of the branch against a `dev` soak on the same base. The soak reports park time, release queue wait, sustained finalization p95, and the `test_load` failure count. The maintainer accepts the evidence before PR #653 leaves draft. **Open.**
   - PR #653 merged on 2026-10-07 without this evidence. A comparison on one base is still possible: a `dev` soak against a `dev` soak with PR #653 reverted.
   - Proposed substitute evidence (2026-10-30): the soaks below compare `dev` and `master` after PR #653 with the issue #24 baseline before PR #653. The bases are different, so the soaks do not isolate the effect of PR #653.
   - The substitute replaces the requirement only after a maintainer approves it.
   - Each sustained finalization sample below is the finalization p95 of one validator in one sustained phase.
   - Baseline before PR #653: issue #24 measured 56% of received blocks parked, for 7.1 s median and 39.6 s p90. Run 37469364217 on `dev` `778cc6754` had 8 of 49 `test_load` failures.
   - After PR #653: the scheduled soak run 40806861759 tested `dev` `252a86d1d` for 72,469 s with 146 iterations and 0 failures. The aggregates use the node metrics of the 73 Docker iterations.
   - Park time: 230 of 148,605 replayed blocks parked (0.15%). The median is at most 0.1 s, the p90 is from 1 s to 2.5 s, and the maximum is at most 5 s. The mean is 0.28 s.
   - Release queue wait: 1,554 released blocks. Each one waited less than 5 ms. The mean is 44 µs.
   - Sustained finalization p95: the median across the 572 sustained-phase samples is 23.5 s, the 95th percentile is 29 s, and the maximum is 45.6 s. No block stayed unfinalized.
   - `test_load`: each of the 146 iterations ran `test_deploy_throughput_and_finalization`. The 143 completed sessions had 0 failures, 73 on Docker and 70 on subprocess. The segment deadline stopped 3 subprocess sessions (exit 124), which do not count as failures.
   - Limit: issue #24 needs at least 100 sessions on each provider. This soak has 73 and 70, so it cannot close issue #24.
   - 60-hour soak: run 40545189128 tested `master` `de6756ed3`, which contains PR #653. It ran for 209,430 s with 356 iterations and 0 failures, and it finished on 2026-10-30. The aggregates use the node metrics of the 178 Docker iterations.
     - Park time: 486 of 360,498 replayed blocks parked (0.13%). The median is at most 0.1 s, the p90 is from 0.1 s to 0.25 s, and the maximum is at most 2.5 s. The mean is 0.09 s.
     - Release queue wait: 3,339 released blocks. Each one waited less than 5 ms. The mean is 46 µs.
     - Sustained finalization p95: the median across the 1,400 sustained-phase samples is 23.2 s, the 95th percentile is 29.5 s, and the maximum is 34.5 s. No block stayed unfinalized. The last-finalized-block spread is 0 in all 350 samples.
     - `test_load`: the 350 completed sessions had 0 failures, 175 on Docker and 175 on subprocess. The segment deadline stopped 6 sessions (exit 124), 3 on each provider. All 86 benchmark segments passed.
     - This soak has more than 100 sessions on each provider, so it meets the session count of issue #24. Its target is `master`, and issue #764 measures the gate on `dev` first.
8. C5: a unit test adds two dependency-free entries at different times and runs the overflow prune with a limit of one node. The older entry is removed and the newer entry stays. The test fails on the old order. **Done 2026-10-16:** `the_overflow_prune_removes_the_oldest_dependency_free_entry_first` failed on the old order and passed after the fix. The prune now sorts oldest first and tries each hash once.
   - After the PR #752 review, the prune builds the list again when it uses up the first list. It then also finds the entries that a removal freed. `the_overflow_prune_also_removes_an_entry_that_a_removal_freed` failed on the single-list prune and passed after the change.
9. C6: a `TestNode` spec buffers a block, sends a recovery request for it, and disposes a duplicate copy of it. A second recovery request inside the cooldown sends nothing, and the retriever keeps tracking the block. The spec fails on the old disposal. **Done 2026-10-16:** `a_duplicate_copy_of_a_buffered_block_keeps_its_refetch_cooldown` (`casper/tests/sync/refetch_cooldown_spec.rs`) failed on the old disposal: the second request sent one more message.
   - After the fix, `dispose_not_of_interest` does not acknowledge a duplicate copy of a block that waits in the buffer. The cleanup that cleared the cooldown also cleared the retry counter and the quarantine, so the fix keeps all three.
   - `cargo test --release --no-fail-fast -p casper -p node -p block-storage` passed 2,385 tests and ignored 14.
10. C7: tests show that a refused message changes no state keyed by the hash that it claims, and that a refused stored copy is removed. **Done 2026-10-29** on the fix branch of advisory GHSA-6crf-h3q7-4jm7.
    - `forged_copy_does_not_take_the_in_flight_slot_of_the_honest_block`, `a_body_signed_under_a_foreign_hash_does_not_touch_that_hash`, `a_badly_signed_message_does_not_touch_the_hash_that_it_claims`, `a_refused_body_that_this_node_stored_is_purged`, `discard_copy_removes_only_the_matching_body`, `multi_parent_casper_should_refuse_a_forged_body_under_an_honest_hash`, `engine_should_score_out_a_peer_that_pushes_forged_blocks`, and `a_peer_strike_counts_without_touching_a_tracked_source`.
    - Mutation controls ran on a copy of the tree. Each of these changes makes at least one of the tests fail: the hash check is disabled, the signature check is disabled, the stored-copy purge is removed, every refused message is treated as a stored copy, the on-disk delete is skipped, and the peer strike is removed.
11. C8: tests show that an unknown-sender flood cannot displace a known sender's pending block. **Done 2026-10-29** on the same branch.
    - `a_flood_from_unknown_senders_never_evicts_a_known_sender_block`, `a_known_sender_block_evicts_the_oldest_unknown_sender_block`, `an_unknown_sender_cannot_displace_known_sender_blocks`, `a_reinsert_promotes_an_entry_to_a_higher_tier_only`, and `multi_parent_casper_should_keep_a_known_sender_pending_block_under_an_unknown_sender_flood`.
    - The review package is in `docs/cbc-evidence/runs/casper-buffer-integrity-20261029-01/`. The claim stays `pending` until the maintainer accepts the evidence.
    - `running.rs`, `peer_scoring.rs`, `key_value_block_store.rs`, and `pending_blocks.rs` are new artifacts of this claim. As in item 1, the maintainer decides whether each one is `cbc=mandatory`.

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

A received block parks in the casper buffer while one of its dependencies (a parent or a justification) is not validated. Issue #24 measured that under sustained load 56% of received blocks park, for 7.1 s median and 39.6 s p90, while replay takes 0.5 to 1.1 s. Parked blocks stretch the finality rounds, and the finalization tail fails the `test_load` 45 s gate.

A code trace on 2026-10-07 found three release-path causes. A released block re-enters the tail of the one shared processing queue, behind gossip. The release scan reads every buffered block after every processed block. A block with a stale parent link can be dropped as already processed and stay parked until the 180 s stale prune.

TASK-021-12 changes the release path. This claim states the properties that the change must preserve or establish. The claim is pending: no code change for TASK-021-12 starts before this file exists (EPIC-021 `cbc_policy`).

TASK-021-18 (stall-loop item A7) extends the claim with C5 and C6. A code reading of `dev` on 2026-10-16 found two defects that do not depend on load:

- The overflow prune of the buffer sorts dependency-free entries youngest first and removes the first one. It removes the newest block, which is often the block that just became ready.
- A recovery re-request for a buffered block gets a duplicate copy as its reply. The copy has the verdict `AlreadyProcessed`, and its disposal calls `ack_processed`. That call clears the re-fetch cooldown, the retry counter, and the quarantine of the block. The retry budget then never runs out.

## Definitions

- **Validated:** a block hash is in the block DAG, in the equivocation tracker, or in the invalid-block set.
- **Ready:** every dependency of a buffered block is validated.
- **Released:** a ready block is given to the processing pipeline.
- **Stale link:** a buffer relation from a block to a dependency that is already validated.

## Claim statements

**C1. Release liveness.** A buffered block that becomes ready is released within one processing step of the event that made it ready. That event is the validation of its last missing dependency. The release does not wait for a timer, a stale prune, or a restart.

**C2. No stale hold.** A released block is processed. The pipeline does not drop it as already processed because of a stale link. A block never keeps a stale link to a dependency after that dependency is validated. This rule also holds when the dependency check of the block races with the validation of the dependency.

**C3. Release order without starvation.** A released block is processed before every gossip block that entered the queue after the release. Gossip blocks still progress: within a bounded number of processing steps, the pipeline takes a gossip block when one is waiting.

**C4. Verdict invariance.** The validation verdict of a block, and the resulting DAG, do not depend on the order in which ready blocks are processed. C3 changes only the order and the latency of processing, never a validation result.

**C5. Eviction order.** When the buffer is over its node limit, the overflow prune removes dependency-free entries oldest first. Age is the time since the buffer first saw the hash. One prune pass tries at most `max_prune_batch` hashes and tries each hash at most once. An entry that a removal frees in the same pass is a candidate too.

**C6. Re-fetch rate limit.** A block can wait in the buffer. A duplicate copy of that block does not reset its re-fetch cooldown, its retry counter, or its quarantine. A duplicate copy of a block that is already in the DAG clears all tracking of the block, as before.

**C7. Integrity before hash-keyed state.** No step reads or changes state keyed by a block hash until the fields, the signature, and the hash of the block are checked against its contents. That state is the in-flight set, the buffer, the pending area, the block store, and the request tracking of the retriever. A message that fails the check from the network changes none of that state for the hash that it claims. A refused body that this node stored is purged and deleted, but only when the stored bytes equal the refused body.

**C8. Fair pending eviction.** The pending area keeps three tiers: unsolicited blocks from an unknown sender, unsolicited blocks from a known sender, and solicited blocks. A known sender has a latest message in the DAG of this node. An insert evicts only entries in its own tier or in a lower tier, oldest entry of the lowest tier first. A flood of blocks from new sender keys therefore never evicts a pending block from a known sender.

## Seam premises (documented, not proven)

- **Dependency closure.** A block is processed only after all its dependencies are validated. C4 depends on this premise: validation of a block reads only the block and its validated dependencies.
- **Equivocation and invalid sets.** A dependency in the equivocation tracker or the invalid-block set counts as validated for release. The rules that put a hash into those sets are outside this claim.
- **Bounded buffer.** The buffer keeps its existing size limits and its stale prune as a safety net. C1 and C2 make the stale prune unnecessary for correct release, but this claim does not remove it.

## Discharge plan

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
5. C4: a bounded model or a property test processes one set of ready blocks in every order or in generated orders. It compares the resulting DAG and verdicts with the FIFO order. **Done 2026-10-30.**
   - `ready_blocks_reach_the_same_dag_and_verdicts_in_every_processing_order` (new, `casper/tests/sync/release_order_invariance_spec.rs`): a fresh node processes three ready blocks from three validators in all six orders. The verdicts, the DAG membership, and the latest messages equal the FIFO order.
   - Scope limit: the ready set holds valid, independent blocks only, not equivocations or invalid blocks.
6. Record the evidence in `docs/casper/cbc-evidence/` for each `cbc=mandatory` artifact and cite this claim id. **Done 2026-10-30.**
   - `scripts/cbc.sh claim-set` declared this claim on the nine `cbc=mandatory` artifacts (`docs/cbc-evidence/*.claims.json`). No artifact is discharged or waived yet.
   - The review package for items 2 to 6 is in `docs/cbc-evidence/runs/casper-buffer-release-discharge-20261030-01/`.
7. Run a comparison soak of the branch against a `dev` soak on the same base. The soak reports park time, release queue wait, sustained finalization p95, and the `test_load` failure count. The maintainer accepts the evidence before PR #653 leaves draft. **Open.**
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

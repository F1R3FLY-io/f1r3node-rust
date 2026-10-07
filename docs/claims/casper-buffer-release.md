# Claim: casper buffer release liveness, no stale hold, and release order

```yaml
claim_id: CLAIM-CASPER-BUFFER-001
artifacts:
  - casper/src/rust/blocks/block_processor.rs                          # dependency check, buffer commit, check_if_of_interest
  - casper/src/rust/engine/multi_parent_casper/buffer_resolver.rs      # release scan (get_dependency_free_from_buffer)
  - node/src/rust/instances/block_processor_instance.rs                # block queue and release re-enqueue
  - block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage.rs  # buffer relations and pendants
status: pending
adapter: agentic
mechanization: none   # planned in the discharge plan below
task: TASK-021-12
tdd_plan: docs/tdd-plans/issue-24-parked-block-release-2026-10-07.md
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/24   # sustained finalization lag
  - GitHub Actions run 37469364217                          # dev 778cc6754: 8 of 49 test_load failures
```

## Context

A received block parks in the casper buffer while one of its dependencies (a parent or a justification) is not validated. Issue #24 measured that under sustained load 56% of received blocks park, for 7.1 s median and 39.6 s p90, while replay takes 0.5 to 1.1 s. Parked blocks stretch the finality rounds, and the finalization tail fails the `test_load` 45 s gate.

A code trace on 2026-10-07 found three release-path causes. A released block re-enters the tail of the one shared processing queue, behind gossip. The release scan reads every buffered block after every processed block. A block with a stale parent link can be dropped as already processed and stay parked until the 180 s stale prune.

TASK-021-12 changes the release path. This claim states the properties that the change must preserve or establish. The claim is pending: no code change for TASK-021-12 starts before this file exists (EPIC-021 `cbc_policy`).

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

## Seam premises (documented, not proven)

- **Dependency closure.** A block is processed only after all its dependencies are validated. C4 depends on this premise: validation of a block reads only the block and its validated dependencies.
- **Equivocation and invalid sets.** A dependency in the equivocation tracker or the invalid-block set counts as validated for release. The rules that put a hash into those sets are outside this claim.
- **Bounded buffer.** The buffer keeps its existing size limits and its stale prune as a safety net. C1 and C2 make the stale prune unnecessary for correct release, but this claim does not remove it.

## Discharge plan

The status of each item is recorded here as the TDD cycles complete.

1. `/cbc identify` classifies the four artifacts. Each one is `cbc=mandatory`, or a maintainer decision records why it stays untagged. **Open.**
2. C2: a test reproduces the stale-link hold on the current code and passes after the fix (plan B5). A test forces the race between a dependency check and the dependency's validation through the public interfaces and shows that no stale link remains (plan B6). **Open.**
3. C1: a test shows that the validation of the last missing dependency releases its waiting children. A second test shows that a chain of missing ancestors releases level by level without a timer (plans B5 and B8). **Open.**
4. C3: the two-lane release queue has tests for release-first order and for the bounded gossip share (plan B7). A pipeline test asserts processing order through the block processing result channel. **Open.**
5. C4: a bounded model or a property test processes one set of ready blocks in every order or in generated orders. It compares the resulting DAG and verdicts with the FIFO order. **Open.**
6. Record the evidence in `docs/casper/cbc-evidence/` for each `cbc=mandatory` artifact and cite this claim id. **Open.**
7. Run a comparison soak of the branch against a `dev` soak on the same base. The soak reports park time, release queue wait, sustained finalization p95, and the `test_load` failure count. The maintainer accepts the evidence before PR #653 leaves draft. **Open.**

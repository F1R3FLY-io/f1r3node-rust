# v6 order-then-execute, tranche 1

This tranche adds the v6 consensus core to the rev-stacks phase 1 branch. It implements the design in "Breaking the Validator Stall Loop", revision 7 (`cost-accounting/stall-loop-design-r7.pdf` in the publications repository). It applies to shards that set `CasperShardConf::v6_order_then_execute`.

This document describes the state of `patch/P1-02`. That branch holds P1-01, the P1-01 hook points, P1-03, P1-04, P1-04b and P1-13 on M0 (`rev-stacks/m0-track-a`). The record and gate contracts, the lag gate, the inclusion checks and pruning (P1-05 to P1-12) are on lane A2's branches.

## What changes on a v6 shard

- A proposer puts unexecuted envelopes into a block. It does not merge parent states and it does not execute deploys. An envelope is a plain signed `DeployData`.
- Validation is structural. It runs no Rholang and replays nothing. The checkpoint replay and the bonds-cache check do not run.
- A v6 block records its main parent as its merge base. The finality code reads the state lineage from the merge base, so the lineage is the main chain.
- When the LFB advances, the linearizer emits the newly finalized blocks in the canonical order: main-chain epochs, each in Kahn order, with ties broken by block number and then by hash.
- Every node executes the emitted blocks in that order, ahead of the canonical frontier (P1-04):
  - Each envelope runs in its own runtime session from the block pre-state, on dev's meter.
  - The runs are merged in block order. A later envelope that conflicts with a kept one is not merged.
  - The run produces a block record (P1-04b). For each envelope, the record holds the outcome, the COMM phlo, the compute phlo and the user log. It also holds the pre-root, the mid-root, the post-root and the close-block system deploy.
- A node cites the records of its own blocks in its next block. It also cites the record of any block that stays uncited for eight emitted blocks.
- The canonical record of a block is the first record, in canonical order of the finalized blocks that cite it, whose pre-root is the canonical root before the block and whose replay reaches its stated roots.
  - A node whose own run matches the canonical record adopts it without a replay.
  - Otherwise the node replays the canonical record and discards its speculative runs.
- Each block attests the newest canonical block and its root. Evidence comes from three things: a wrong attestation, an invalid record, and a record whose envelopes do not match its block.
- The executor keeps the records of the last `v6-retained-records` fixed blocks. `BlockAPI::offered_status` and `GET /api/deploy/offered/{deploy_id}/receipt` report a deploy's status, with a receipt from the record when it is retained (P1-13).

## What does not change

- Legacy shards. Every hook point is a branch on the presence of the v6 engine, and the engine exists only when the flag is set.
- Fork choice, justifications, finality and equivocation detection.

## Hook points

| File | Change |
|---|---|
| `casper/src/rust/blocks/proposer/proposer.rs` | `do_propose` calls `V6Engine::create_block` on a v6 shard. |
| `casper/src/rust/engine/multi_parent_casper/validation_dispatcher.rs` | A v6 block gets the structural check instead of the checkpoint replay and the bonds-cache check. |
| `casper/src/rust/engine/multi_parent_casper/finalization_runner.rs` | Each LFB advance wakes the executor. |
| `casper/src/rust/engine/multi_parent_casper/snapshot.rs` | If a v6 block names a root that this node does not hold yet, the active validators come from the block bonds. |
| `casper/src/rust/casper.rs`, `types.rs`, `dispatch.rs` | The engine handle and the `Casper::v6_engine` accessor. |

The flag has no configuration key yet. `casper_launch` sets it to `false` (D-P1-01h). The v6 data rides in `Body.extra_bytes` behind a magic prefix, so the protobuf schema does not change and the block hash covers the data.

## Known limits of this branch

- **Funding gate and settlement are not here.** Charging is off until phase 2, so test shards run at price 0. The gate (P2-04, P2-05) calls `V6Engine::record_rejection` for a refused envelope (Decision 34).
- **Slashing.** Evidence is recorded and logged. It does not yet become slash records.
- **Restart.** The executor state lives in memory on this branch. After a restart, a node derives it again from the DAG. The stored executor state is P1-07.
- **Lag gate, inclusion checks and pruning** are P1-08, P1-09 and P1-11, on lane A2's branches.

## Test harness

`casper/tests/helper/test_node.rs` builds v6 test networks:

| Constructor | Use |
|---|---|
| `TestNode::create_v6_network(genesis, n)` | Engines started. They run when the finalizer wakes them or when a test calls `wake()`. |
| `TestNode::create_v6_network_driven(genesis, n)` | Engines not started. The test calls `tick()` and decides when each engine runs. Lag tests use this mode. |
| `TestNode::create_v6_network_with_block_deploys(genesis, n, k)` | Sets `max_user_deploys_per_block` to `k`, which also sets the pool cap (P0-09). |
| `TestNode::create_v6_network_with(genesis, n, V6TestConfig)` | Any combination of the settings above. |

All of these constructors build the engine in one function, `v6_engine_for_test`. A base with a different `V6Engine::new` changes only that function. A restart test builds a second engine from the node's `block_store`, `block_dag_storage`, `deploy_storage` and `runtime_manager`.

`casper/tests/helper/v6_network.rs` holds the shared steps. These are `engine`, `propose`, `broadcast`, `propose_and_broadcast`, `wake_until`, `tick_until_idle`, `wait_for_finality` and `cited_records`.

An engine orders only the blocks at or below the LFB. A block becomes fixed only after a later block cites its record. A driven test therefore does three things in turn: it proposes, it waits for finality, and it ticks.

## Tests

- `casper/src/rust/v6/*`: unit tests for:
  - the body codec, the linearizer, the structural check and the envelope selection
  - the executor state machine, the block record, the receipts and the backend helpers.
- `casper/tests/multi_node/v6_order_then_execute.rs`, on three validators:
  - `validators_agree_on_racing_blocks_without_merging_or_replaying_on_the_consensus_path`: the first block carries two sends and two receives on one channel, signed by two keys, so the envelopes race. Every validator fixes the same root for each executed block. No honest validator produces evidence, and a later block cites the record of the racing block.
  - `driven_engines_advance_only_when_the_test_ticks_them`: a driven engine does not run on its own. A tick before any citation fixes nothing. Proposals and ticks in turn fix the first block on every validator with the same root.
  - `an_executed_offer_reports_its_receipt_on_every_validator`: the receipt from `BlockAPI::offered_status` equals the record entry on every validator: outcome, COMM phlo, compute phlo and the number of log events.
  - `offers_are_refused_once_the_pool_is_full`: with one deploy per block, the pool takes 64 offers and refuses the next one with `PoolFull`.
- `casper/tests/v6/*`: the executor on dev's meter, the block executor, and the offer-status lifecycle on one node.

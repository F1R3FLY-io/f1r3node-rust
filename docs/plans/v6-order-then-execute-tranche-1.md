# v6 order-then-execute, tranche 1

This tranche adds the v6 consensus core to `dev`. It implements the design in
"Breaking the Validator Stall Loop" (October 16, 2026), sections 5.2 to 5.10,
for protocol-6 shards: shards launched with
`casper.genesis-block-data.protocol-v6 = true`, whose genesis block has header
version 6. The mode is shard-wide and every node takes it from its approved
block.

## What changes on a v6 shard

- A proposer puts unexecuted envelopes into a block. It does not merge parent
  states and it does not execute deploys.
- Validation is structural. It runs no Rholang and replays nothing. The
  checkpoint replay and the bonds-cache check do not run.
- A v6 block records its main parent as its merge base. The finality code of
  `dev` reads the state lineage from the merge base, so the lineage is the main
  chain. The containment check of the finalizer then holds when assumption
  A-LIN holds, and it holds back otherwise, as the linearizer does.
- When the LFB advances, the linearizer emits the newly finalized blocks in the
  canonical order: main-chain epochs, each in Kahn order with ties broken by
  block number and then by hash.
- Every node executes the emitted blocks in that order, ahead of the canonical
  frontier. All envelopes of a block run concurrently in one RSpace session, so
  deploys race each other. The run produces a record: the pre-root, the
  user-phase root, the post-root, the interleaved COMM log, the cost and the
  close-block system deploy.
- A node cites the records of its own blocks in its next block. It also cites
  the record of any block that stays uncited for eight emitted blocks.
- The canonical record of a block is the first record, in canonical order of
  the finalized blocks that cite it, whose pre-root is the canonical root
  before the block and whose replay reaches its stated roots. A node whose own
  run matches the canonical record adopts it without a replay. Otherwise it
  replays the canonical record and discards its speculative runs.
- Each block attests the newest canonical block and its root. A wrong
  attestation, an invalid record or a record whose envelopes do not match its
  block produces evidence.

## What does not change

- Legacy shards. Every hook point is a branch on the presence of the v6 engine,
  and the engine exists only on a protocol-6 shard.
- Fork choice, justifications, finality and equivocation detection.

## Hook points

| File | Change |
|---|---|
| `casper/src/rust/blocks/proposer/proposer.rs` | `do_propose` calls `V6Engine::create_block` on a v6 shard. |
| `casper/src/rust/engine/multi_parent_casper/validation_dispatcher.rs` | A v6 block gets the structural check instead of the checkpoint replay and the bonds-cache check. The mergeable-channel cache is not updated. |
| `casper/src/rust/engine/multi_parent_casper/finalization_runner.rs` | Each LFB advance wakes the executor. |
| `casper/src/rust/engine/multi_parent_casper/snapshot.rs` | If a v6 block names a root that this node does not hold yet, the active validators come from the block bonds. |
| `casper/src/rust/casper.rs`, `types.rs`, `dispatch.rs` | The engine handle and the `Casper::v6_engine` accessor. |
| `casper.rs` (`CasperShardConf::is_v6`) | The shard mode: the casper version is 6 on a protocol-6 shard. |

The v6 data rides in `Body.extra_bytes` behind a magic prefix, so the protobuf
schema does not change and the data is covered by the block hash.

## Known limits of this tranche

- **Shared budget and identity.** The runtime on `dev` has one cost manager and
  one deploy-data cell per runtime. All envelopes of a block therefore share
  one budget, the sum of their phlo limits. If the budget runs out, the whole
  user phase of the block is reverted and the record is marked aborted. The
  `rho:deploy:data` system process does not return per-envelope data.
  Per-signature budgets come with the cost-accounting core (B1).
- **Funding gate and settlement are stubs.** The executor charges no fees and
  pays no refunds. The PoS pre-charge of `dev` does not run on v6 blocks.
- **Charges are those of `dev`.** The order-independent charge model (B2) is
  not in this tranche.
- **Slashing.** Evidence is recorded and logged. It is not yet turned into slash
  records, and v6 blocks carry no slash system deploys.
- **Restart.** The executor state lives in memory. After a restart a node
  re-derives it by replaying the canonical records from genesis.
- **Linearizer memory.** The set of emitted blocks is not pruned.
- **The finality fixes of Track A are separate work.** A v6 shard still needs
  them under sustained load.

## Tests

- `casper/src/rust/v6/*`: 26 unit tests for the body codec, the linearizer, the
  structural check, the envelope selection and the executor state machine.
- `casper/tests/multi_node/v6_order_then_execute.rs`: three validators. The
  first block carries two sends and two receives on one channel, signed by two
  different keys, so the envelopes race. The test checks that every validator
  fixes the same canonical root for each executed block, that no honest
  validator produces evidence, and that a later block cites the record of the
  racing block.

## The record and gate contracts

The seam reaches execution through two contracts, so that the executor (B3)
and the consensus seam (B4) can change independently.

- `v6::record::ExecutionRecord`. Outside the backend, the seam reads only the
  target, the pre-state root, the post-state root and the envelope ids of a
  record. It encodes, decodes and digests records, but it never reads their
  other fields. `ExecutionBackend` names its record type, and `ExecutorCore`,
  `V6Engine` and `check_structure` are generic over it. The tranche-1 record
  is `v6::concurrent_record::ConcurrentRecord`.
- The v6 body carries each record as opaque bytes. The structural check limits
  the number of records and their total bytes (`Limits::max_record_bytes`).
- A node adopts a cited record without replay only when the record digest
  equals the digest of its own run. Two runs that reach one root with
  different verdicts are not equal.
- Envelope ids are the B1 identity bytes of each envelope.
- `v6::gate::BlockGate` decides, at a state root, a verdict for each envelope:
  admitted with its purse holds, or rejected with a cause. The proposer keeps
  only the admitted envelopes. Tranche 1 uses `AdmitAll`. B3 supplies the
  funding gate that the executor applies at the pre-state.

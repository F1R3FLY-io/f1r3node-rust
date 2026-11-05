# FX-18: joining and rejoining from finalized state (P12), plan and size

Lane A2, 2026-11-05. Design r25, Decisions 115 to 117. Lane A1's A-10 audit (revisions 33 and 34). Base: `patch/FX-17` at dbc3dca9b (F52 closed, `30258ed83` and `c97c2e4b3` merged).

## Current state of the join

- A joiner with `trim_state` gets the responder's LFB as an approved block with `required_sigs: 0` (`running.rs`). It trusts one bootstrap node.
- `request_approved_state` downloads the blocks to the lifespan bound and the tuple space at the block's `post_state_hash`.
- `V6Engine::new` anchors the executor at the approved block with `post_state_hash` as the anchor root. For a v6 block that hash is its attestation root, the canonical root of an earlier block (P12-1).
- `TestNode` builds every node from genesis. There is no harness for a node that joins through LFS.

## Design

**Anchor (P12-1).** The joiner anchors at the LFB L with the canonical root at L's position, R(L). The joiner learns R(L) from the record that fixed L: a record whose target is L, cited by a block above L, with post-root R(L). The joiner checks that the records cited above L form a chain from R(L) to the attested roots (pre-root of each record equals the post-root of the one before). It downloads the tuple space at R(L), not at `post_state_hash`.

**Quorum (P12-2, Decision 117).**
1. The configuration holds a checkpoint: a finalized block C and its bond set B(C).
2. The responder sends the chain of validator-set handoffs from C to L (lane B's handoff record). Each handoff is certified by signatures of more than 2/3 of the previous set's stake. The joiner checks each signature and stake against the set before it.
3. The responder sends the attesting blocks for L: signed blocks whose attestation (j, root) has j at or above L's position and whose root is on the verified record chain. The joiner checks each block signature, and that the attesting stake is more than 2/3 of the bond set in force at L.
4. `required_sigs` becomes the size of that quorum, and `Validate::approved_block` checks it.

**Statuses.** The joiner gets the deployment statuses for the lifespan window from lane C's work on the downloaded records, without execution. FX-18 calls that interface when the executor store is created at L.

**Rejoin (P12-3).** At start, and when a block request fails because peers pruned it, the node compares its LFB with the peers' fringe. If its LFB is behind by more than the retention bound, or a needed block cannot be fetched, it discards its executor prefix and joins again through the anchor path.

**Resets (P12-5).** `LmdbRecordStore::open` with `ReplacedOtherAnchor` or `ReplacedOlderFormat` no longer restarts at genesis. It rejoins through the anchor path. A format change that keeps the store follows Decision 115.

**Decision 115.** For a store from before FX-17 without statuses (or with 9-byte statuses), the node replays each block of the lifespan window below its LFB from that block's own pre-state and writes the statuses. It never replays from genesis.

## Tests

1. A fresh node joins a running 3-validator v6 shard from a checkpoint, then executes and attests the same roots as the others. This needs a `TestNode` harness that runs the LFS requester against a running node.
2. A node absent longer than retention rejoins.
3. A forged quorum (signatures under 2/3, a wrong root, or a bad signature) is rejected.
4. A forged handoff (under 2/3 of the previous set, or a bad signature) is rejected.

## Size

| Work | Days | Depends on |
|---|---|---|
| Anchor at R(L): the record chain, the state download at R(L), the executor store at L | 2 | — |
| Checkpoint configuration, handoff chain check, LFB quorum check, `required_sigs` | 1.5 | lane B's handoff record format (needed by day 2) |
| Statuses at the anchor from the records | 0.5 | lane C's interface |
| Rejoin after an absence (detection and switch) | 1 | anchor path |
| Resets go to the anchor path; Decision 115 rebuild | 1.5 | — |
| Joiner harness in `TestNode` (LFS against a running node) | 1.5 | — |
| The four tests and the full suites | 1.5 | all |
| **Total** | **9.5** | |

**Risks:** the harness is new, and LFS has no multi-node test today. If lane B's handoff record is not fixed by day 2, the quorum work slips by the same amount. Lane A1's P12-8 (the conservation audit needs every wallet) stays a known issue. A joiner disables the audit or reads its index from the anchor state.

## Shared interface (for lanes B and C, before code)

The proof travels with the trimmed approved block, beside `floorSeed`, outside the signed candidate:

```proto
message ApprovedBlockProto {
  ApprovedBlockCandidateProto candidate = 1;
  repeated Signature sigs               = 2;
  FinalizedFloorSeedProto floorSeed     = 3;
  V6JoinProofProto v6Join               = 4;   // new, v6 only
}

message V6JoinProofProto {
  bytes anchorRoot                         = 1; // R(L), the canonical root at the LFB's position
  repeated BlockMessageProto citers        = 2; // blocks above L whose records chain from R(L) to the attested roots
  repeated BlockMessageProto attesters     = 3; // signed blocks whose attestations make the quorum
  repeated ValidatorSetHandoffProto handoffs = 4; // lane B: checkpoint to L, in order
}
```

- **Joiner check, in order:** (1) the handoff chain from the configured checkpoint, each certified by more than 2/3 of the previous set's stake; (2) every citer and attester signature; (3) the records in `citers` chain from R(L) (a record with target L and post-root R(L)) up to each attested root; (4) the attesters whose attested root lies on that chain hold more than 2/3 of the bond set in force at L. Only then does the joiner request the tuple space at R(L).
- **Lane B:** `ValidatorSetHandoffProto` (the new set, the epoch, and the signatures of the previous set). The joiner needs its fields and its signed payload.
- **Lane C:** the statuses for the lifespan window are derived from the records reachable from `citers` and from the blocks LFS downloads.
- `required_sigs` stays 0 for the candidate. The quorum lives in `v6Join`, so the genesis ceremony's signed payload does not change.

## Finding while merging: a timing test after Decision 114

`multi_node::v6_engine_state::a_long_run_keeps_executor_memory_flat_and_old_receipts_available` needs 30 fixed positions in a fixed time. Measured alone: 31 at 488122957, 31 at eae1a57c7 (F52), 29 at 36168ae0a (after lane B's `30258ed83`, Decision 114). The heavier epoch close in D114 slows execution enough to miss the bound. The memory-flatness checks pass. Lane B to decide: speed up the close, or lane A1 to accept a lower liveness bound in this test.

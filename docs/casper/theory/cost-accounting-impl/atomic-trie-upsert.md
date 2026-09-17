# Atomic trie update or insertion

## Purpose and status

The monetary allocator needs one persistent cursor per authorized payer cohort.
Concurrent first use must not create duplicate cursor state.
The existing map updater does not invoke its callback when the trie path is absent.
A separate lookup followed by an unconditional setter cannot provide atomic initialization.

The new `updateOrInsert` operation extends the existing contract in [Registry.rho](../../../../casper/src/main/resources/Registry.rho).
It does not change Casper voting, fork choice, finality, or pruning.
This operation is a prerequisite for cursor initialization, not a complete implementation of monetary settlement.

The bounded formal checks passed before the contract change.
The native baseline returned no result because the operation did not exist.
The implemented operation passes the fixed native cases and generated concurrent-update property.
The delayed-callback check also passes after inspecting a checkpoint with the leaf held and a checkpoint after explicit release.
The complete legacy suite has not yet passed in this validation batch.
It reached the 60-second deadline under one CPU, then encountered a worker-stack overflow under four CPUs with default stack settings.
A further run used four runtime workers and 32 MiB worker stacks under a 6 GiB memory limit with swap disabled.
That run also reached the 60-second deadline during test-source evaluation.
The timeout cause remains unresolved.
Strict Casper library and test Clippy checks passed afterward.
These results do not justify changing the test assertions or claiming that the complete suite passed.

## Contract interface

Call the operation with a map, key, callback name, and acknowledgement name:

```rholang
new update, ack in {
  contract update(@current, reply) = {
    if (current == Nil) { reply!(1) } else { reply!(current + 1) }
  } |
  TreeHashMap!("updateOrInsert", map, "counter", *update, *ack)
}
```

The enclosing process must bind `TreeHashMap` and `map` before this example runs.
The callback receives the current value and a private reply name.
The callback must send one replacement value to that reply name.

An absent key and a stored `Nil` value both produce `Nil` as the callback argument.
Returning `Nil` stores a present key with that value.
It does not delete the key.
Use `contains` when key presence matters independently of the stored value.
The proposed private cursor directory must store non-`Nil` markers.

## Implementation

The existing setter still creates trie paths.
The new operation preserves the existing parent acquisition and existence recheck.
It does not replace those steps with a lookup followed by an insertion.

The leaf writer receives the map from the consuming receive, not the earlier peek.
It keeps that leaf unavailable until the callback returns a replacement.
It replaces only the requested key and preserves the other entries in the collision bucket.
A collision bucket contains keys with the same selected hash prefix.

The algorithm follows these steps:

1. Read the next parent node.
2. If its child is absent, acquire the parent and check again.
3. Create the child only if it remains absent.
4. Continue toward the leaf while scheduled parent and child publications complete.
5. Acquire the leaf datum.
6. Invoke the callback with the current key value.
7. Receive the replacement value.
8. Publish the updated bucket and acknowledge the operation.

Replacement publication and acknowledgement are parallel sends, as in the existing setter and updater.
An acknowledgement does not establish that the replacement send has already completed.
A subsequent operation can wait for that replacement datum.

The public `set` operation retains its overwrite behavior without a callback.
Its overwrite branch remains inline and avoids an additional helper communication carrying the complete bucket.
The internal operation tuple changes message contents, so this document does not claim identical historical event logs or execution costs.
The existing `update` operation retains its missing-path behavior.

## Concurrency and authority

Only operations that share a leaf compete for that leaf datum.
Path creation can briefly acquire shared parent nodes.
The operation does not introduce a global map lock.
Trie depth controls how many hash-prefix collisions can share a terminal bucket.

The callback runs while the leaf is unavailable.
A callback that never replies can prevent further operations on that leaf.
This behavior matches the existing callback updater.
SystemVault must use a private directory and a trusted initialization callback.
Untrusted callers must not receive the directory capability or its callback reply names.

## Verification correspondence

The model is [AtomicTrieUpsert.tla](../../../../formal/tlaplus/cost_accounted_rho/AtomicTrieUpsert.tla).
It represents parent and leaf ownership, separate child publication, and parent publication that can overlap deeper traversal.
It also represents peek snapshots, consumed bucket snapshots, key presence, stored `Nil`, callback replies, and acknowledgement timing.

| Invariant | Implementation requirement | Native check |
| --- | --- | --- |
| `UniqueNodeCreation` | Recheck the child while the parent datum is consumed. | Concurrent first writes traverse missing paths. This test observes results, not internal creation counts. |
| `ExclusiveLeafOwnership` | Keep one available or owned datum per node. | Same-key and colliding-key updates preserve all increments. |
| `NoLostUpdates` | Construct the replacement from the consumed bucket. | Each key equals its independently counted submitted operations. |
| `NoLostPresence` | Preserve bucket membership independently of values. | A callback-returned `Nil` remains present. |
| `CompletedKeysRemainPresent` | Insert rather than delete a callback-returned value. | Subsequent lookup and update work after insertion. |
| `CallbackHoldsLeaf` | Keep the leaf consumed until the reply arrives. | The delayed-callback test inspects state before and after an explicit release. |
| `AcknowledgementRequiresReply` | Schedule acknowledgement only after the callback reply. | Joined acknowledgements precede final lookup requests. |

The two safe configurations use two workers and three keys, including two keys with the same leaf path.
They enumerate initial key presence and initial values, including stored `Nil`.
One configuration increments values, and the other returns `Nil`.
Four negative controls expose duplicate creation, an unlocked callback, a stale value bucket, and lost `Nil` membership.
All six checks passed.

The [native tests](../../../../casper/tests/util/rholang/atomic_trie_upsert.rs) use the actual contract and RSpace runtime.
Fixed cases cover trie depths zero, one, and three.
Depth zero forces all keys into the same collision bucket.
The generated property checks eight cases with one through eight concurrent updates across four keys and depths zero through three.
Its numeric oracle counts submitted operations independently of the contract.

These checks do not prove arbitrary callback termination, bitmap arithmetic, or parallel-branch merge safety.
The model does not include overwrite mode, monetary balances, cursor revisions, or checkpoint rollback.
The native tests do not enumerate every scheduler interleaving.
No new Rust synchronization primitive was added, so a test-only Loom lock would not verify this contract operation.

## Reproduction and remaining integration

Run the filtered checks through the repository's memory-bounded verification wrapper:

```text
bash scripts/check-cost-accounted-rho-tla-invariants.sh --filter AtomicTrieUpsert
cargo test -p casper --test mod atomic_trie_upsert -- --test-threads=1
cargo test -p casper --test mod tree_hash_map_spec -- --test-threads=1
```

Run heavy commands within `systemd-run` with a hard memory limit and swap disabled.
Store temporary files on disk, not under the tmpfs-mounted `/tmp` directory.
Current result logs use `target/verification/claims-audit-20260909/atomic-trie-*.log`.

Native cursor integration must still bind the cursor to the certificate and update it atomically with settlement.
It must test stale transitions, rollback, same-cohort branch initialization, distinct cohorts, and replay.
The [monetary allocation design](rotating-monetary-allocation.md) records that remaining work.

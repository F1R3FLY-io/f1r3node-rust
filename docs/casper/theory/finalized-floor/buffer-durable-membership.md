# Durable buffer membership

## Status and scope

This repair belongs to pgmcp task `pr216-admission-backpressure`.
The formal specification preceded the storage correction.
Focused production qualification passed.
That qualification did not establish safe age or pressure eviction.
The [pruning audit](buffer-pruning-preservation.md) reproduces unresolved-edge and retry-owner loss through the current eviction path.
The larger candidate-index and shared-pump repair remains in progress.
The [admission plan](admission-and-recovery-backpressure.md) describes the larger task.

The buffer stores unresolved block and certificate dependencies.
These records support rediscovery after restart.
They do not prove that a block is valid or that its dependencies have admitted metadata.
The repair does not change voting, certificate thresholds, fork choice, block serialization, or cost settlement.

## Failure evidence

The inspection found four loss cases in `casper_buffer_key_value_storage.rs`.

| Operation | Old behavior | Required behavior |
| --- | --- | --- |
| Restore an empty parent row | The constructor ignores the row. | Restore the explicitly buffered pendant. |
| Insert a pendant | A temporary parent is inserted and removed. No durable pendant row remains. | Persist an empty row without a synthetic dependency. |
| Resolve the final dependency | The newly ready child's row is deleted. | Keep the child with an empty parent set. |
| Remove a child | An orphaned parent's row can be deleted despite its own unresolved dependencies. | Preserve every unrelated explicit row and dependency. |

The same defects exist in pinned local `dev`, commit `62fa58f1183630d08919e4957d29018ecd1b3bcb`.
This reference does not establish the contents of a later remote revision.

Removal also publishes memory changes before fallible storage writes.
Separate puts and deletes allow a crash to leave an incomplete durable update.
An index-only correction cannot repair these persistence defects.

## Durable representation

The existing parent-set encoding remains unchanged.
A row identifies an explicitly buffered block.
An empty parent set identifies an explicitly buffered block without unresolved dependencies.
A parent that appears only inside another row identifies a reference-only missing root.

Explicit identity and referenced identity are different concepts.
Removing a root's final child must not erase an independently buffered pendant at that root.
The implementation must retain explicit identity separately from graph edges.

The existing `contains` and `get_parents` interfaces describe nonempty parent mappings.
Preserving empty durable rows must not silently change those interfaces.
Certificate dependencies retain their disjoint 33-byte namespace.
Ordinary 32-byte block hashes remain ordinary hashes, including hashes that start with `0xff`.

## Mutation contract

The existing buffer write guard serializes mutations within one buffer instance and its clones.
This guard does not serialize validator consensus or independent block execution.
Code that also needs the DAG write guard must acquire the DAG guard first.

Each mutation follows this sequence.

1. Acquire the existing buffer write guard.
2. Prepare changes for the affected rows and graph links.
3. Encode all affected keys and values before storage changes.
4. Commit all row changes through one strict backend transaction.
5. Publish the corresponding memory, identity, and timestamp changes.
6. Release the guard.

A failed transaction leaves durable rows and published memory unchanged.
A restart after commit reconstructs the complete committed state, even if memory publication did not occur.
The supported backend must provide strict transactions.
An unsupported backend must return an error instead of attempting separate writes.

Removal deletes only the selected explicit row.
It removes the selected dependency from every surviving direct child.
It retains those child rows even when their parent sets become empty.
Unrelated parents retain their own explicit identity and incoming dependencies.
A reference-only root can disappear when its final dependent disappears.

Repeated removal is idempotent.
Ensuring a pendant preserves an existing nonempty parent set.
Mutation preparation must depend on the affected neighborhood, not a clone of the entire DAG.
The existing startup reconstruction can enumerate durable metadata once.

## Formal correspondence

[`BufferDurableMembership.tla`](../../../../formal/tlaplus/block_admission/BufferDurableMembership.tla) models two concurrent clients and the existing exclusive mutation guard.
Prepare, commit, memory publication, abort, and crash-restart are separate transitions.
Clients can add dependencies, ensure explicit rows, and remove identities.
The model permits cyclic relations as well as acyclic relations.
Certificate keys remain distinct from block keys.

| Invariant | Meaning | Native obligation |
| --- | --- | --- |
| `Inv_DurableMembership` | Durable rows equal the complete committed mutation. | Compare all rows against an independent reference after success or injected failure. |
| `Inv_PublishedProjection` | Unlocked memory equals durable rows. | Check forward links, backward links, and dependency-free identities. |
| `Inv_RestartCandidates` | Restart preserves candidate membership. | Reopen after pendant insertion, dependency resolution, certificate resolution, and removal. |
| `Inv_CertificateSeparation` | Certificate keys are not block candidates. | Check both key namespaces and certificate-waiting children. |

Five unsafe configurations independently remove these controls.
Each configuration must violate its named invariant.
A parser error, timeout, or resource failure does not qualify as an expected counterexample.

[`BufferDurableMembership.v`](../../../../formal/rocq/finalized_floor/theories/BufferDurableMembership.v) proves row semantics without a fixed identity-count bound.
Its 15 theorems cover dependency preservation, ready-child retention, idempotence, commuting removals, transaction outcomes, and arbitrary finite operation histories.
The proofs use pointwise equality and introduce no custom axioms.
The abstract transaction theorem assumes the backend's all-or-nothing transaction contract.
Native fault injection and the backend's separate transaction tests must establish implementation correspondence.

The initial TLC run checked 4,008 distinct states with two block keys, one certificate key, and two clients.
All five unsafe controls failed the required durability or publication invariant.
Independent Rocq kernel checking passed all 15 closed theorems.
The [task log](../../../work-logs/task-pr216-admission-backpressure-2026-09-06.md) records evidence paths and resource limits.

Twenty native buffer tests, twelve atomic-transition tests, and two production-publication Loom tests passed.
Strict Clippy passed for the storage tests and the Loom target.
Generated histories compare every row and graph link against an independent reference after each operation.
Real LMDB tests terminate child processes before commit and after commit, before memory publication.
Reopening must recover the complete old or new state.

Loom imports the production commit-before-publication helper under the existing mutation guard.
It checks competing writers, a reader, and transaction failures.
The negative control proves why that guard must cover both durable commit and memory publication.
This check does not replace native graph tests or backend transaction verification.

Run the focused gate with an explicit memory limit.

```sh
systemd-run --user --scope -p MemoryMax=5G -p MemorySwapMax=0 \
  bash scripts/check-buffer-durable-membership.sh all
```

The gate records input hashes and rejects missing tests or unexpected negative-control failures.
Its `formal` mode runs Rocq and both finite TLC domains.
Its `native` mode runs buffer regressions, atomic-transition tests, production-linked Loom, and strict Clippy.

## Restart age and pressure count

Restored identities previously had no first-seen timestamp.
The age lookup substituted the current scan time, so each later scan again calculated zero age.
Isolated pendants also contributed zero to the former adjacency-map count.
These two controls could leave restored entries outside both positive-TTL and pressure pruning.

The constructor now assigns one startup timestamp to every reconstructed live identity.
Each later scan measures age from that stable restart-local epoch.
The pressure count includes the disjoint nonempty-parent and dependency-free partitions.
The existing `size`, `contains`, and `get_parents` meanings remain unchanged.

The new count measures live identities instead of the sum of two adjacency-map lengths.
An identity with both incoming and outgoing links no longer counts twice.
An isolated pendant now counts once.
The same configured threshold can therefore trigger pressure pruning at a different point.
This is a local buffer-pressure correction, not a consensus validity change.

`BufferRetentionEpoch.tla` checks stable age, positive-TTL eligibility, and complete counting.
Its safe domain passed 2,300 states.
Both unsafe controls violated their named invariant.
`BufferRetentionEpoch.v` adds six kernel-checked theorems for arbitrary timestamps and finite identity partitions.
The native age and pressure regressions both failed before this correction.

These age claims require a nondecreasing clock within the inspected interval.
Clock rollback delays expiry because the implementation uses saturating subtraction.
Restart begins a new age epoch and does not preserve time already spent waiting.
Repeated restarts can therefore postpone TTL expiry.
Pressure eligibility does not depend on the age epoch.

## Limits and remaining work

This specification does not prove a fixed memory bound for all live buffer metadata.
The [candidate-index specification](buffer-candidate-rotation.md) defines examination fairness separately.
Scanner payload bounds still require shared-pump integration and verification.

Pruning explicitly retires its selected buffered identity.
Pruning is not evidence that the selected block has admitted metadata.
For a chain of dependencies, the scanner must still check admitted metadata before admitting a surviving descendant.
The larger preservation audit must verify how the node rediscovers a pruned missing dependency.

Old stores can already lack rows for pendants lost by the previous implementation.
Reconstruction cannot recover evidence that the store no longer contains.
The repair must not infer pending membership by scanning unrelated block bodies.

The proof covers one buffer instance and its clones over a strict backend.
It does not authorize independent mutable buffer instances over the same store without a shared mutation guard.

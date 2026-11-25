# Execution Performance Proposals

Status: draft designs for review. Implementation and operational testing need separate approval.
Date: 2026-11-25.

## Scope and Evidence

These proposals address repeated allocation, copying, compilation, sorting, and cache initialization.
They do not change consensus rules, deploy scheduling, timeouts, or retained chain history.
They do not propose larger concurrency limits or caches.

Source inspection identifies unnecessary operations and candidate costs.
It does not establish their share of node CPU time or finalized throughput.
The [cold-leaf measurements](../erd/storage/leaf-measurements.md) describe encoded storage, not execution frequency or live heap.
No live shard was changed or profiled for these designs.

The first three proposals have explicit repeated operations in the inspected source.
The other three need workload measurements or a side-effect audit before implementation.
Even a large local reduction can have little effect when consensus or network waits dominate.

## Evaluation Order

| Order | Proposal | Source observation | First deliverable | Main decision |
| --- | --- | --- | --- | --- |
| 1 | [Storage buffer copies](performance-storage-buffer-copies.md) | Point reads and conditional writes clone owned payloads | Move existing buffers without changing transactions | Does allocator and CPU work decrease in actual store callers? |
| 2 | [Checkpoint assembly](performance-checkpoint-assembly.md) | Action concatenation deep-clones already owned values | Consume action vectors in the same order | Does checkpoint cost decrease with unchanged bytes and roots? |
| 3 | [Merge ordering](performance-merge-ordering.md) | Comparators repeatedly hash channels or sort branch members | Invocation-local prepared ordering keys | Are comparator results and rejection decisions identical? |
| 4 | [Compiled deploy terms](performance-compiled-term-cache.md) | Admission discards a term that execution compiles again | Measure reuse of exact validated deploys | Do saved compiler costs exceed key and AST-copy costs? |
| 5 | [History cache eviction](performance-history-cache-eviction.md) | Capacity enforcement clears whole cache maps | Count reloads attributable to clearing | Is there reusable history that the active hot store does not already retain? |
| 6 | [Replay cache fast path](performance-replay-cache-fast-path.md) | A hit constructs and rigs a temporary replay runtime | Audit initialization effects and measure hit cost | Can any work be removed without losing recovery or initialization effects? |

This order reflects risk and evidence strength, not a measured speed ranking.
Split each proposal into the phases it describes.
Do not combine representation changes, cache policy changes, and consensus ordering changes in one implementation patch.

Existing optimizations remain part of the baseline:

- Soft checkpoints already use shared persistent maps.
- LMDB reads already avoid a global database mutex.
- Conflict discovery already uses event indexes.
- Multiset differences already use occurrence counts.
- Rejection selection already prepares numeric loss and cost keys.
- Replay-cache event logs already use shared ownership and bounded retention.

These designs do not replace those mechanisms with earlier implementations.

## Source Baseline

The inspected revision is `0cefdb6b5cacad069e47f29d157227b5758f5895`.
Relative source links in the proposals provide navigation to the current checkout.
The following fixed references preserve the main implementations used as evidence:

- [Key/value helper methods](https://github.com/machieke/f1r3node-rust/blob/0cefdb6b5cacad069e47f29d157227b5758f5895/shared/src/rust/store/key_value_store.rs)
  (local Git: `0cefdb6b5cacad069e47f29d157227b5758f5895:shared/src/rust/store/key_value_store.rs`).
- [LMDB implementation](https://github.com/machieke/f1r3node-rust/blob/0cefdb6b5cacad069e47f29d157227b5758f5895/shared/src/rust/store/lmdb_key_value_store.rs)
  (local Git: `0cefdb6b5cacad069e47f29d157227b5758f5895:shared/src/rust/store/lmdb_key_value_store.rs`).
- [Hot-store snapshots, actions, and caches](https://github.com/machieke/f1r3node-rust/blob/0cefdb6b5cacad069e47f29d157227b5758f5895/rspace%2B%2B/src/rspace/hot_store.rs)
  (local Git: `0cefdb6b5cacad069e47f29d157227b5758f5895:rspace++/src/rspace/hot_store.rs`).
- [Checkpoint serialization and publication](https://github.com/machieke/f1r3node-rust/blob/0cefdb6b5cacad069e47f29d157227b5758f5895/rspace%2B%2B/src/rspace/history/history_repository_impl.rs)
  (local Git: `0cefdb6b5cacad069e47f29d157227b5758f5895:rspace++/src/rspace/history/history_repository_impl.rs`).
- [State-change ordering](https://github.com/machieke/f1r3node-rust/blob/0cefdb6b5cacad069e47f29d157227b5758f5895/rspace%2B%2B/src/rspace/merger/state_change_merger.rs)
  (local Git: `0cefdb6b5cacad069e47f29d157227b5758f5895:rspace++/src/rspace/merger/state_change_merger.rs`).
- [Conflict-set ordering](https://github.com/machieke/f1r3node-rust/blob/0cefdb6b5cacad069e47f29d157227b5758f5895/casper/src/rust/merging/conflict_set_merger.rs)
  (local Git: `0cefdb6b5cacad069e47f29d157227b5758f5895:casper/src/rust/merging/conflict_set_merger.rs`).
- [Deploy admission](https://github.com/machieke/f1r3node-rust/blob/0cefdb6b5cacad069e47f29d157227b5758f5895/casper/src/rust/engine/multi_parent_casper/block_admission.rs)
  (local Git: `0cefdb6b5cacad069e47f29d157227b5758f5895:casper/src/rust/engine/multi_parent_casper/block_admission.rs`).
- [Interpreter charging and compilation](https://github.com/machieke/f1r3node-rust/blob/0cefdb6b5cacad069e47f29d157227b5758f5895/rholang/src/rust/interpreter/interpreter.rs)
  (local Git: `0cefdb6b5cacad069e47f29d157227b5758f5895:rholang/src/rust/interpreter/interpreter.rs`).
- [Deploy-specific normalizer environment](https://github.com/machieke/f1r3node-rust/blob/0cefdb6b5cacad069e47f29d157227b5758f5895/models/src/rust/normalizer_env.rs)
  (local Git: `0cefdb6b5cacad069e47f29d157227b5758f5895:models/src/rust/normalizer_env.rs`).
- [Replay cache use and runtime spawning](https://github.com/machieke/f1r3node-rust/blob/0cefdb6b5cacad069e47f29d157227b5758f5895/casper/src/rust/util/rholang/runtime_manager.rs)
  (local Git: `0cefdb6b5cacad069e47f29d157227b5758f5895:casper/src/rust/util/rholang/runtime_manager.rs`).
- [Runtime initialization](https://github.com/machieke/f1r3node-rust/blob/0cefdb6b5cacad069e47f29d157227b5758f5895/rholang/src/rust/interpreter/rho_runtime.rs)
  (local Git: `0cefdb6b5cacad069e47f29d157227b5758f5895:rholang/src/rust/interpreter/rho_runtime.rs`).

## Reproducible Operation Probe

The [standalone Rust probe](bench/operation-count-probe.rs) models three ownership patterns and repeated sort-key evaluation.
It uses synthetic payloads and the standard library.
It does not call node code, hash channels, access a database, or measure elapsed time.
Its assertions compare values and operation counts within those models.
They are not production regression tests or consensus proofs.

Run from the repository root:

```bash
mkdir -p target/proposal-probes
rustc --version
rustc --edition=2021 -O -D warnings docs/improvement-proposals/bench/operation-count-probe.rs -o target/proposal-probes/operation-count-probe
target/proposal-probes/operation-count-probe
```

The probe isolates payload bytes copied by `Clone`.
It excludes key clones, collection headers, allocator overhead, and upstream decoding or serialization.
Consuming a value does not eliminate the allocation that originally created it.

| Synthetic case | Existing pattern | Proposed pattern |
| --- | ---: | ---: |
| Three groups, each with 256 payloads of 2,048 bytes | 768 clones, 1,572,864 copied bytes | 0 payload clones |
| One returned payload of 1,048,576 bytes | 1 clone, 1,048,576 copied bytes | 0 payload clones |
| Conditional batch of 256 payloads, each 2,048 bytes | 256 clones, 524,288 copied bytes | 0 payload clones |
| Prepared sorting key for `N` values | Compiler-dependent repeated evaluation | Exactly `N` key evaluations |

Sorting tests include unique keys, repeated keys, and empty input.
Equal-key cases check stable input order.
The reported sort counts depend on the standard-library implementation and input distribution.
A reduction in key evaluations is not an equivalent latency improvement.

The probe passed on 2026-11-25 with `rustc 1.95.0-nightly (6efa357bf 2026-02-08)` and the command above.
For 1,024 unique keys, it counted 23,076 existing key evaluations versus 1,024 prepared evaluations.
For 1,024 inputs sharing seven keys, it counted 8,638 versus 1,024 evaluations.
All copy-count, value-equivalence, and stable-order assertions passed.
These results are reproducible synthetic evidence, not results from the node's production methods.

## Shared Acceptance Conditions

These conditions describe the proposed implementation gates, not completed correctness evidence.
The existing [verification tiers](../cbc-verification-tiers.md) apply where the affected subsystem requires them.
Tests below do not replace those tiers or establish an unbounded correctness claim.

### Behavior

- Preserve canonical bytes, state roots, channel hashes, and event multiplicity.
- Preserve accepted and rejected deploy sets, ordering, charges, and error classifications.
- Preserve finalized-floor scope, merge recovery, and duplicate-execution protection.
- Preserve replay admission limits, root pins, pruning coordination, and cancellation cleanup.
- Preserve stored formats, external schemas, and mixed-version compatibility.
- Treat cache failure or eviction as lost performance, never as missing contract state or successful validation.

Do not assume that fewer instructions permit a weaker failure contract.
Any necessary behavior change needs a separate design and review.

### Benchmarks

Record the revision, compiler, build profile, CPU, allocator, resource limits, and fixture identifiers.
Separate wall time, CPU time, allocation count, copied bytes, live heap, and resident memory.
File-backed pages and allocator-retained pages need separate interpretation.
Report latency at the 50th, 95th, and 99th percentiles with repeated trial ranges.

Use at least five baseline and five candidate trials per selected case.
Alternate the execution order and record cold versus warmed state.
Choose practical improvement and regression thresholds from baseline variability before evaluating the candidate.
Reject a change that moves cost into another phase or merely increases retained heap without a justified benefit.

Measure each proposal independently, then test combinations.
Do not add percentage savings from overlapping allocations.
For example, storage helper copies and checkpoint action copies occur at different stages of the same checkpoint.

New metrics are proposed, not claimed as existing instrumentation.
Use bounded labels such as component, phase, result, and cache kind.
Do not use deploy signatures, source text, block hashes, or channel values as labels.
Never log private contract data to collect performance evidence.

### Integration Workloads

Use disposable test state and separately approved shard tests.
Compare state and verdicts after the same ordered inputs, not only transaction submission rate.
Retain baseline regression cases while adding targeted tests for each changed path.

| Workload | Required observation |
| --- | --- |
| 100 concurrent counter deploys | Exactly 100 finalized increments, with no duplicate effect after replay or merge recovery |
| Repeated source with different signatures and parameters | Independent deploy bindings and unchanged charges |
| Large continuation and multi-channel join | Identical leaves, roots, ordering, and peak-memory accounting |
| Divergent branches with rejected deploy recovery | Identical rejection decisions and eventual canonical inclusion |
| Root pruning, restart, and state transfer | Available retained roots and unchanged missing-state behavior |
| Idle finality after traffic | Continued finalization, bounded unresolved scope, and no restart or memory-growth regression |

The existing [replay benchmark](../../casper/tests/util/rholang/cost_accounting_perf_spec.rs) provides a starting point for execution timing.
The [conflict benchmark](../../casper/tests/merging/conflict_detection_perf_spec.rs) covers related merge workloads.
Neither is a complete regression suite for these proposals.

## Delivery and Rollback

Start with storage-helper moves and checkpoint concatenation as independent patches.
Add production-path regression tests before accepting each optimization.
Then evaluate prepared merge keys with a reference comparator.
Proceed to caches only when measured reuse and explicit lifetime rules justify them.

No proposed phase changes the disk format.
A previous binary should remain able to open the same state.
Cache experiments should support disabling their optimization without discarding authoritative state.
Do not introduce runtime pooling or cross-root cache sharing as an incidental extension.

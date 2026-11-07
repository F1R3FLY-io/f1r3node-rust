# Bounded DAG implementation and validation report

This report covers the implementation for [issue 933](https://github.com/F1R3FLY-io/f1r3node-rust/issues/933) in [PR 949](https://github.com/F1R3FLY-io/f1r3node-rust/pull/949).
The PR retains production behavior and a small set of focused regressions.
Historical experiments and broader test results are recorded here instead of retaining their additional programs in the PR.
Those historical results do not establish that the reduced test selection or a later revision has passed validation.

## Implementation boundaries

The [DAG storage documentation](README.md#dag-storage) describes configuration, query behavior, publication, and migration.
The following components are necessary for those behaviors.

| Component | Responsibility | Why eviction alone is insufficient |
| --- | --- | --- |
| Atomic storage operations | Commit related records together and limit ordered scans. | A partial write can publish membership without matching metadata or progress. |
| Durable historical indexes | Retain membership, heights, and reverse edges outside resident maps. | Consensus and APIs still query blocks and children after eviction. |
| Captured DAG views | Preserve insertion visibility using a publication sequence. | A later insertion must not appear in an older view through a disk fallback. |
| Startup and migration | Resume indexing in bounded pages and reconstruct the resident height band. | Rebuilding every historical map at startup would restore the original memory problem. |
| Persistent garbage collection | Store traversal work, visited records, and retry progress on disk. | An ancestry traversal can otherwise allocate a history-sized heap frontier. |
| Fallible readers | Propagate storage failures through consensus, recovery, and APIs. | A damaged historical record must not silently become an absent block. |

The height window does not impose an absolute process-memory limit.
Fork width, unfinalized height growth, and retained old views can increase memory independently of finalized historical retention.
Finality metadata remains mutable, even when a view preserves its insertion visibility boundary.
The change does not alter block hashes, signatures, or the upstream rules for accepting stored blocks.

## Retained regression coverage

The focused selection targets six failure boundaries.
Existing repository tests retain the API and fixture adaptations required by fallible historical reads.
They are not replaced with these six regressions.

| Regression | Observable result |
| --- | --- |
| Atomic rollback and ordered scanning | A failed transaction leaves no partial publication, and a requested scan stays within its page bound. |
| Cold queries and captured-view consistency | Evicted blocks and complete child relations remain queryable, while an older view excludes later insertions. |
| Restart and legacy migration | Reconstructing storage objects from persisted history restores the resident band and preserves historical lookup results. |
| Invalid future blocks and damaged historical reads | Invalid heights do not expand the valid resident horizon, and corrupted records produce errors. |
| Garbage-collection resumption | Replacing the coordinator during traversal preserves safe deletion results and reclaims stored scratch records. |
| Historical finality-floor lookup | A cold held floor remains usable, an absent floor falls back, and a storage error propagates. |

The tests live in [atomic storage](../../shared/src/rust/store/atomic_store.rs), [bounded DAG integration](../../block-storage/tests/bounded_dag_test.rs), [GC traversal](../../casper/src/rust/util/mergeable_channels_gc/job.rs), and [finality-floor containment](../../casper/src/rust/finality/floor.rs).

The source reduction requires its own compile, regression, and CI results.
The historical ledger below is not a substitute for those checks.
Removing broader tests also removes their ongoing regression protection, even when an earlier execution passed.
The storage reconstruction test runs within one process.
The GC regression uses the atomic in-memory backend and replaces its coordinator while retaining the store.
Neither test establishes recovery after an operating-system process crash.

### Coverage moved out of the executable change

The reduction removes the allocator, restart, consensus-timing, and network-soak programs, together with their fixture backends and diagnostic accessors.
It also removes the extra fixed-clock proposer entry point and test-only window overrides needed by those programs.
The standard proposer still obtains its timestamp from the system clock.

The larger test selection previously exercised individual history-index encodings, GC scratch operations, publication failure paths, cold dependency checks, initialization, observer behavior, and node processing errors.
Their recorded results belong to the historical revisions below.
They are no longer independent regression checks on every future change.
The retained tests cover the principal storage and GC boundaries, but do not reproduce every removed failure injection or network-consensus scenario.
Existing tests from the base branch remain, including the fixture and API updates needed to run them against the new storage interface.

### Validation of the reduced source

Workspace formatting and the locked, offline all-target workspace compile passed after removing the programs and test hooks.
The production-node compile with default features disabled also passed.
The complete shared and block-storage release suites passed 454 tests, with no failures and one existing ignored test.
This includes atomic rollback on both backends and all three focused LMDB regressions.
The GC module passed 28 tests and the finality-floor module passed 39 tests, including the retained coordinator-resumption and cold-history containment regressions.
Together these release runs passed 521 tests, with no failures and one existing ignored test.
The existing soak-summary script regression also passed after removing the added benchmark runners.
CI coverage must be recalculated for this selection; earlier coverage percentages do not apply.

Reproduce the release regression runs with:

```bash
rtk cargo test --locked --offline --release -p shared -p block-storage
rtk cargo test --locked --offline --release -p casper --lib rust::util::mergeable_channels_gc
rtk cargo test --locked --offline --release -p casper --lib rust::finality::floor
```

The offline commands require the pinned toolchain and dependencies to be available locally.

The strict CbC discharge check reports two unresolved artifact claims under [CLAIM-CASPER-BUFFER-001](../claims/casper-buffer-release.md): `buffer_resolver.rs` and `block_processor_instance.rs`.
This reduction only removes added tests and their fixture support in those two files.
The claim remains pending in the repository; this report neither discharges it nor substitutes for acceptance.

## Historical evidence

Results are tied to these source revisions:

- **Integration revision:** [`789da0b9ed2a7a89af956faf836156edd8d4ac1f`](https://github.com/F1R3FLY-io/f1r3node-rust/commit/789da0b9ed2a7a89af956faf836156edd8d4ac1f).
- **Finality revision:** [`e454b8c722ef93082c2924d3710caf248103fba4`](https://github.com/F1R3FLY-io/f1r3node-rust/commit/e454b8c722ef93082c2924d3710caf248103fba4).
- **Measured candidate:** [`b3a9b7caa013dc8fa5ad0408632d2ac4fee6e24b`](https://github.com/F1R3FLY-io/f1r3node-rust/commit/b3a9b7caa013dc8fa5ad0408632d2ac4fee6e24b).
- **Archived validation tools:** [`a3cf9546134a6597549a2223786246d583e1d196`](https://github.com/F1R3FLY-io/f1r3node-rust/commit/a3cf9546134a6597549a2223786246d583e1d196).
- **Comparison reference:** [`2cce114a52ac1e1a85bbd03e0120de848df80d6d`](https://github.com/F1R3FLY-io/f1r3node-rust/commit/2cce114a52ac1e1a85bbd03e0120de848df80d6d).

The archived revision changes only the soak summary's jq variable names relative to the measured candidate.
The local evidence was collected before the current reduction of tests and tooling.

| Source | Check | Recorded result | Limit of the evidence |
| --- | --- | --- | --- |
| Integration revision | Shared/storage, startup, node pipeline, processor, recovery, observer, and GC regressions | 644 passing test executions and one ignored test. | This precedes the finality integration and diagnostic additions. |
| Finality revision | Finality unit tests | 61 passed. | These are unit regressions, not a network soak. |
| Measured candidate | Four finality integration groups | 14 passed: ten deploy-status, two finalized-eviction, one floor-erasure, and one low-base-merge test. | Focused integration coverage does not establish latency or long-running liveness. |
| Measured candidate | Full local release suites | Block-storage, Rholang, and node passed; Rholang executed 2,367 tests. | Build and test completion do not establish bounded allocator growth. |
| Measured candidate | Production node release build and fuzz compilation | Node built with test helpers disabled; all six fuzz targets compiled. | Compilation is not a fuzz campaign. |
| Measured candidate | Rocq 9.1.1 canonical snapshot verification | Eight closed theorems, 76 production cases, and six rejection controls passed. | The proof covers canonical snapshot correspondence, not full consensus or memory bounds. |
| Measured candidate | LMDB diagnostic smoke runs | Cold lookup, captured checkpoints, reconstruction, and separate-process reopening passed. | Small fixtures cannot certify the history-size or latency gates. |
| Measured candidate/reference | Full measurement sequence | Reference, candidate, and GC binaries built successfully. The memory comparison was interrupted with exit 143. | No complete comparison, GC sweep, or consensus-latency result was accepted. |

The [measured candidate's CI run](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/42295964660) passed every crate test job.
Its script test rejected the soak runner's reserved jq variable name.
The archived revision fixes that variable, and the same local script test then passed.
The comm coverage job generated its report, but GitHub's artifact service timed out during upload.
Coverage Summary reported the missing comm artifact, and the Casper aggregate gate failed on that coverage result.
These facts do not constitute a fully green CI run.
In the [archived revision's subsequent CI run](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/42313723174), both Script Tests and Coverage (comm), including its artifact upload, passed.
That run precedes the source reduction and cannot validate the reduced test selection.

### Measured smoke values

These measurements used real LMDB storage, window depth `1`, ten validators, seven blocks per height, and a fixed finality lag of 32 heights.
The allocator values below are requested live bytes, not resident process memory or total allocator arena size.

| Stage | Total blocks | Resident blocks | Resident edges | Live allocator bytes |
| --- | ---: | ---: | ---: | ---: |
| First checkpoint | 420 | 237 | 1,659 | 905,683 |
| Second checkpoint | 840 | 237 | 1,659 | 924,971 |
| Separate process reopening the second checkpoint | 840 | 237 | 1,659 | 810,089 |

The unchanged resident cardinalities demonstrate eviction in this small fixture.
They do not establish a long-history plateau or a production byte bound.
Historical-index query and row counters were available.
Metadata-read and metadata-cache-hit counters were unavailable and recorded as `null`, not zero.
The complete metadata query-cost requirement therefore remains unverified.

## Archived tools and reproducibility

The following links preserve executable procedures without keeping their programs in the production change:

- [Reference preparation and adapter manifests](https://github.com/F1R3FLY-io/f1r3node-rust/blob/a3cf9546134a6597549a2223786246d583e1d196/scripts/bench/prepare-dag-memory-reference.sh).
- [Allocator fixture](https://github.com/F1R3FLY-io/f1r3node-rust/blob/a3cf9546134a6597549a2223786246d583e1d196/block-storage/examples/dag_memory.rs) and [comparison runner](https://github.com/F1R3FLY-io/f1r3node-rust/blob/a3cf9546134a6597549a2223786246d583e1d196/scripts/bench/run-dag-memory-comparison.sh).
- [Fresh-process restart and legacy-migration comparison](https://github.com/F1R3FLY-io/f1r3node-rust/blob/a3cf9546134a6597549a2223786246d583e1d196/scripts/bench/run-dag-restart-comparison.sh).
- [GC measurement fixture](https://github.com/F1R3FLY-io/f1r3node-rust/blob/a3cf9546134a6597549a2223786246d583e1d196/casper/examples/dag_gc_memory.rs).
- [Consensus comparison runner](https://github.com/F1R3FLY-io/f1r3node-rust/blob/a3cf9546134a6597549a2223786246d583e1d196/scripts/bench/run-dag-consensus-comparison.sh).
- [Multi-node idle, load, and catch-up runner](https://github.com/F1R3FLY-io/f1r3node-rust/blob/a3cf9546134a6597549a2223786246d583e1d196/scripts/bench/run-dag-memory-soak.sh).

Run historical reproductions from a separate checkout pinned to the complete archived source revision.
Do not copy an old fixture into the current implementation and assume identical APIs or test hooks.
For a current-revision comparison, adapt the harness explicitly and record both the adaptation and source hashes.
Build the reference and candidate serially from identical workload sources, apart from reviewed compatibility adapters.
Record the compiler, binary hashes, machine configuration, available memory, and concurrent host load.
The historical host had unrelated CPU workloads, so its partial timings do not establish isolated latency.

The archived reference-preparation command requires an explicit base:

```bash
rtk proxy bash scripts/bench/prepare-dag-memory-reference.sh \
  --base 2cce114a52ac1e1a85bbd03e0120de848df80d6d \
  --out-dir target/dag-reference-reproduction --apply
```

That command generates `commands.txt` with matching build, allocator, restart, and consensus invocations.
Review its compatibility patches and workload hashes before running those commands.
Use new output directories so failed or partial runs cannot enter a later comparison.

## Outstanding acceptance work

The following procedure records the intended experiment precisely.
None of its pending outcomes is reported as a pass.

1. Generate 120,000-, 240,000-, and 480,000-block checkpoints with window depth `1024`.
   Use ten validators, seven blocks per height, and finality lag `32`.
   Run three separate processes per implementation, alternating reference/candidate order, and retain every run.
   Exclude the first `min(10,000, interval_count / 2)` insertions from each interval's latency distribution.
2. At each history size, run three fresh reference restarts and three candidate restarts.
   Run three migrations from actual reference checkpoints.
   Use disposable copies, verify source hashes before and after, and report copy time separately from open time.
   In-process reconstruction does not count as a fresh restart.
3. Run three retained-view experiments with 20,000 inserted blocks, then release the old view and measure retained allocations again.
4. At each history size, run three GC sweeps with a low-height eligible candidate still pending.
   Record temporary heap, traversed edges, yielded turns, elapsed time, scratch bytes, and complete scratch reclamation.
5. Compare consensus traces with windows `1`, `2`, `16`, and `1024`, and parent depths `15`, `64`, and `2147483647`.
   Measure performance with depth `15`, window `1024`, 1,088 warmup proposals, and 100 measured proposals.
   Use three processes per implementation.
   Require cold-history access during every measured candidate proposal.
6. Run bootstrap, three validators, and a read-only follower continuously for 86,400 seconds, sampling every 60 seconds.
   Follow with a 300-second load phase targeting two deploys per second and at least 100 finalized samples.
   Pause the follower until its gap exceeds `W+32`, then allow at most 7,200 seconds for recovery.
   Verify the same finalized target hash and height on every validator and the follower.
   Require follower finality lag of at most eight blocks.

The proposed live-heap increase limit is 10 MiB between the smallest and largest fixtures.
The proposed migration peak-heap increase limit is 16 MiB over the same history sizes.
The GC temporary-heap increase limit is 10 MiB with fixed record sizes.
Insertion, snapshot, proposal, and peer-validation median and p95 latency must each regress by no more than 10% against the reference.
Report all three runs and the median of their quantiles, with cold-query latency separately identified.
Keep the 1,024-record page limit and 10,000-unit ancestry-work budget per GC turn visible in the result interpretation.
The turn budget charges work records and parent-edge visits; it does not promise 10,000 edges per turn.

The network report must include total history, resident cardinalities, finality progress, lag, cutoff, eviction cost, anonymous memory, disk usage, and historical-query costs.
Container anonymous memory is supporting evidence and does not replace allocator live-heap measurements.
A completed runner alone cannot certify plateauing memory, acceptable latency, or consensus liveness.
The allocator comparison, latency gates, GC measurements, and 24-hour network soak remain unfinished.

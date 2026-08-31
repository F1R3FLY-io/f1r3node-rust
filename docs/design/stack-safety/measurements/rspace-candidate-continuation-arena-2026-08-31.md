# RSpace candidate continuation arena experiment

Status: **ACCEPTED for the candidate-product traversal mechanism**

Date: 2026-08-31

Tracking: pgmcp work item 5985; experiment 5990 successor run

## Question and hypothesis

The RSpace candidate product intersects two synchronized prefix-compressed tries: the candidate
availability `PathMap<Multiplicity>` and the candidate-position `PathMap<CandidatePosition>`. The
frozen control retains cloned read zippers at continuation points. If a clone owns path state
proportional to its depth, retaining clones at successively deeper points admits the cumulative
cost

```math
\sum_{k=1}^{n} \Theta(k) = \Theta(n^2).
```

The treatment hypothesis was that one paired moving cursor plus a flat append-only continuation
arena would preserve exact candidate and capture semantics while changing traversal time to
approximately linear in query depth. The treatment was required to beat the frozen control in
every measured cell, retain a 99% bootstrap upper bound below parity in every cell, use constant
native stack, and remove rather than relocate checkpoint allocations.

## Implemented mechanism

The selected machine keeps both guarded borrowed PathMap cursors alive for the whole query. Its
continuation arena stores integer parent links, frame and byte depths, a frame source, and an
optional capture edit. Exact atoms and already-bound captures borrow their query or capture bytes;
only wildcard, first-capture, nested-predicate, and remainder frames discovered while scanning own
bytes once. A lowest-common-ancestor rebase first undoes capture edits while ascending and then
reapplies edits while descending the target suffix.

```text
Algorithm REBASE-CANDIDATE-CONTINUATION(current, target)
Input:  one paired cursor, the current arena node, and the target arena node
Output: both PathMap cursors and the capture table focused at target

1. Find the lowest common ancestor by comparing arena frame depths.
2. Ascend source-only frames on both cursors; clear each frame's capture edit.
3. Record target-only node indices in one reusable suffix buffer.
4. Visit that suffix in reverse, descending both cursors transactionally.
5. Reapply each target frame's capture edit only after both descents succeed.
6. On asymmetric failure, restore both cursor foci before refusing the branch.
```

No candidate-lane `ZipperSubtries::make_map`, subtree snapshot, raw owned
`PathMap<Arc<[u8]>>`, retained cursor clone, recursive continuation chain, or PathMap fork remains
in production. Repeated capture identifiers compile to dense `u32` slots and match through a
`Vec<Option<Vec<u8>>>`, avoiding a second trie lookup inside the candidate traversal.

## Experimental design

The release benchmark used two trie shapes, five query families, five depths, and 30 retained
counterbalanced pairs per cell:

- shapes: compressed and dense-64;
- queries: exact, repeated capture, wildcard, remainder, and nested predicate;
- depths: 256, 512, 1,024, 2,048, and 4,096; and
- arms: frozen zipper-clone control and moving-arena treatment.

This is 50 cells and 3,000 timed observations. AB/BA order was balanced within every cell. The
analysis used 100,000 pair-preserving bootstrap replicates for the treatment/control median ratio,
Wilcoxon paired tests with Benjamini-Hochberg correction, and log-log median-time slopes. The run
was single-threaded under `MemoryMax=6G`, `MemorySwapMax=0`, and `CPUQuota=200%`; the host reported
approximately 53% idle CPU immediately before the run.

Semantic anti-vacuity was mandatory: every observation carried the result count and BLAKE2b-256
result digest, and every arm/cell pair had to match. The implementation also had to equal the
frozen legacy oracle and an independent byte oracle on fixed cases and 128 fixed-seed randomized
small tries.

## Results

All 50 cells were semantically equal. Every treatment median was below its control median, and
every 99% pair-preserving bootstrap upper bound was below one. The narrowest observed win was a
1.058685-fold median speedup; the largest was 88.084504-fold. Even the relatively narrow compressed
exact depth-2,048 cell had median ratio 0.9446 and 99% upper bound 0.9660.

| Shape | Query | Treatment log-log slope |
|---|---|---:|
| compressed | exact | 0.9885 |
| compressed | repeated capture | 0.9198 |
| compressed | wildcard | 1.0544 |
| compressed | remainder | 1.0484 |
| compressed | nested predicate | 1.0901 |
| dense-64 | exact | 1.0039 |
| dense-64 | repeated capture | 0.9371 |
| dense-64 | wildcard | 1.0268 |
| dense-64 | remainder | 1.0318 |
| dense-64 | nested predicate | 1.0868 |

The frozen control slopes were approximately 0.97--0.98 for exact and repeated capture,
1.25--1.33 for wildcard, 1.36--1.43 for remainder, and 1.94/2.38 for compressed/dense nested
predicates. The moving arena therefore removes the superlinear branch-continuation slope without
penalizing deterministic queries.

### Headless allocation attribution

Heaptrack ran in `--record-only` mode with debuginfod network retrieval disabled, under
`MemoryMax=2G`, `MemorySwapMax=0`, and `CPUQuota=200%`. Each profile used the compressed depth-4,096
fixture for 100 operations. Heaptrack perturbs absolute time, so these observations attribute
allocation mechanisms and are not additional latency trials.

| Query | Arm | Allocation calls | Temporary allocations | Peak heap | Peak RSS including profiler |
|---|---|---:|---:|---:|---:|
| exact | frozen clone | 74,328 | 51,140 | 936.72 KiB | 6.98 MiB |
| exact | moving arena | 73,928 | 51,140 | 945.52 KiB | 7.23 MiB |
| repeated capture | frozen clone | 60,175 | 47,145 | 887.98 KiB | 6.76 MiB |
| repeated capture | moving arena | 58,075 | 46,945 | 896.78 KiB | 6.99 MiB |

The paired totals remove 400 calls from 100 exact operations, or 4 per operation. They remove
2,100 calls and 200 temporary allocations from 100 repeated-capture operations, or 21 and 2 per
operation respectively. The treatment's peak heap differs from control by only 8.80 KiB in each
profile; the profiler-inclusive RSS difference is at most 0.25 MiB. The result digest in all four
profiles was
`44cfd30f6f52a2788d977e2debab22bca54aa9833665b221803ad2e0526149b3`.

## Correctness and resource gates

Sixteen focused tests pass. They cover the frozen and byte oracles, dense and compressed topology,
transactional asymmetric-descent restoration, root and descendant endpoints, capture undo before
redo, malformed/preclassified scanner-prefix restoration, source-capability exclusions, dense
capture slots, randomized small tries, and exact/repeated/wildcard depth 16,384 on a 256 KiB native
thread stack. No `RUST_MIN_STACK`, `stacker`, artificial traversal limit, or recursive fallback is
used.

Rocq separately proves the flat continuation machine's 31-invariant catalog and rejects 24 causal
mutants. The proof build is admission-free and both `coqc` and `coqchk` pass at commit
`6ef40fc37`.

## Decision and boundary

The treatment is accepted for the candidate-product traversal mechanism. It is faster in every
measured cell, exhibits approximately linear treatment slopes, removes checkpoint construction,
reduces operation-level allocation calls, and preserves every observed candidate digest and oracle
result. This is a consensus-neutral implementation refinement under CBR-053 because the bytes are
derived candidate-index data and the authoritative spatial matcher remains the semantic arbiter.

This result does not independently retire CBR-053 or prove all generated associative-commutative
COMM schedules and post-states. Whole-workspace release and broader lifecycle gates remain separate
campaign obligations. The measurements are from one host; counterbalancing limits thermal and
contention drift but cannot eliminate every external source of wall-clock noise.

## Reproducibility coordinates

| Artifact | SHA-256 |
|---|---|
| raw 3,000-observation matrix | `57c72b1d19cc65f25d18962f40d906d0f005c699a377bd80c50ae67f57f7eff6` |
| statistical analysis JSON | `753a25b406490eed593f0c10f97831c4ee729c066974fbc4963f1c07ffd3c716` |
| derived cell summary | `38a774c871c872135b17c35288a5f5b89121bb7fb1ade13b0b5d2db2e868011b` |
| release profile executable | `df701100234b2e5669e6e5d5eac26dd7810e1ef2f570488ef13a4ad919abc7fc` |
| exact control Heaptrack trace | `6c3c7c2aaf51b6abc6e758b9e9eb4304c7a05af0abae815ec683da4c20390420` |
| exact treatment Heaptrack trace | `45227ce4eab0a6a7507002e9cdb500838d945a715cd80d0fdd55027208ed7443` |
| repeated-capture control Heaptrack trace | `f9c7adbbfef2a3ea476c82726438747fac02774094d34ddf8d8089bb9831c428` |
| repeated-capture treatment Heaptrack trace | `f46edf4fcebb85972eab509091d10ef6cbf7cc250f3ec72ea120e243d30af981` |

The large Heaptrack traces are machine-local scratch products and are identified by hash rather
than committed. The TSV, JSON, summary, analyzer, benchmark source, focused tests, and this report
are the durable reproduction record.

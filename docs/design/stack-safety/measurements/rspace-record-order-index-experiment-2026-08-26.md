# RSpace continuation/join record-order PathMap experiment

Status: **both retained order-index candidates rejected; production `sorted_prepared` retained**

Date: 2026-08-26

Pgmcp work item: `rspace-continuation-join-pathmap-authority` (#6079)

## 1. Question and terminology

The production continuation/join store keeps complete canonical record bytes as endpoints of its
primary prefix-compressed `PathMap`. The same record carries fixed-width `u64` ordinal metadata for
the scheduler-visible, newest-first order. A **projection** produces that operational order; it is
not membership authority and it is not canonical cold-store order. The production control walks the
primary trie, collects borrowed `Arc` records with their ordinals, and sorts those scalar/borrowed
descriptors. It never constructs a `Vec<Par>` or decodes a canonical record.

The experiment asks whether a second retained PathMap in ordinal order repays its complete
lifecycle cost. It compares three representations at source coordinate `5ca3ca513`:

| Arm | Retained representation |
|---|---|
| `sorted_prepared` | Production primary/identity PathMaps plus transient ordinal descriptors. |
| `shared_record_order_trie` | Production roots plus `PathMap<Arc<StoredRecord<_>>>`, keyed by group prefix and reversed big-endian ordinal. |
| `split_payload_order_trie` | Production roots plus separate record, identity, and ordinal PathMaps whose values share immutable payloads. |

All arms preserve the production `OrderedGroupStore`. The two treatments deliberately add indexes
instead of replacing authority so their incremental cost can be measured without changing RSpace
semantics.

## 2. Frozen decision rule

For each candidate/control timing ratio $`r_i`$, the analyzer computes a pair-preserving 99%
bootstrap interval over 31 counterbalanced retained sets. A candidate may be selected only if every
one of these gates passes:

1. complete logical observations, canonical leaf bytes, canonical leaf digest, primary paths,
   identity paths, ordinals, next ordinal, and exact encoded-length accounting equal the control;
2. the upper 99% bootstrap bound for every lifecycle cell is at most 1.05;
3. the upper 99% bootstrap bound for every repeated-projection cell is at most 1.05; and
4. the candidate/control retained-byte lower-bound ratio is at most 1.05 in every case.

Benjamini--Hochberg correction is applied to the complete family of paired two-sided Wilcoxon
tests. The confidence interval is the selection statistic; the corrected test is retained as a
distributional diagnostic. A failure in any cell rejects the candidate. This deliberately prevents
a large wide-case projection win from hiding a mutation, narrow-case, or resident-memory loss.

## 3. Protocol

The matrix contains 14 fixtures across continuation, resting-join, and installed-join collections.
It includes 1, 16, 256, and 4,096 distinct records; join multiplicities 16 and 4,096; shared and
unshared prefixes; record bodies of 16, 64, and 4,096 bytes; and group prefixes of 32 and 256 bytes.
Nine lifecycle operations cover projection, construction, replacement, insertion, removal by
identity, removal by canonical path, copy-on-write clone/update, clear, and drop. Projection reuse
is measured at 1, 4, 16, and 64 traversals.

Each fixture/operation has five warmups followed by 31 retained sets. The six permutations of the
three arms rotate across sets. Timing and exact operation-scoped allocation counters are recorded
separately. Retained bytes are a conservative owned-byte floor while the complete fixture remains
reachable.

**Algorithm 1 (validated paired matrix).** The semantic witness is checked outside the timer.

```pseudocode
Input: fixture F, lifecycle operation O, arm permutation P
control := execute(F, O, sorted_prepared)
for arm in P do
    candidate := execute(F, O, arm)
    require candidate.complete_observation = control.complete_observation
    reset allocation counters
    measure execute(F, O, arm)
for reuse in {1, 4, 16, 64} do
    measure reuse consecutive operational projections from the same ready fixture
```

The analyzer refuses unknown row kinds or arms, duplicate samples, negative counters, zero timing,
out-of-range set numbers, unequal logical witnesses, missing arms, and incomplete lifecycle or
projection matrices before computing statistics. The accepted input contains 33,894 rows:
42 semantic checks, 11,718 lifecycle timings, 11,718 lifecycle allocation observations, 5,208
projection timings, and 5,208 projection allocation observations.

Admission observed a 32-CPU host at 73.61% idle with no `cargo` or `rustc` process. The release
benchmark ran in a systemd user scope with `MemoryMax=12G`, `MemorySwapMax=0`, and
`CPUQuota=2400%`. Statistical analysis used 100,000 pair-preserving bootstrap replicates with seed
`0x4131602920260826` under `MemoryMax=8G`, zero swap, and a 16-core quota.

## 4. Result

Every semantic witness agrees. Both auxiliary-order candidates nevertheless fail all-category
selection.

| Candidate | Lifecycle median-ratio range | Projection median-ratio range | Retained ratio range | Retained failures | Lifecycle-time failures | Projection-time failures |
|---|---:|---:|---:|---:|---:|---:|
| shared record/order trie | 0.384--304.801 | 0.268--2.042 | 1.035--11.048 | 117 | 110 | 4 |
| split payload/order tries | 0.406--311.945 | 0.269--2.029 | 1.845--11.052 | 126 | 113 | 4 |

The best projection cell is the 256-record, 4,096-byte continuation at reuse 16: the shared and
split candidates take 0.268 and 0.269 times the control median. The worst cell is a single distinct
resting join with multiplicity 4,096. Removing its canonical path requires deleting 4,096 ordinal
endpoints, producing lifecycle ratios above 304 for both candidates. That same repeated-record
fixture also makes a one-use projection approximately twice as slow. The treatments therefore do
not merely lose a fixed amount on small inputs; occurrence-expanded order endpoints expose exactly
the multiplicity-dependent time and retained-space slope the PathMap Bag representation is meant
to avoid.

The shared candidate narrowly reaches a 1.035 retained ratio in one wide-record fixture, but the
decision is intentionally universal rather than average. It exceeds the 1.05 ceiling in 117 of 126
lifecycle cells and reaches 11.048 under repeated joins. The split candidate exceeds the ceiling in
all 126 cells. Projection-only speedups cannot compensate for retained authority duplicated across
every mutation, CoW clone, and resident checkpoint interval.

## 5. Decision and reusable PathMap rule

**Retain production `sorted_prepared`; reject both secondary order PathMaps.** The experiment-only
feature remains a reproducibility harness and cannot enter an ordinary build. No production
RSpace representation, scheduler order, canonical leaf byte, hash, COMM result, post-state,
metering rule, or PathMap crate source changes as a consequence of this verdict.

The positive rule is representation-specific:

1. use PathMap endpoint order directly when it is already the required external order, as canonical
   continuation/join checkpoint egress does;
2. when operational order differs, sort only scalar/borrowed descriptors and keep the trie as sole
   membership authority; and
3. never occurrence-expand a Bag into a retained order trie solely to remove that transient sort.

This verdict closes the record-order representation decision. A subsequent production change now
preflights an immutable CoW-root plan and can replay exact canonical ranges into a fallible sink;
the existing LMDB action materializes one exactly reserved compatibility buffer from that plan. The
complete continuation/join RSpace lifecycle has not yet been proved equivalent, and the planned
binary-WAL/content-addressed PathMap-DAG persistence backend is not implemented. Those remain
independent tracked obligations.

## 6. Durable evidence

| Evidence | SHA-256 |
|---|---|
| [validated raw matrix](rspace-record-order-index-raw-2026-08-26.tsv) | `50266fb4969b3f947770151e5b661568aebd6d0c665257723fd061bdf9a5b0ca` |
| [statistical analysis](rspace-record-order-index-analysis-2026-08-26.json) | `310062f638185c0753c8753aa6648fff59cc1822ce5e96898a2cd483aae9f8ee` |
| [compact statistical summary](rspace-record-order-index-summary-2026-08-26.tsv) | `dff8fd37c32468b6c432a75c4bade98950370342db741d5ca4fe2cdd271bbeba` |

The analyzer is
[`scripts/bench/analyze-rspace-record-order-index.py`](../../../../scripts/bench/analyze-rspace-record-order-index.py).
It records its input digest, Python/platform coordinates, schema, matrix dimensions, bootstrap seed,
per-cell statistics, every failed guardrail, and the final production decision. The source benchmark
is feature-gated as `record-order-index-experiment` and is not compiled by default.

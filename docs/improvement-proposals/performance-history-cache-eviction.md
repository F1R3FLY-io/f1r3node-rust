# Evaluate Incremental History Cache Eviction

Status: draft hypothesis. Actual reload cost must be measured before changing eviction.
Date: 2026-11-25.

## Objective

Avoid discarding reusable history-cache entries when one capacity boundary is reached.
Preserve root isolation, authoritative hot-store state, bounded retention, and low-cost accounting.
The [performance acceptance conditions](performance-overview.md#shared-acceptance-conditions) apply.

## Current Behavior

The [hot store](../../rspace++/src/rspace/hot_store.rs) has separate history caches for continuations, data, and joins.
`enforce_history_cache_bounds` clears a whole map when its entry or item threshold is reached.
At this source revision, each kind uses thresholds of 512 entries and 8,192 items.
The enforcement call examines all three kinds before a history-cache lookup.

The implementation uses atomic item counters to avoid full-map scans during enforcement.
Do not restore a scan of all cached values on each access.
Existing bulk-clear metrics show clear events, not the cost of subsequent reads.

A clear does not necessarily cause a disk lookup.
Normal access first checks active hot-store maps, which can retain values materialized from history.
Snapshot access and repeated history reads can have different paths.
The relevant quantity is reusable data lost by clearing that the active maps do not already supply.

The history reader belongs to a fixed base state for that hot-store instance.
Reset and checkpoint paths must continue to respect that root boundary.
Cross-root reuse keyed only by channel is not part of this proposal.

## Phase 0: Establish Causality

Add bounded metrics at existing access boundaries before replacing eviction.
Separate active-map hits, history-cache hits, history-reader calls, and decoding work.
Record bulk clears and root resets independently.
Measure latency and bytes for each cache kind.

Use controlled fixtures to determine whether a clear causes a later reload of still-relevant data.
Do not add an unbounded production map of recently evicted channels for attribution.
Compare cache access traces from synthetic workloads against the unchanged implementation.

If active-map retention masks most clears, stop after measurement.
An incremental eviction policy would add complexity without addressing the measured bottleneck.
If root resets dominate misses, better eviction within one root will not solve them.

## Candidate Policy

Compare a bounded second-chance policy with a weighted least-recently-used policy.
Select one through hit-rate, lock-contention, metadata, and CPU measurements.
Keep the existing per-kind entry and item limits during comparison.
Do not increase limits to manufacture a better hit rate.

Evict only enough cached entries to admit a new value within both limits.
If a single value exceeds a cache budget, return it uncached.
Do not purge the whole useful cache to retain one oversized value.
Do not reject valid state reads because an optional cache cannot retain their result.

Keep active mutable data, continuations, installed continuations, and joins outside eviction authority.
Those maps are not interchangeable with a read-through cache.
Preserve the distinction between absent cache entries, cached empty results, and active deletion markers.

## Concurrency and Accounting

Use a short synchronization boundary for cache metadata, insertion, removal, and charge updates.
Read and decode history outside that boundary.
Recheck the cache before publication after a concurrent miss.
Release losing duplicate results without double charging.

Maintain checked accounting for entries and retained items.
An insertion or removal updates membership and its charge as one operation.
Cancellation releases any admission reservation.
Clear and late-fill operations must not reset counters while retaining uncharged entries.

Prefer a cache object whose history-reader root never changes.
If an object can be cleared or reused during an outstanding read, use a generation check before publishing the result.
Such a check does not authorize sharing values across different roots.
Do not hold cache metadata locks across storage I/O or an async wait.

Evicting an `Arc` does not immediately free payloads retained by callers or active maps.
Report cache-owned bytes and process live bytes separately.
Entry and item limits are not a complete heap-byte limit for large continuation bodies.
A byte-weighted limit needs a separate size-accounting design, not an arbitrary item-to-byte conversion.

## Regression Plan

| Case | Required assertion |
| --- | --- |
| Hot set plus a sequential scan | Useful entries survive where the selected policy predicts |
| Uniform scan and cycle larger than capacity | No false performance claim when locality is absent |
| Concurrent identical and different misses | Correct values and exact insertion/removal accounting |
| Empty history result and active deletion marker | No resurrection of consumed data |
| Root reset during outstanding read | No publication into the wrong root or generation |
| Oversized single record | Correct uncached read without unrelated cache destruction |
| Eviction during execution or rollback | Authoritative hot state and checkpoint results remain intact |
| Cancellation, clearing, and counter boundary cases | No underflow, leaked reservation, or uncharged retained entry |

Test real hot-store accessors, not only a standalone cache container.
Include paths where the active map prevents a history-cache miss.
Otherwise a synthetic cache benchmark can overstate the node-level benefit.

## Benchmark and Decision

Use the same root lifetime, read sequence, and capacity for baseline and candidate.
Measure hit rates, attributable reloads, decoded bytes, CPU, lock wait, and latency percentiles.
Include concurrent readers and snapshot rollback workloads.
Measure metadata allocations and time spent choosing victims.

Compare steady state near capacity, bursts over capacity, and frequent root resets.
Keep low-locality cases as controls.
Proceed only when reduced reads outweigh the new bookkeeping and contention.

Release an experimental policy with a local disable path and unchanged authoritative state.
Rollback drops optional cache contents and uses the previous policy.
It requires no database migration or change to pruning rules.

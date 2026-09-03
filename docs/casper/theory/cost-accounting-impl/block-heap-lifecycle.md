# Block-Heap Lifecycle and Reclamation

This document specifies the memory-lifecycle boundary for block creation and
validation. It complements the semantic garbage collection of mergeable-channel
evidence: the evidence collector decides when consensus-adjacent records are no
longer needed, while block-heap reclamation returns already-deallocated replay
and RSpace pages to the operating system.

## Terms and scope

- **Live block heap** is memory still reachable from a block-creation or
  block-validation task.
- **Retained heap** is allocator-owned memory whose Rust values have already
  been dropped but whose pages remain resident in the node process.
- **Resident set size (RSS)** is the physical memory currently attributed to a
  process by the operating system.
- **Completion boundary** is the final drop guard of a block-processing or
  block-creation attempt. Every transient value created after that guard has
  already been dropped, including values on rejected and unwound paths.
- **Semantic garbage collection** removes reconstructible merge evidence only
  after finality, horizon, child, latest-message, and complete-DAG advancement
  guards pass.
- **Allocator reclamation** asks glibc to release whole free pages. It never
  changes RSpace, block-DAG, finality, cost, vault, or replay state.

The Linux `malloc_trim(3)` interface is GNU-specific, thread-safe, and attempts
to release free heap pages through allocator and operating-system mechanisms.
Its return value reports whether any page was released; it is not a promise
that every free byte becomes nonresident. See the
[Linux man-pages specification](https://man7.org/linux/man-pages/man3/malloc_trim.3.html).

## Why Rust destruction was not sufficient

Rust ownership correctly dropped the temporary replay runtime and its RSpace
objects. The glibc allocator nevertheless retained many freed pages in process
arenas for reuse. Consequently, live-cache counters remained small while RSS
ratcheted upward until the integration host's resource guardian terminated the
nodes. Reducing block-processing parallelism did not cure the problem because
the leak-shaped observation was retained free heap, not additional live tasks.

Calling the allocator at the completion boundary changed that diagnosis
decisively: the same six-node bonding workload stayed near 2.1–2.7 GB total RSS
with per-block reclamation, instead of entering the prior multi-gigabyte growth
regime. This is evidence for allocator retention, not permission to weaken any
semantic cache, replay, or consensus invariant.

## Layered ownership

| Layer | Owner | End of lifetime | Safety rule |
| --- | --- | --- | --- |
| Cost witness | Runtime budget and authenticated block payload | Finalization/reporting policy | Never discard evidence required by replay or settlement. |
| Mergeable evidence | Runtime manager and mergeable-evidence collector | Exact finalized execution passes every retirement guard | Delete only the complete execution key; reconstruct only by local authenticated replay. |
| Replay objects | One block creation or validation task | Task completion or failure unwind | Rust ownership drops all transient values. |
| Free allocator pages | glibc process arenas | Completion-boundary reclamation | Reclaim after the transient owners are gone; never couple reclamation to consensus state. |
| Process RSS | Operating system | Successful page release | Enforce with the integration resource guardian because allocator release is platform behavior. |

These layers are deliberately independent. A node may retain valid mergeable
evidence while releasing unrelated free heap pages, and it may conservatively
decline evidence collection without allowing allocator retention to grow across
every completed block.

## Completion algorithm

The implementation has one incoming-block lifecycle owner in
`BlockProcessorInstance`. A guard declared before the processing body runs last
on normal return, error return, and panic unwinding. Local proposal creation has
the same outer guard in `BlockCreator`, so failed or empty proposal attempts do
not retain their temporary search/replay heap. The incoming path uses an atomic
bounded counter because multiple block tasks may complete concurrently.

```text
complete_block(interval):
    if interval = 0:
        return

    repeat:
        current := atomic_load(completions_since_trim)
        if current >= interval - 1:
            next := 0
            reclaim := true
        else:
            next := current + 1
            reclaim := false
    until compare_exchange(current, next) succeeds

    if reclaim:
        request_glibc_reclamation()
        increment_allocator_trim_metric()
```

Resetting at the configured boundary avoids machine-integer wraparound for
every positive interval. A corrupted or obsolete counter greater than the
boundary also resets rather than delaying reclamation for another full numeric
cycle. Relaxed atomic ordering is sufficient: the counter chooses a cadence but
does not publish semantic memory, and `malloc_trim(3)` is independently
thread-safe.

## Resource envelope

Let $`P`$ be the maximum number of concurrently live block tasks, $`C`$ the
maximum transient heap attributable to one task, and $`I > 0`$ the reclamation
interval. Immediately before the next successful abstract reclamation, at most
$`I-1`$ completed task heaps can remain retained. The abstract envelope is:

```math
R \leq \left(P + I - 1\right)C.
```

The production default is $`I=1`$, reducing the abstract envelope to:

```math
R \leq PC.
```

This equation is a refinement contract, not a claim that glibc can always
release every fragmented page. The TLA+ and Rocq models prove the scheduling,
counter, and noninterference obligations under the abstract reclamation
contract. The bounded integration workload and RSS guardian validate the
platform refinement on the Linux/glibc deployment target.

## Consensus noninterference

Reclamation runs only after block semantics have been computed. It has no
reference to the block hash, state root, deploy order, vault balances, cost
witness, merge vector, parent selection, latest-message map, clique, or
finalization threshold. Therefore it cannot choose or modify a consensus
transition.

The formal artifacts make that separation explicit:

- `BlockHeapLifecycle.tla` updates the committed history and an independent
  semantic reference identically while exhaustively interleaving two block
  slots. `ReclamationIsSemanticallyInvisible` requires equality in every state.
- `BlockHeapLifecycleMissingBoundaryUnsafe.cfg` retains the same semantic
  history but must violate `ResidentWithinIntervalEnvelope`.
- `BlockHeapLifecycle.v` proves the bounded counter, exact cadence, default
  per-completion reclamation, semantic equality with reclamation enabled or
  disabled, interval bound preservation, and the missing-boundary witness.
- `loom_block_heap_lifecycle.rs` exhausts concurrent completion-counter and
  retained-heap schedules under the production compare-and-exchange protocol.

The unsafe control matters: a safe semantic history does not imply a bounded
validator process. Resource safety is an additional implementation refinement,
not a replacement for consensus correctness.

## Configuration and operations

`F1R3_MALLOC_TRIM_EVERY_BLOCKS` selects the positive completion interval on
Linux with glibc:

| Value | Behavior |
| ---: | --- |
| unset, empty, or invalid | Use the production default of `1`. |
| `1` | Request reclamation after every completed incoming block-processing task. |
| positive $`I`$ | Request reclamation after every $`I`$ concurrent completions. |
| `0` | Disable explicit incoming-block reclamation. This removes the bounded retained-heap contract and requires a separately demonstrated deployment envelope. |

Non-glibc targets compile a no-op boundary because `malloc_trim(3)` is not a
portable allocator interface. Such targets need allocator-specific evidence
before claiming the Linux RSS envelope. Operators should observe
`f1r3fly.casper.allocator.trim.total`,
`f1r3fly.casper.block.processing.active`, and
`f1r3fly.casper.process.rss.kb` together: trim attempts without an RSS plateau
indicate live allocation, fragmentation, or a platform-refinement failure and
must not be hidden by raising the guardian ceiling.

## Verification matrix

| Obligation | Example/property test | Concurrency test | Formal proof | End-to-end test |
| --- | --- | --- | --- | --- |
| Default and invalid configuration select `1` | Node unit tests | — | TLA+/Rocq positive interval | Rebuilt-node run with the variable absent |
| Counter never wraps or exceeds its interval | Arbitrary-`usize` proptest | Loom completion races | `positive_interval_counter_is_bounded` | Long-running block stream |
| Exactly one request occurs per interval | Boundary examples | Loom intervals `1` and `2` | `trim_is_requested_exactly_at_the_boundary` | Trim metric cadence |
| Reclamation cannot change committed semantics | — | Loom semantic-commit equality | TLA+ semantic reference and Rocq noninterference | Unchanged block/state assertions |
| Missing reclamation exceeds the retained envelope | — | Disabled-reclamation witness | Required TLC/Apalache unsafe counterexample and Rocq witness | Resource guardian reproducer |
| Linux/glibc RSS remains bounded in practice | — | — | Abstract platform contract only | Six-node canonical workload under the 7 GB aggregate ceiling |

## Failure interpretation

A rising RSS curve must be classified before any fix:

1. If live queues, cache byte gauges, or in-flight block counts rise, repair
   their ownership or backpressure contract.
2. If semantic evidence remains reachable, repair its exact-key retirement
   policy without relaxing finality or replay authentication.
3. If live counters plateau but per-block reclamation collapses RSS, inspect
   allocator retention and fragmentation.
4. If RSS rises even with successful completion-boundary reclamation, retain
   the failed shard and profile live allocation ownership; do not shorten
   histories, reduce proof horizons, disable consensus checks, or raise the
   resource ceiling as a substitute for root-cause repair.

This classification prevents an operational memory symptom from being
misdiagnosed as permission to alter majority voting, finalized-floor state,
cost settlement, replay equality, or merge evidence.

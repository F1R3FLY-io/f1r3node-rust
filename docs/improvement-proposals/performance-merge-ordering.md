# Prepare Merge Ordering Keys Once

Status: draft proposal. Comparator equivalence is a prerequisite for implementation acceptance.
Date: 2026-11-25.

## Objective

Remove repeated key computation from merge sorting without changing its ordering relation.
Preserve branch selection, prior-loss fairness, rejection choices, and state roots.
The [performance acceptance conditions](performance-overview.md#shared-acceptance-conditions) apply.

## Current Work

The [state-change merger](../../rspace++/src/rspace/merger/state_change_merger.rs) sorts continuation changes with `sort_by_key`.
Its key calls `hash_from_hashes` for the channel set.
The [hash helper](../../rspace++/src/rspace/hashing/stable_hash_provider.rs) sorts hash references, assembles bytes, and hashes them.
The merger later computes that history pointer again while building the action.

`sort_by_key` can evaluate its key closure repeatedly.
For data and joins, the closures clone existing hash wrappers instead of calculating a new hash.
Those wrappers contain owned bytes, so a comparison can allocate even without cryptographic work.

The [conflict-set merger](../../casper/src/rust/merging/conflict_set_merger.rs) has a second repeated-work path.
`compare_branches` collects and sorts both branches' members on each comparison.
It then compares member counts before comparing the sorted members lexicographically.
Rejection-option ties also collect and sort branch lists inside the comparator.

Numeric rejection keys are already computed once per option.
This proposal preserves the existing loss profile, cost, and size calculations.
It does not change conflict discovery, rejection-option enumeration, or their complexity.

## State-Change Keys

For each continuation change, calculate its existing history pointer once.
Keep that pointer beside the original change through sorting and action construction.
Sort by the same pointer order, with the same stable equal-key behavior.
Consume the prepared pointer when producing the action.

For data and join entries, compare existing hashes by reference.
Do not allocate an owned key on each comparison.
Keep the current hash type, byte representation, and ordering implementation.
The separate [inline-hash proposal](memory-inline-hashes.md) is not a prerequisite.

Do not deduplicate channel lists or change their hashing rules.
Hash equality is not permission to merge distinct input records.
Preserve stable order and multiplicity even in deliberately constructed equal-key fixtures.

## Branch Keys

Prepare a branch key from its member count and sorted member references.
Build keys once within the immutable merge invocation.
Use references or invocation-local indexes where their lifetimes permit it.
Do not copy complete block-index objects into keys.

The branch comparator remains:

1. Compare member counts.
2. For equal counts, compare sorted members lexicographically.

Plain lexicographic comparison of member vectors is not equivalent to count-first comparison.
A hash of a branch is also not an equivalent ordering key.
Use the existing member `Ord` relation, not debug text or hash-table iteration order.

For tied rejection options, prepare a list of branch keys in that same branch order.
Compare the lists element by element, then compare their lengths if the shared prefix is equal.
Keep this tie break after the existing loss, cost, and total-size criteria.
Do not move option-list length ahead of its elements.

A first implementation can prepare tie keys only for groups sharing the numeric keys.
Compare this against eager preparation in the benchmark.
Many options with distinct numeric priorities might not benefit from preparing every tie key.

## Lifetime and Resource Bounds

Keys belong to one merge invocation and its immutable input collections.
Drop them when selection or action construction completes.
Do not introduce a cross-block cache keyed only by branch identity.
Such a cache would need separate invalidation for changing context and prior losses.

Measure peak metadata bytes as well as saved CPU time.
Preparing every key can increase memory even when it reduces repeated allocations.
Retain existing merge admission limits and cancellation behavior.
Do not expand the unresolved DAG scope to improve benchmark utilization.

## Regression Plan

Keep the old comparators as test-only reference functions for differential tests.
Compare complete comparison results and selected outputs, not only whether a list is sorted.

| Case | Required assertion |
| --- | --- |
| Branch lengths and member values that imply different sort orders | Count-first ordering remains exact |
| Tied loss, cost, and size keys | Nested branch-list ordering selects the same rejection option |
| Different numeric priorities | Existing fairness and cost precedence remains unchanged |
| Randomized insertion order into hash sets | Equivalent inputs produce equivalent ordered outputs and decisions |
| Empty sets, shared prefixes, and equal keys | Reference ordering and stable ties are preserved |
| Equal data and continuation records | No multiplicity loss or new deduplication |
| Multi-channel continuation changes | Same history pointers, action bytes, and roots |
| Conflict recovery and duplicate-deploy histories | Same rejected deploys, effects, and validation verdicts |

Test comparator antisymmetry and transitivity in addition to reference agreement.
Retain event-indexed conflict tests and existing multiset regressions.
Required formal evidence remains governed by the existing verification policy.

## Benchmark and Decision

Count channel-key evaluations, branch-member sorts, option tie-key builds, and allocated bytes.
Measure CPU and latency for small, wide, and deeply tied merge inputs.
Include inputs with mostly distinct numeric priorities and inputs with many ties.
Report key-preparation time separately from sorting and full merge execution.

The [operation probe](performance-overview.md#reproducible-operation-probe) demonstrates repeated key evaluation with an inexpensive synthetic key.
It does not model branch semantics or the cost of `Blake2b256`.
Actual state-change and conflict-set benchmarks are required before estimating savings.

Start with state-change keys, then branch keys, as separate reviewable patches.
Reject any candidate that changes a decision or canonical output, regardless of speed.
If eager key storage raises peak memory, evaluate tie-group preparation or leave that phase unchanged.
There is no storage migration, and rollback restores the previous comparator implementation.

# Spatial-matcher candidate group masks and pressure scheduling — 2026-08-23

This scientific and engineering record supports stack-safety report §5.36, PathMap engineering
journal J-017, Consensus-Breaking Register (CBR) classification CBR-070, pgmcp bug
`spatial-matcher-candidate-mask-depth-width-retention`, and pgmcp experiment 312. The selected
implementation is `f1r3node-rust-mettail@e3f3b4831329ec8863d33af7ae0790d930efa041`.

## 1. Terms and question

| term | definition |
|---|---|
| target group | The equivalence class of target occurrences having the same authoritative canonical identity bytes. |
| occurrence | One original target-list position. A group may contain many occurrences. |
| group mask | `All`, or a `PathMap<()>` whose prefix-free eight-byte keys are stable `u64` group identifiers. |
| occurrence mask | The frozen control representation: a `PathMap<()>` containing one eight-byte target-position key per matching occurrence. |
| retention pressure | The minimum peak number of simultaneously live child results under a Sethi–Ullman-style evaluation schedule. |
| CoW | Copy on write: immutable PathMap roots share structure until a mutation requires a private root. |
| RSS | Resident set size: memory resident in physical memory. |
| PDA | Pushdown automaton represented by explicit heap frames and an iterative driver. |
| CBR | Consensus-Breaking Register: the living classification ledger for may-change-consensus work. |
| SHA | Secure Hash Algorithm; SHA-256 identifies the exact content of retained artifacts. |

The predecessor expanded every byte-identical target occurrence into every candidate-query leaf.
Its fixed postorder evaluator retained all sibling masks until the parent reduced them. For a plan
of depth $`d`$ and duplicate width $`w`$, an adversarial shape could therefore retain
$`\Theta(d w)`$ endpoints even though all $`w`$ occurrences belonged to one semantic group.

The question was whether candidate algebra could remain compressed by semantic group until the
root, while preserving the exact ordered occurrence vector consumed by the existing ListMachine.
The treatment was required to preserve conservative candidate inclusion, matcher verdict,
complete `FreeMap`, remainder, and target-position order without changing PathMap.

## 2. Selected algorithm

The compiler discovers captures in source order, then schedules only pure candidate-set operands.
For a leaf, $`P = 1`$. For an n-ary node with children sorted by descending pressure, the retained
result pressure is:

```math
P_{\mathrm{nary}} = \max\!\left(P_0,\; 1 + \max_{i>0} P_i,\; 3\right).
```

Work weight breaks unequal-pressure ties; exact ties retain source order. The evaluator's `Work`
enum remains scalar. Retained `PathMap<()>` values live in a separate LIFO accumulator so the
continuation frame size does not inherit the map representation.

**Algorithm 1 (GROUP-MASK-EVALUATE).** The prose and pseudocode intentionally name every authority
crossed by the treatment.

```pseudocode
ALGORITHM GroupMaskEvaluate(index, compiled_plan)
Input:  authoritative target Bag/catalog and a flat, source-observing candidate plan
Output: the predecessor's ascending target-position candidate vector

1. For each Query leaf, visit only matching Bag prefixes with a read zipper.
2. Insert each reached target group identifier once into a PathMap<()> group mask.
3. Treat a statically universal query as All; retain Empty and All as absorbing fast paths.
4. Evaluate AND with native PathMap meet and OR with native PathMap join.
5. Evaluate higher-pressure children first and release each consumed child mask promptly.
6. Traverse the final group mask once with a read zipper and decode each fixed-width group ID once.
7. If group positions are already ordered, concatenate directly.
8. Otherwise compare sparse sorting work with dense ordered scanning work.
9. Emit the chosen groups' original positions in ascending order.
10. Return Vec<u64> solely as the existing ListMachine random-access witness.
```

The expansion selector compares a sparse estimate $`K \log K`$ with a dense estimate
$`T + \lceil G / W_{\mathrm{word}}\rceil + M`$, where $`K`$ is the number of emitted positions,
$`T`$ is target cardinality, $`G`$ is group count, and $`M`$ is matched-group count. Sparse
expansion concatenates and sorts only when concatenation is not already ordered. Dense expansion
marks group membership in a bit vector and scans the original position-to-group sequence once.

No `Vec<Par>`, decoded shadow set/map, per-element sort, PathMap fork, recursive fallback,
`RUST_MIN_STACK`, `stacker`, or artificial traversal bound was introduced.

## 3. Immutable protocol

| item | value |
|---|---|
| source | `e3f3b4831329ec8863d33af7ae0790d930efa041` |
| control | Non-default feature `spatial-matcher-candidate-mask-differential`; copied occurrence-mask builder and postorder evaluator; source scheduling restored; universal-query canonicalization disabled. |
| treatment | Production group masks, retention-pressure schedule, PathMap join/meet, adaptive root expansion. |
| timed boundary | `ListTargetIndex` and compiled plan are prebuilt; only candidate-plan evaluation and final ordered expansion are timed. |
| samples | 51 retained Criterion samples per arm in every fixture; 13 fixtures; 1,326 observations total. |
| machine | AMD Ryzen Threadripper PRO 5975WX; 32 physical cores exposed; simultaneous multithreading disabled. |
| placement | CPU 2; `amd-pstate-epp` performance governor; boost enabled. |
| memory policy | `MemoryHigh=8G`, `MemoryMax=12G`, `MemorySwapMax=0` for Criterion; profilers used a 6 GiB hard cap and zero swap. |
| primary metric | `duplicates_width_4096_or_evaluator_latency_ns`; lower is better. |
| locked criterion | Mann–Whitney lower-tail $`p < 0.05`$, minimum absolute Cliff's delta 0.33, and at least 20% median improvement. |
| semantic guard | Complete ordered control domain equals treatment domain and the explicit fixture oracle before benchmark registration. |
| sparse guard | `distinct_sparse_width_4096` treatment/control median ratio no greater than 1.05. |

The raw observations are committed in
[`spatial-matcher-candidate-group-mask-2026-08-23.tsv`](spatial-matcher-candidate-group-mask-2026-08-23.tsv),
SHA-256 `6cdfc07467d7e9cff11594026dfd482971003a29342d39c41ba0add4a77264c8`.
Each normalized value is Criterion's aggregate elapsed nanoseconds divided by that sample's
iteration count; the aggregate elapsed value and iteration count are retained beside it.

## 4. Timing results

The table reports the median of the 51 raw normalized observations in nanoseconds per evaluation.

| fixture | occurrence control | group treatment | treatment/control |
|---|---:|---:|---:|
| distinct sparse, width 4,096 | 2,330.140 | 1,441.165 | 0.6185 |
| duplicates, width 1, AND | 19,159.776 | 18,497.642 | 0.9654 |
| duplicates, width 1, OR | 17,815.152 | 17,565.680 | 0.9860 |
| duplicates, width 16, AND | 46,098.341 | 18,552.267 | 0.4025 |
| duplicates, width 16, OR | 47,976.235 | 17,854.295 | 0.3721 |
| duplicates, width 256, AND | 305,588.033 | 19,476.122 | 0.0637 |
| duplicates, width 256, OR | 307,571.097 | 18,022.679 | 0.0586 |
| duplicates, width 4,096, AND | 5,489,111.351 | 19,759.546 | 0.003600 |
| **duplicates, width 4,096, OR (primary)** | **5,513,777.071** | **18,869.671** | **0.003422** |
| interleaved dense, width 1,024 | 304,108.268 | 85,318.932 | 0.2806 |
| opposing depth 8 / width 256, AND | 9,151,544.111 | 587,832.898 | 0.0642 |
| opposing depth 8 / width 256, OR | 9,277,837.667 | 548,836.738 | 0.0592 |
| universal before opposing depth 8 / width 4,096 | 632,892.531 | 541.166 | 0.000855 |

Pgmcp experiment 312 accepted the locked primary hypothesis with $`U = 0`$,
$`p = 1.6518407509198735 \times 10^{-18}`$, Cliff's delta $`-1.0`$, and relative median
improvement 0.9966. Both normality warnings are expected and justify the preregistered
nonparametric test. The sparse guard improved rather than regressed.

## 5. Physical profiles

### 5.1 Headless Heaptrack

Every accepted trace used `heaptrack --record-only` and text-only `heaptrack_print` analysis.

| fixture and arm | allocation calls | temporary allocations | peak heap |
|---|---:|---:|---:|
| duplicate control, 10 measured calls | 159,590 | 113,499 | 3.36 MiB |
| duplicate treatment, 10 measured calls | 30,304 | 13,931 | 1.85 MiB |
| opposing control, 5 measured calls | 197,700 | 91,347 | 3.57 MiB |
| opposing treatment, 5 measured calls | 109,006 | 35,045 | 742.19 KiB |

The duplicate control's dominant allocation paths are
`occurrence_mask_matching_control`, `WriteZipperCore::set_val`, and mutable PathMap node growth.
The treatment's corresponding paths are `group_mask_matching` and read-zipper traversal. The
opposing treatment reduces modeled peak heap by about 79.2%.

### 5.2 Paired Cachegrind

Both arms build the same duplicate-width-4,096 fixture and execute exactly three warmups, followed
by zero measured-loop calls. This isolates equal evaluator call counts and avoids sampling-volume
confounding.

| counter | control | treatment | absolute reduction |
|---|---:|---:|---:|
| instruction references | 310,627,559 | 29,198,834 | 90.60% |
| data references | 115,536,030 | 13,129,677 | 88.64% |
| L1 data misses | 548,844 | 56,222 | 89.76% |
| last-level cache misses | 73,562 | 28,274 | 61.56% |
| branches | 33,336,786 | 3,899,952 | 88.30% |
| mispredictions | 452,408 | 257,007 | 43.19% |

The treatment's percentage miss rate can appear larger because common startup becomes a larger
fraction of much less total work. The absolute counts are the relevant comparison and all decline.

### 5.3 Paired Massif

The corrected native Massif protocol uses the same equal-three-warmup shape, 30 snapshots maximum,
`time-unit=B`, and `detailed-freq=1000000`. Peak total modeled bytes fall from 3,292,888 to
1,778,424, a reduction of approximately 46.0%, consistent with Heaptrack's independent peak.

### 5.4 Headless AMD uProf

Both Hotspots profiles last 3.554 seconds and pin the application to CPU 2. The control executes
500 evaluations; the faster treatment executes 150,000 so both profiles contain a stable sample.
The control's top functions are PathMap mutation and CoW uniqueness:

| control function family | CPU share |
|---|---:|
| `node_set_val` | 18.82% |
| `make_unique` | 18.45% |
| mutable child traversal | 16.97% |
| `pjoin_dyn` | 9.59% |
| `WriteZipperCore::set_val` | 8.12% |

The treatment shifts to read-only prefix and zipper operations:

| treatment function family | CPU share |
|---|---:|
| relative structural-prefix byte preparation | 18.64% |
| `ReadZipperCore::to_next_get_val` | 10.85% |
| `positions_matching_plan` | 5.76% |

No PathMap mutation routine appears in the treatment's ten hottest functions.

## 6. Semantic, ordering, and stack gates

| gate | result |
|---|---|
| complete matcher-equivalence module, debug | 57 passed, 0 failed, 10.22 seconds |
| complete matcher-equivalence module, release | 57 passed, 0 failed, 1.00 second |
| opposing depth/width retention matrix | depth 1/4/8 × width 2/8/32 × AND/OR × encoded/resident × both source orientations; exact domains and bounded live group endpoints |
| adverse scheduler control | node-count schedule retains 10 masks/endpoints and 8 accumulators where pressure scheduling retains 4/4 and 2 at depth 8 |
| expansion differential | empty, all, single-group, ordered sparse, sort-required sparse, and dense lanes; 48 deterministic randomized/interleaved layouts forced through all three expansion modes |
| mixed duplicates | exact candidate domain, verdict, complete `FreeMap`, and remainder agree with the independent oracle |
| feature boundary | frozen control, benchmark fixture, Criterion target, and probe require one non-default feature; default production cannot execute them |
| final reviewer | no blocker in production equivalence, ordering, stack safety, control fidelity, benchmark preflight, or feature isolation |

The final ordered `Vec<u64>` is not a membership authority and is not a `Vec<Par>`. It is the
existing ListMachine's random-access position witness. Removing it belongs to a separately proved
matcher-schedule redesign, not to candidate-domain compression.

## 7. Negative and corrected protocols

Negative results are retained because they constrain future reproduction:

1. Native Massif with `detailed-freq=1` took about 590 seconds for one control arm. It was stopped
   before treatment and is excluded from every conclusion.
2. AMD uProf `assess` hardware-counter mode took about 702 seconds for one control arm. It was
   stopped before treatment and is excluded.
3. An initial Heaptrack recording omitted `--record-only`; Heaptrack opened its GUI after recording.
   No process remained, that artifact was superseded, and all accepted traces were recollected
   with `--record-only` and analyzed only through `heaptrack_print`.
4. Valgrind rejects native Massif `detailed-freq=0`; the corrected paired protocol uses the maximum
   accepted interval, 1,000,000.

## 8. Artifact integrity

Pgmcp experiment 312 artifacts 384–390 retain the Massif, Heaptrack, Criterion, Cachegrind, protocol-
audit, test, and uProf summaries with parsed metrics. Primary raw samples are also recorded in the
experiment's named metric runs.

| artifact | SHA-256 |
|---|---|
| Criterion campaign log | `b99710b95f3de06f6b9c48bf96258af18bc977489ab6e7ab2e2161d288ee1c5d` |
| debug matcher-equivalence log | `22e4b58cdc913e9e378d6f82cca233b2985a1c8fa52f37fdf32f923fc784f47a` |
| release matcher-equivalence log | `fd5b79afc166d36feb14b7704512969a0ac036cbe8c32ed472b86ed21f808482` |
| Cachegrind control log | `bc4ee8826013efaf0051e08aa25b02ca677d5e6fb307ad421d2bc744f5d99c3b` |
| Cachegrind treatment log | `2112ce956da50182552fd8ee839a8c2e1cf5118189dcd49c54594d6606e4044c` |
| Massif control output | `1e5ff82ca2e8e0e6c8dbacf433f63e8c7ed585dda87a5174f830fb869ed6c32d` |
| Massif treatment output | `f1482e5f846ba8828409f74a4ad1166f2204057bc96911ad6a67c0ef3dcd2a46` |
| uProf control CSV | `033faf0156dd993a067322751e787c77627f1f9aa99b0115610c12e72ac21061` |
| uProf treatment CSV | `4e3403eb28ec5fe6fc76e33011dc27f424b37c97faf669dbc6cfebda0675a603` |

## 9. Threats to validity

- Criterion measures the evaluator boundary, not complete RSpace rendezvous throughput. The index
  and plan are deliberately prebuilt to test the named retention mechanism.
- uProf uses unequal iteration counts to obtain equal-duration samples. Percentages explain hot-path
  movement; Criterion supplies per-evaluation latency.
- Heaptrack changes timing and RSS. It is used for allocation mechanism and modeled peak, while
  unprofiled Criterion supplies latency and paired Massif independently checks peak evolution.
- Cachegrind simulates caches. Its absolute counter differential identifies eliminated work; it is
  not a hardware-cache latency prediction.
- Whole-process RSS is dominated by common startup and fixture state: `/usr/bin/time -v` records
  approximately 24–25 MiB in both arms. Flat RSS does not contradict the lower evaluator heap peaks.
- Candidate filtering remains conservative and non-authoritative. The spatial matcher still decides
  acceptance and publication. The broader CBR-053 generated-schedule and AC-lifecycle obligations
  therefore remain `UNVERIFIED` even though this candidate representation is proven equivalent.

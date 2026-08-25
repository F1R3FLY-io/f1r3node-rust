# RSpace identity-order PathMap lifecycle experiment

Status: **secondary index rejected; one-pass zipper mechanism retained as engineering evidence**

Date: 2026-08-25

Pgmcp experiment: `rspace-identity-order-pathmap-2026-08-25` (#314)

## 1. Question and terms

The production DATA checkpoint route walks the authoritative RSpace
`PathMap<Multiplicity>`, copies each complete canonical bincode `Datum` identity into one transient
arena, sorts small borrowed-range descriptors by those complete identity bytes, and writes final
sequence framing. A **secondary identity-order index** is an additional
`PathMap<Multiplicity>` whose keys are the complete canonical identities in their required output
order. A **ready egress** measures an index that was maintained while the resident state changed. A
**cold rebuild** constructs that index from the authoritative structural trie immediately before
egress. A **copy-on-write (CoW) clone update** clones both persistent roots and changes one key.

The question was whether eliminating the descriptor sort repays the added mutation work and
retained space. The frozen production control is commit `abd915454`; the initial experimental
harness is commit `30d5e88b3`; the repaired one-pass treatment is commit `8fd6dde61`.

The experiment does **not** compare PathMap with a `Vec<Datum>` data model. Both arms retain the
same authoritative PathMap Bag. The treatment deliberately adds a second trie so its complete
lifecycle cost can be measured before any production activation.

## 2. Frozen decision rule

For mean ready-egress saving $`S`$ and incremental per-mutation maintenance cost $`M`$, the
checkpoint break-even frequency is:

```math
F = \frac{\max(0,M)}{S}.
```

The treatment could be selected only if all of the following held:

1. complete output bytes, digest, logical cardinality, and derived root observation were identical;
2. no ready-egress mean regressed by more than 5%;
3. $`F \le 1`$ in every case;
4. every ready-egress one-sided Welch result passed $`\alpha=0.01`$ with
   $`d\le-0.5`$ after Benjamini–Hochberg correction over all 36 timed cells; and
5. the retained-memory ratio was at most 1.10 in every case.

Failure of any gate selects the arena-plus-descriptor-sort control. The gate was frozen before the
treatment matrix; it was not weakened after observing the result.

## 3. Harness and protocol

The matrix contains nine shapes: shared-prefix 1/16/256/4,096, unshared 256/4,096, shared
multiplicity 2 and 16 at 16 identities, and an opposed-order 256-identity fixture whose PathMap
structural order differs from canonical identity order. Each of four operations has one semantic
check and 82 retained counterbalanced arm pairs after five warmups, for 36 checks and 5,904 timed
observations per implementation.

**Algorithm 1 (one observation pair).** Fixture construction and semantic auditing are outside the
timer.

```pseudocode
Input: fixture F, operation O, counterbalanced order AB or BA
authority := execute(F, O, arena_sort)
treatment := execute(F, O, identity_pathmap)
require treatment.bytes = authority.bytes
require treatment.digest = authority.digest
require treatment.logical_cardinality = authority.logical_cardinality
for arm in counterbalanced order do
    reset operation-scoped allocation counters
    start timer and counters
    result := execute(F, O, arm)
    stop timer and counters
    require result = authority
    emit complete observation
```

The analyzer rejects an unknown schema, case, operation, arm, pair number, row count, missing arm,
inconsistent counterbalance label, zero elapsed time, or differing semantic witness before it
computes any statistic. It reports Welch and Mann–Whitney tests, Cohen's $`d`$, Cliff's
$`\delta`$, Shapiro diagnostics, a pair-preserving 20,000-replicate 99% bootstrap interval, exact
allocation counters, lifecycle frontiers, and the frozen acceptance gates. Benjamini–Hochberg
adjustment covers all 36 timed cells.

The tabular protocol writes `-` for a deliberately absent digest and `NA` for an inapplicable
derived statistic. These explicit sentinels keep every row schema-complete and distinguish absence
from truncation without relying on trailing delimiters; they do not alter an observation or a
computed result.

The matrix ran on an AMD Ryzen Threadripper PRO 5975WX with 32 physical cores and no simultaneous
multithreading. Admission observed more than two idle physical cores; the subject was pinned to one
idle core with the performance governor and boost enabled. The systemd user scope used a 12 GiB
high watermark, a 16 GiB hard limit, and zero swap.

A separate exact release gate runs ready egress, cold rebuild, insert/remove, CoW clone-update, and
fixture destruction for shared, unshared, and opposed 4,096-identity shapes on a 128 KiB thread
stack. It passes 1/1 without a depth limit, `RUST_MIN_STACK`, `stacker`, or recursive fallback. The
captured stdout/stderr SHA-256 values are respectively
`94aba928be409fe026f7bf32b8b3a5e4b1c0038b291ed47156073ea9afcd3968` and
`4bc17736a7363995b187407641a5ff96f0f640e55bb2636b00899e28cac3b585`.

## 4. First result: two trie walks are a performance defect

The first treatment visited the identity-order trie once to validate/count endpoints and again to
emit bytes. It is rejected independently of retained memory. At shared 4,096 it was 8.60% slower
than the arena-sort control; at unshared 4,096 it was 4.70% slower. Wide cells therefore had no
positive break-even frequency, and shared multiplicity 2 required 3.13 checkpoints per mutation.

This negative result identified a concrete defect rather than disproving identity order. The
repair maintains the exact framed output length transactionally with multiplicity. Trusted egress
then reserves once and performs one read-zipper traversal:

```pseudocode
on insert(identity, multiplicity):
    next_length := checked_add(length, multiplicity * (8 + bytes(identity)))
    insert_many(identity, multiplicity)
    length := next_length

on ready_egress:
    reserve_exact(length)
    write occurrence count
    visit identity trie once in canonical order
    for each (identity, multiplicity):
        repeat multiplicity times:
            write identity length
            write identity bytes
```

The preflight remains fail-before-publication: checked length changes are committed only after the
corresponding Bag mutation succeeds, and removal subtracts only an existing occurrence.

## 5. Repaired one-pass result

All nine ready-egress cells improve, with treatment/control mean ratios from 0.691 to 0.960. Every
corrected Welch result passes; the largest ready-egress corrected q-value is
$`1.044\times10^{-15}`$, and the least favorable Cohen effect is $`d=-1.412`$. Maintenance
amortizes in fewer than one checkpoint per mutation everywhere.

| Case | Ready-egress ratio | Break-even checkpoints/mutation | One checkpoint + insert/remove ratio | Cold rebuild ratio |
|---|---:|---:|---:|---:|
| shared 1, multiplicity 1 | 0.691 | 0.638 | 1.035 | 1.041 |
| shared 16, multiplicity 1 | 0.738 | 0.248 | 0.904 | 1.783 |
| shared 16, multiplicity 2 | 0.801 | 0.207 | 0.908 | 1.553 |
| shared 16, multiplicity 16 | 0.960 | 0.248 | 0.981 | 1.117 |
| shared 256, multiplicity 1 | 0.802 | 0.0276 | 0.819 | 2.214 |
| shared 4,096, multiplicity 1 | 0.813 | 0.00197 | 0.814 | 1.986 |
| unshared 256, multiplicity 1 | 0.784 | 0.0202 | 0.798 | 2.418 |
| unshared 4,096, multiplicity 1 | 0.765 | 0.00125 | 0.766 | 2.240 |
| opposed 256, multiplicity 1 | 0.670 | 0.0219 | 0.690 | 1.946 |

The cold-rebuild column is intentionally adverse: sorting a transient descriptor view is cheaper
than constructing a whole second trie and then walking it. The secondary representation only has a
time advantage when maintained as resident state.

At shared 4,096, ready egress falls from 995,252 to 809,061 ns on average. Operation-scoped
allocation calls fall from 39 to 10 and requested bytes from 2,234,662 to 701,834. The exact 29-call
delta is independently visible in Heaptrack below.

## 6. Retained-memory rejection

The custom allocator enables counting immediately before deterministic fixture construction and
samples net live requested bytes while the complete fixture is still reachable. This isolates
retained fixture storage from process startup and temporary construction work. The secondary index
fails the 1.10 ceiling in every case: ratios range from 1.672 to 2.094. Representative exact values
are:

| Case | Arena-sort retained bytes | Secondary-index retained bytes | Ratio |
|---|---:|---:|---:|
| shared 1 | 640 | 1,340 | 2.094 |
| shared 256 | 78,784 | 140,348 | 1.781 |
| shared 4,096 | 1,253,632 | 2,230,780 | 1.779 |
| unshared 4,096 | 1,741,440 | 3,030,332 | 1.740 |
| opposed 256 | 159,040 | 265,852 | 1.672 |

Multiplicity changes logical occurrences but not distinct retained endpoints, so shared 16 at
multiplicity 1, 2, and 16 has the same 1.816 ratio. This is the intended Bag compression property;
there is no occurrence-expanded retained vector in either arm.

Headless Heaptrack used `--record-only` followed only by `heaptrack_print`; no graphical process
was opened. For shared 4,096 fixture construction, control/treatment peak heap is 4.18/4.68 MiB and
allocation calls are 141,479/169,975. For 20 ready-egress operations after fixture construction,
both arms peak at 4.68 MiB while allocation calls are 170,755/170,175. The 580-call difference is
exactly 29 calls per operation, agreeing with the operation-scoped counter. Heaptrack perturbs
timing and is used for causal allocation attribution, not as the latency population.

The profiled executable SHA-256 is
`39ec99c9d72aff0e3b3db05793f0b41d05261b6800c0c4ab252b41265c32613a`.
Trace/report integrity coordinates are:

| Profile | Trace SHA-256 | Text-report SHA-256 |
|---|---|---|
| retained control | `ac2535a0ce6c72117385b273f7b8699cbede73dec113385b6f58b175ff9b58cb` | `cf4723dea24430eb37436097ac4d2e1968198c4b370b43c055a8552ecc82b92c` |
| retained treatment | `7be5b68e223f082cf644885ed4d023205ada46265176b57bdcac822895b0ca40` | `e2573200e68b980c8c684ae7ab5961977a66454acbcd8f848a0a1e54bb9e041f` |
| egress control | `20dd22e37dea0c62dd37f33516169b44cd3eace57aad2054251b02ded23f8446` | `b7d12115539b39da1d4562a53304b9b0169c74bfdaf3086e4e686642a522e590` |
| egress treatment | `b5bf04b337c400bc5de42c58fbf490eff2852bd07eb50e3abc9dc9b50855710a` | `14140bc9e343781401dc8a23bb123e49ec51a5afaae57cf9a0d45a5d575da135` |

## 7. Hardware-counter attribution

Five `perf stat` repetitions measured 1,000 shared-4,096 egress operations. Events were user-space
only and multiplexed at approximately 83%, so ratios and direction are claimed rather than
single-counter exactness. The one-pass treatment reduces cycles by 17.87%, instructions by 18.80%,
branches by 56.09%, branch misses by 75.61%, cache references by 53.21%, and cache misses by 59.35%.
The complete means and run-relative standard deviations are in
[`rspace-identity-order-index-hardware-counters-2026-08-25.tsv`](rspace-identity-order-index-hardware-counters-2026-08-25.tsv).

AMD uProf 5.3's headless `assess` profile independently attributes the same direction: sampled
user-space cycles, instructions, branches, and L2 requests are all lower for the one-pass zipper
arm. The hottest shared functions are BLAKE2b compression and PathMap read-zipper advancement; the
control additionally exposes descriptor sorting and canonical-datum range collection. Operating-
system samples are excluded because `perf_event_paranoid` was at least 2. The raw reports are
[`control`](rspace-identity-order-index-uprof-control-2026-08-25.csv) and
[`treatment`](rspace-identity-order-index-uprof-treatment-2026-08-25.csv).

## 8. Decision and reusable PathMap rule

**Reject the duplicated secondary identity-order PathMap.** It passes byte identity, statistical
latency, effect size, 5% narrow-cell, and checkpoint/mutation break-even gates, but fails the frozen
retained-memory gate by a large margin. Production therefore keeps the J-023 transient
arena-plus-descriptor-sort route. No production authority, checkpoint byte, hash, root, RSpace
semantics, persistence format, PathMap operation, or consensus result changes.

The experiment establishes two positive engineering rules:

1. when trie visitation order already equals destination order, maintain exact encoded length with
   the transaction and emit with one zipper pass—never validate by replaying the trie immediately
   before a trusted write; and
2. do not pay for a second persistent trie solely to remove an egress sort. A future experiment may
   test a **non-duplicating primary layout** whose prefix-free channel frame is followed by complete
   canonical identity. It must preserve channel-prefix operations, exact-key mutation, matching,
   replay, checkpoint ordering, and memory before replacing the current primary layout.

The latter is a distinct representation hypothesis, not permission to activate this rejected
secondary index or to erase this negative result.

## 9. Durable evidence

| Evidence | SHA-256 |
|---|---|
| [two-pass raw matrix](rspace-identity-order-index-two-pass-raw-2026-08-25.tsv) | `406b9526fdba0a4235f2b86abb62bb0c5a92e430e5d5f0eb0bc5f8600ca7b5a9` |
| [two-pass analysis](rspace-identity-order-index-two-pass-analysis-2026-08-25.json) | `68e5e42a095348d99ffdc0e0af3ded0a4d43e090655d22be273416d2688546a2` |
| [two-pass summary](rspace-identity-order-index-two-pass-summary-2026-08-25.tsv) | `e377879808f9b7812b0f63ccf49e373f1a604856ee094d930ad719dd4b647cca` |
| [one-pass raw matrix](rspace-identity-order-index-one-pass-raw-2026-08-25.tsv) | `3398c93fac972b4572de445c6b7f51472ab6b112a57d3eaf65ea469cb1692c90` |
| [one-pass analysis](rspace-identity-order-index-one-pass-analysis-2026-08-25.json) | `bccfa2b19b220863865cd1a045a6817ab7ba9ecd2c38e7a81060970875c203b8` |
| [one-pass summary](rspace-identity-order-index-one-pass-summary-2026-08-25.tsv) | `bec689be82ed522633d9008f3c95aa9813974c5ee02ae06de7b2711ac4937323` |
| [retained profiles](rspace-identity-order-index-retained-2026-08-25.tsv) | `33a8d47a27d4b150c60c1eed98fdfba7afd3ae1acbaacff15bf2d464c4aedafe` |
| [hardware counters](rspace-identity-order-index-hardware-counters-2026-08-25.tsv) | `40f05e0d7948a54eff852a5a50b3a720fa4152471fbd62dd66df8c36cb7b5599` |
| [uProf control](rspace-identity-order-index-uprof-control-2026-08-25.csv) | `ad9545e19ae95655cd1c871cc591d490e8ea65b2624085b2d3a95259e9c5f33f` |
| [uProf treatment](rspace-identity-order-index-uprof-treatment-2026-08-25.csv) | `45547431f2001a5b696470717c09fde441473ea419e77b8e4fdd1c0d855b2f1c` |

The one-pass timing binary SHA-256 is
`ea2b385630774dd166dcdb305dc7beb61668dbbbead8cb03093be64ad24996bf`.
The final self-describing retained-profile binary SHA-256 is
`ade3e8a4e5afde744c1361ba5351073dd271499baf1836ad1565f50de5b98593`.

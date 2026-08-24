# Atomic accepted-match transaction: controlled resource and latency report

Status: measured on 2026-08-24; the transaction-level performance hypothesis is accepted with
explicit neutral and adverse cells retained below. This report measures the correctness repair in
`HotStore::apply_accepted_match`; it does not claim that the broader RSpace layout campaign is
complete.

## 1. Terms and question

An **accepted match** is a selected RSpace rendezvous whose datum multiplicities, continuation,
epoch, and join metadata are ready to become visible. The **atomic arm** publishes that state with
one `HotStore::apply_accepted_match` call. The **frozen split arm** is a test-source reproduction of
the pre-atomic git-history sequence: optional continuation removal, one datum-delta application,
one join removal per selected candidate, and a separate datum insertion when an arrival remains
unconsumed.

A **root publication** is one successful replacement of the authoritative
`PathMap<Multiplicity>` Bag root. A **temporary allocation** is Heaptrack's classification of an
allocation released shortly after its creation. A confidence interval (CI) is Criterion's 99%
interval for an arm's estimated execution time. Resident set size (RSS) is the physical memory
resident for the measured process. Secure Hash Algorithm 256-bit (SHA-256) digests identify exact
evidence files and binaries without treating a filename as provenance.

The preregistered decision question was: does one validated publication preserve the exact
post-state while keeping the singleton and two-channel join conservative cross-CI ratio below
1.05 and reducing work as the accepted transition touches more channels or repeated identities?

## 2. Experimental design

The harness is non-default and has two deliberately separate modes:

1. `accepted-match-transition-experiment` runs the operation without counter or allocator-wrapper
   overhead. Criterion and fixed percentile/RSS/profiler subjects use this mode.
2. `accepted-match-transition-instrumentation` adds transition-scoped allocation, write-lock, and
   PathMap-root counters. Those counters establish mechanism and are never mixed into the latency
   population.

Every comparable control/treatment pair begins from the same deterministic fixture and must emit
the same post-state digest. Stale rejection has no split comparator because the predecessor did not
have a freshness check; its before and after digests must instead be equal. The matrix covers:

- singleton and multiplicities 2 and 16;
- joins over 2, 8, and 32 channels;
- persistent continuation, stale ticket, and named nonleast continuation;
- an accepted match that must retain the unconsumed arriving datum.

**Algorithm 1 (Controlled accepted-match observation).** The algorithm excludes fixture
construction and digest materialization from the timed transition while preserving both for
anti-vacuity.

```pseudocode
Input: deterministic fixture F, arm A, expected status S
before := digest(F.store)
start transition timer and transition-scoped counters
if A is atomic then
    outcome := F.store.apply_accepted_match(F.ticket, F.delta)
else
    outcome := frozen_split_sequence(F.ticket, F.delta)
stop transition timer and transition-scoped counters
after := digest(F.store)
require outcome.status = S
if S is committed then require after = F.expected_digest
if S is stale then require after = before
emit timing, counters, and after
```

The frozen split procedure above is the source-compatible sequence listed in §1; it is never an
alternative semantic model. The digest requirements make an empty or cheaper surrogate arm fail
before its measurement can enter the population.

Criterion used 51 samples, 99% confidence, a 0.01 significance level, 3 seconds of warm-up, and 10
seconds of measurement per arm. Fixed percentiles use 10,000 operations per cell with alternating
arm order. RSS uses 50,000 operations in a fresh process per arm. All runs were pinned to one
physical core under a systemd user scope with a hard resident-memory limit and zero swap. Host
admission observed approximately 85% aggregate idle time on a 32-physical-core machine.

The frozen control is evidence infrastructure, not production code. Ordinary builds contain
neither it nor the counters.

## 3. Semantic and structural gates

The focused release suite passes both registered gates:

- every comparable arm has the same post-state digest and stale rejection is nonmutating;
- every accepted atomic operation acquires exactly one hot-state write lock and publishes exactly
  one PathMap root, while stale rejection publishes none.

The split control acquires 3, 4, 18, 4, 10, 34, 2, 3, and 4 write locks respectively for singleton,
repeated-2, repeated-16, join-2, join-8, join-32, persistent, named-nonleast, and unconsumed-arrival
cells. Its unconsumed-arrival cell also publishes two roots. These exact counts explain why the
atomic arm's advantage grows with transition width; they are not timing estimates.

## 4. Criterion result

Times are Criterion's 99% CI in microseconds. The conservative ratio divides the atomic upper bound
by the split lower bound; it is deliberately more pessimistic than a paired ratio estimator.

| Cell | Frozen split 99% CI | Atomic 99% CI | Conservative ratio | Verdict |
|---|---:|---:|---:|---|
| singleton | 2.0501–2.1223 | 2.1251–2.1407 | 1.0442 | passes the 1.05 gate; small adverse point estimate |
| repeated 2 | 2.5873–2.6294 | 2.5922–2.6306 | 1.0167 | neutral |
| repeated 16 | 8.4383–8.4692 | 6.0909–6.1725 | 0.7315 | atomic win |
| join 2 | 3.5575–3.6261 | 3.6006–3.6824 | 1.0351 | passes the 1.05 gate; intervals overlap |
| join 8 | 12.218–12.397 | 11.759–12.066 | 0.9876 | modest atomic win |
| join 32 | 64.820–65.786 | 55.952–59.312 | 0.9149 | atomic win |
| persistent continuation | 1.9018–1.9264 | 1.9666–2.0273 | 1.0660 | adverse narrow cell retained |
| stale rejection | n/a | 0.83970–0.88711 | n/a | fast nonmutating refusal |
| named nonleast | 2.1811–2.2275 | 2.1884–2.2542 | 1.0335 | neutral/slightly adverse |
| unconsumed arrival | 3.4042–3.5157 | 3.0465–3.1512 | 0.9257 | atomic win |

The fixed uninstrumented p50/p95/p99 matrix independently agrees on the scaling direction. In
nanoseconds, repeated-16 falls from 7,524/7,724/8,316 to 5,000/5,189/5,350; join-32 falls from
57,619/62,658/64,422 to 47,299/50,646/54,093; and unconsumed arrival falls from
2,905/3,036/3,176 to 2,655/2,836/2,986. Singleton changes from 1,763/1,804/1,863 to
1,814/1,904/1,964, so its fixed-run p99 is 5.4% higher even though the preregistered Criterion 99%
cross-CI gate passes. The report retains both observations.

After the task-frozen 51-sample matrix was complete, pgmcp experiment 313 opened a structured
ledger for the already-preregistered work and prescribed at least 82 observations per arm for its
additional join-32 Welch guard. One complete 51-sample join-32 replication—both arms, with no
discarded observations—raises the ledger to 102 observations per arm. Across both runs, mean split
and atomic times are approximately 64,401 and 56,169 ns/op. The locked lower-is-better test accepts
with $`p=4.0138\times10^{-68}`$, Cohen's $`d=-4.2261`$, and a 95% treatment-minus-control CI of
$`[-8{,}770.689,-7{,}693.311]`$ ns/op. Both arms fail the normality diagnostic; the parallel
Mann–Whitney result also accepts with Cliff's $`\delta=-0.980392`$. This powered replication is
supporting primary-cell evidence. It does not replace, rerun, or select among the original ten-cell
results.

## 5. Allocation and memory mechanism

Transition-scoped instrumentation reports allocation calls per operation as follows. Byte volume is
mixed—atomic singleton allocates 46 more bytes per operation despite equal call count—so the result
is a call-churn improvement, not a universal allocated-byte improvement.

| Cell | Frozen split calls/op | Atomic calls/op | Difference |
|---|---:|---:|---:|
| singleton | 37 | 37 | 0 |
| repeated 2 | 41 | 40 | -1 |
| repeated 16 | 83 | 68 | -15 |
| join 2 | 47 | 46 | -1 |
| join 8 | 98 | 91 | -7 |
| join 32 | approximately 302.48 | approximately 271.45 | approximately -31.03 |
| persistent continuation | 35 | 33 | -2 |
| stale rejection | n/a | 0 | n/a |
| named nonleast | 41 | 37 | -4 |
| unconsumed arrival | 62 | 51 | -11 |

Fresh-process unprofiled peak RSS is 4,088–4,216 KiB in every 50,000-operation arm. Differences are
at the process noise floor; no peak-RSS win is claimed.

Headless Heaptrack captures used `heaptrack --record-only`; reports were rendered only with
`heaptrack_print`. No graphical analyzer was used. The direct-debuggee profiles corroborate the
transition counters:

| Cell | Frozen split allocation calls | Atomic allocation calls | Split/atomic peak heap | Interpretation |
|---|---:|---:|---:|---|
| singleton, 1,000 operations | 157,330 | 157,330 | 86.30/86.41 KiB | equal total calls; atomic has 2,000 fewer temporary allocations |
| join 32, 1,000 operations | 4,143,894 | 4,113,132 | 180.35/180.67 KiB | atomic has 30,762 fewer calls and 32,001 fewer temporaries |
| unconsumed arrival, 1,000 operations | 223,451 | 212,451 | 87.30/87.43 KiB | atomic has 11,000 fewer calls and 5,000 fewer temporaries |

Heaptrack totals include deterministic fixture construction and outcome handling. Agreement with
the transition-scoped counter—approximately 31 fewer calls per join-32 operation and exactly 11
fewer per arrival operation—is the causal attribution. Heap peaks are equal at the displayed scale.
The stale direct-debuggee process records 190,259 total calls because its fixture is recreated and
dropped on every iteration; the transition-scoped counter is the relevant boundary and reports zero
allocations for the rejection itself.

## 6. Instruction, branch, and cache evidence

Eleven `perf stat` repetitions measured fixed-run whole-process counters. At join-32, atomic averages
8.818 billion cycles, 25.134 billion instructions, and 4.909 billion branches versus split's 9.212,
26.907, and 5.211 billion: reductions of approximately 4.3%, 6.6%, and 5.8%. Atomic records more
cache references and misses in that cell, so this is not a universal cache-locality win. At the
unconsumed-arrival cell, atomic also uses fewer cycles, instructions, branches, branch misses, cache
references, and cache misses.

AMD uProf 5.3's headless `assess` profile corroborates join-32 with approximately 3.9% fewer sampled
cycles, 6.4% fewer instructions, 5.6% fewer branches, 5.0% fewer L1 accesses, 3.4% fewer local L1
refills, and 0.7% fewer L2 accesses. Because `perf_event_paranoid` prevented operating-system
sampling, only user-space samples are claimed. Visible hotspots include Blake2 compression,
allocation/free, PathMap read-zipper navigation and copy-on-write uniqueness, datum-prefix
construction, bincode datum serialization, vector clone, and SipHash. These are whole-run profiles,
not transition-exclusive percentages.

## 7. Decision and PathMap engineering rule

Accept the atomic transaction as both the required stale-candidate correctness repair and the
selected accepted-match publication design. It passes the preregistered singleton and join-2
five-percent guards, becomes materially faster and lower-churn for repeated multiplicity, wide
joins, and unconsumed-arrival publication, and makes the stale refusal nonmutating with no
transition allocation or root publication.

Do not optimize the singleton by splitting publication again. That would trade a narrow 4–5%
latency effect for loss of the one-lock/one-root linearization invariant. Future work should instead
target the shared key preparation, hashing, allocator, and PathMap zipper/copy-on-write hotspots
without changing the transaction boundary. Keep the persistent-continuation and named-nonleast
negative/neutral cells in every future comparison.

The reusable PathMap rule is: prepare one normalized exact-key delta, let it own one private
copy-on-write root, validate all non-trie authority before publication, and publish the root once.
Repeated exact identities remain one `PathMap<Multiplicity>` endpoint; no operation projects the
Bag to `Vec<Par>`, occurrence-expands it, or serializes an ACT image.

## 8. Reproduction and evidence ledger

The immutable harness checkpoint is `281c1698a89d0a13cca010f6ba82aad459a554ed`. The Criterion,
uninstrumented fixed-run, and instrumented fixed-run binary SHA-256 digests are respectively
`82e4f0257f6265fd184875d7273e6a582d6e3240681e92db9b11f176eadddd54`,
`27ba8b218ed0bad1a2bc16f6ef1cb19e8dd678e6fd0104a6a885d93c3532666f`, and
`7c7e247e08b9acdaaf95d578c7d83874f2543e91459ad795b513cabd981c2d56`.

Primary raw-log SHA-256 digests:

| Evidence | SHA-256 |
|---|---|
| Criterion matrix | `c9b5a2fba2c8371ceb1abb6c9ecf9f2a41ead6655990bda34c30ae3cf08ab1b1` |
| powered join-32 replication | `a52db9598f32f147c8721e1adc9f33f0591888a8279e2563401ef6c1ada97a84` |
| fixed percentiles | `903a809fa4a1f4bdeb5aa34562b00f3725cdff1220c3b63c9f2400d1f7b8f6aa` |
| transition resources | `6d649a2feaac48d2e812fb6522cf2d1517090626f616308bb17ca4ac9d2cf136` |
| RSS command log | `eef6e0fb6dba1d59cc826991f989e3b2e41b5833bfb95be2b71741993a9a9b54` |
| RSS result matrix | `a85b4f3cd2006e32bcc4fad9b7e99d7ac77b862991fd5c5d353e92b587ba18e4` |
| eleven-repetition hardware counters | `026320e81bb2dd4b5009b4b08fe2363699d2522a9d9335ae7b6a166a1a29853a` |
| uProf join-32 atomic/split reports | `4b4f11a7f5b1587ce626cc4c14fa879edd1c2e31b5e126fe3e3175dd415efe10` / `06b79f47fd87db38f3d3df2c554d9cc77dccf7f397c667cd15b6fe0053317703` |
| Heaptrack join-32 atomic/split text reports | `056eee4a2c78e5d338193ed3b3a8d25f37945bd814a0c916ad8f17ec45201a7d` / `ea702e5110b4c4e23af75a7c2c24ea46742bed9cf36e29d65594e030d2ea09e4` |
| Heaptrack arrival atomic/split text reports | `42c9d0c892d9e4e7ffd391d09f0f638eba4b266229d630e66c2d13428b26459b` / `184d810250627b46cbb25aef4bab75e5202946b6e9a9204d5c5c745462baced5` |
| Heaptrack singleton atomic/split text reports | `165d16428400e75331f35caa22cf36c81631dc5d3886cd3e454c232e4b3e5bc5` / `37cea45dbd2410b83873320b4608878d8a4f61a2e575d448ab9a04352c32a772` |

The final focused semantic test log has SHA-256
`03854adae08db5b326cc79ae44e1ca0fd40b1a425f9dd25d2cd360dc6e43f26b`; the final default
all-target compile log has SHA-256
`2461b00c8697c4189b2de92ca6fb05b5d72273c78a0a47b1ce4e83ccca077b70`. The structured record is
pgmcp experiment 313, `rspace-accepted-match-atomic-transition`, linked to
`rspace-atomic-versioned-accepted-match` without an auto-verification criterion.

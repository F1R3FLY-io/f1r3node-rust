# Final-root construction and codec phase attribution

## Question and experimental boundary

Experiment 345, linked to campaign 4131 tasks 6444 and 5834, asks which operations still
account for depth-growing time and allocation after the final-root fixture migrations.
The implementation coordinate is `fefd234ce`. This is a diagnostic experiment, not a
comparison with `dev`, a production-throughput verdict, or whole-campaign acceptance.

RPB3 is the canonical representation of a ParBag component key, containing its score and
protobuf body. EPM1 is the direct EPathMap snapshot envelope; ACT is PathMap's compact
trie image. The three shapes are an ordinary unary list chain, an escaped set containing
that chain, and a fixed-key nested map-value chain. The last two are distinct: set keys
grow with depth, whereas the outer map's key topology remains fixed as its value grows.

The pre-registered prediction is that final-root construction scales with final encoded
size rather than repeatedly constructing every resident suffix. Separately measured
operations may retain other slopes. Frozen resident builders are controls through depth
128 only; that experimental domain does not reduce any production or correctness limit.

## Method and reproducibility

The first complete ladder contains 78 shape/phase/depth cells at depths 64 and 128.
Each has one pilot, three warmups and seven retained observations, with fresh inputs.
Warm digest/snapshot observations batch 1,000 accesses after explicit cache priming;
other phases execute once per observation. The four meter/ownership controls pass.
See engineering journal [J-171](../../pathmap/pathmap-engineering-journal-2026-08-23.md#j-171--2026-09-17-final-root-phase-measurement-boundaries)
for the complete phase boundaries and independent review.

The release test binary is invoked directly with these selectors, inside the capped service:

```bash
taskset -c 3 env FIXTURE_SHAPE=escaped FIXTURE_PHASE=outer_map_construct \
  FIXTURE_DEPTH=128 FIXTURE_BATCH=1 FIXTURE_SAMPLES=7 FIXTURE_WARMUPS=3 \
  target/release/deps/epathmap_epm1_snapshot-e8b05c4998cd0695 \
  --exact fixture_phase_probe::fixture_phase_measurement --ignored --nocapture
```

The service uses `MemoryMax=2G`, `MemoryHigh=1536M`, `MemorySwapMax=0`, `CPUQuota=100%`,
and a 120-second deadline per cell. The immediate three-second `mpstat` check reports
99.33% idle on CPU 3 and 85.50% across the 32 physical cores. No owned build or test
overlaps this ladder. Total service time is 113.054 seconds, CPU time 109.535 seconds,
peak cgroup memory 6.8 MiB, and swap zero. These are whole-service observations.

[Retained observations](final-root-fixture-phase-r2-2026-09-17.tsv) preserve integer
batch totals, sample IDs and input/output sizes. pgmcp artifact 677 preserves numerical
observations; 234 metric groups contain allocation calls, requested bytes and instrumented
nanoseconds. An optional-dimensions schema mismatch was avoided by retaining dimensions
in each command specification and the artifact; no pgmcp implementation was modified.

## First measured results

Times are medians per operation, in milliseconds. Requested bytes count cumulative
successful allocation requests, including replacements during reallocations. They are
**not** peak live memory, retained bytes, or process RSS.

| Shape and phase | Time, depth 64 | Time, depth 128 | Requested bytes, depth 64 | Requested bytes, depth 128 |
|---|---:|---:|---:|---:|
| Unary final-root construction | 1.017 | 1.984 | 368,687 | 726,767 |
| Unary frozen resident construction | 62.681 | 251.351 | 22,360,342 | 89,026,582 |
| Escaped inner construction | 3.101 | 6.117 | 1,148,744 | 2,268,104 |
| Escaped outer EPathMap construction | 67.088 | 264.489 | 23,439,533 | 91,187,245 |
| Escaped legacy protobuf decoding | 133.323 | 525.904 | 46,920,684 | 182,438,444 |
| Map final-root construction | 2.178 | 4.377 | 1,334,156 | 2,612,108 |
| Map frozen resident construction | 125.176 | 501.078 | 45,321,669 | 178,645,861 |
| Map canonical-key decoding | 375.589 | 1,548.247 | 260,136,433 | 1,041,415,537 |
| Map lookup/decode descent | 179.922 | 740.130 | 126,491,327 | 513,551,647 |
| Map EPM1 decoding | 3.808 | 7.996 | 3,165,150 | 6,359,454 |

**MEASURED:** the new constructors approach a twofold allocation increase per depth
doubling; the frozen controls and four remaining operations above approach fourfold.
Two sizes are a diagnostic, not an asymptotic proof. The subsequent depth-256 extension
and larger constructor rungs below strengthen the attribution without closing the experiment.

Warm ParBag digests and warm snapshots request zero allocations at both sizes. The
map's uncached key-topology digest requests exactly 1,384 bytes at both sizes, consistent
with excluding nested values. These facts do not establish nanosecond production latency:
the timing meter and cache-state witness are instrumented, and tiny batches remain
sensitive to measurement overhead.

### Third rung and complete constructor ladder

All 36 non-legacy-control cells at depth 256 completed with seven retained observations
each; [the observations are preserved separately](final-root-fixture-phase-r3-2026-09-17.tsv).
The service took 298.105 seconds, 293.694 seconds CPU, reported 5.8 MiB peak cgroup memory
and zero swap. The immediate idle fractions were 92.83% on CPU 3 and 71.82% machine-wide.
The four problematic operations retain the same mechanism across a third rung:

| Phase at depth 256 | Median time (ms) | Requested bytes |
|---|---:|---:|
| Escaped outer EPathMap construction | 1,035.101 | 359,925,677 |
| Escaped legacy protobuf decoding | 2,105.900 | 719,959,788 |
| Map canonical-key decoding | 6,293.270 | 4,167,875,761 |
| Map lookup/decode descent | 3,114.196 | 2,069,623,263 |

The three repaired constructors also completed depths 512, 1024, 2048 and 4096, retaining
seven observations per cell. The [additional 12 cells](final-root-fixture-phase-r4-2026-09-17.tsv)
took 16.861 seconds / 13.794 seconds CPU in the capped service, with 19.1 MiB reported
peak cgroup memory and zero swap. Preflight idle fractions were 94.16% on CPU 3 and 76.58%
machine-wide. All 126 cells across the three ladders passed; no cell was censored or silently
dropped. Higher-depth non-construction phases are not counted in this initial ladder;
their successor run and stopping outcomes appear in the extended operation section below.

An ordinary least-squares fit of log median cost against log depth across all seven sizes
gives the following descriptive exponents. An exponent near one indicates roughly proportional
growth over this measured range; it is not an asymptotic proof or an inferential confidence bound.

| Constructor | Time exponent versus depth | Requested-byte exponent versus depth | Time at depth 4096 (ms) | Requested bytes at depth 4096 |
|---|---:|---:|---:|---:|
| Unary | 1.0000 | 1.0097 | 64.882 | 24,526,676 |
| Escaped inner | 1.0026 | 1.0095 | 201.141 | 76,353,895 |
| Fixed-key map | 1.0139 | 1.0049 | 147.681 | 86,122,928 |

The time exponents against final encoded output size are 0.9538, 0.9594 and 0.9936;
requested-byte exponents are 0.9631, 0.9660 and 0.9848. Protobuf length-prefix boundaries
make encoded size non-exactly proportional to depth. These results support the final-root
construction hypothesis over the measured range, not removal of the independent decoder costs.
pgmcp now retains 378 metric groups and artifacts 677, 679 and 680 for these three ladders.

## Attribution: source inspection and headless profiles

The outer-construction path is:

```text
EPathMap::new -> EntryTrie insertion -> canonical key encoding
  -> split_carrier_list -> Par::first<Expr> -> decode_exact
  -> generated protobuf attachments -> resident ParBag insertion/destruction
```

`split_carrier_list` decodes the sole expression before checking whether it is an EList.
For an ESet it reconstructs the deep expression, rejects it as a list, and discards it;
the escape path subsequently uses the original canonical protobuf bytes. Generated
legacy EPathMap tag-1 admission independently reconstructs nested Pars before insertion.
These are representation-construction costs, not evidence of slow native trie lookup.

AMD uProf time-based profiles cover depths 64 and 128. The initial report exposes hot
zipper advancement/pruning, shared-reference counting and guarded root destruction.
It reports affinity `0-31` despite the outer `taskset`; therefore it is call-attribution
evidence only, not a verified pinned timing observation. The direct measurement ladder
does not launch through uProf. Frame-pointer sampling may omit optimized ancestors.
The corrected r2 profile additionally specifies `--affinity 3 --call-graph-mode fpo
--call-graph-size 4096`; its report confirms affinity `3` and frame-pointer-omission
unwinding enabled. Its collection/report service completed in 12.353 seconds, 7.365 seconds
CPU, 308.5 MiB peak and zero swap. This strengthens profile reproducibility, not a latency claim.

Heaptrack ran only with `--record-only`; all analysis used `heaptrack_print`. At each
depth it captured a pilot and one retained subject, plus a separate preparation-only
control. At depth 128 its allocation stacks explicitly connect `split_carrier_list`,
`decode_exact`, `attach_par_exprs`, ParBag insertion and guarded PathMap teardown. One
such line-list split stack accounts for 513,975 allocation calls. Whole subject processes
record 580,151 allocation calls at depth 64 and 2,210,444 at depth 128, with reported
peak heaps 361.21 KB and 636.66 KB. The depth-128 preparation-only process records 67,360
calls and no split-classifier stack among the printed top allocators. Do not subtract
these process totals as pure subject costs: verification and output destruction differ.
Heaptrack reports 944 bytes not released at exit in each displayed run; this observation
is not attributed to EPathMap without a separate allocation-stack analysis.

The profile/report services completed under their caps: uProf collection 61.2 MiB peak;
Heaptrack collection plus uProf reporting 290.5 MiB; Heaptrack text reporting 54 MiB.
All report zero swap. No GUI, PathMap fork, enlarged stack, or production edit was used.

## Initial decision and remaining obligations

At the pre-repair checkpoint, the decision was to retain the qualified fixture migrations.
The following obligations describe that historical boundary; successor evidence follows.
Do not close task 6444 or claim that all slopes
are fixed. The prerequisite before the subsequently implemented repair was to prove that a resident encoded-Expr discriminator
rejects exactly the expressions that cannot be split as lists, preserving full path bytes,
stability, metadata, multiplicity and malformed-input boundaries. Keep genuine EList
handling unchanged unless separately proved. The schema-wide reconstruction repair
belongs to existing task 5834, including arbitrary protobuf merge and unknown-field
semantics; a fixture-specific shortcut is not an acceptable substitute.

The map-key decoder has a separate source-level mechanism: `validate_canonical_path`'s
`StableValue` branch runs `eval_stable_par_declared_keys`, which materializes each remaining
Expr suffix, then `dismantle_collecting_epathmap_keys`, which decodes nested components again.
The branch compares protobuf bytes; it does not re-encode the trie path for that comparison.
The small split-classifier repair must not be claimed to fix this validation path or the
separate repeated lookup/decode traversal. Their exact profile shares remain unmeasured.
At this initial checkpoint the high-depth operation ladder and repair qualification were open;
the successor sections below record their later results. Full codec/campaign acceptance
does not follow from this investigation. No production workaround was introduced to obtain
the baseline results.

## Artifact integrity

Paths below are relative to `target/verification/`; source inputs are committed at `fefd234ce`.

| Artifact | SHA-256 |
|---|---|
| `fixture-phase-6444-ladder-r2.log` | `b6ed823b2bf499c5127ee693169c70248f8d25ae26be402859d1675ff4f57f9b` |
| `fixture-phase-6444-ladder-r3.log` | `3fa23c5cd9457aa4e92667a92b98b460b7f95420f010ad5478d2fd440f15f2d2` |
| `fixture-phase-6444-construction-r4.log` | `c14c54656ea0260223d8195ad0b05b88edd94aeb4af6bb0b2bf959f241d1b1d0` |
| `fixture-phase-6444-heaptrack-r1-d128-prep0.txt` | `1c5607c64e546e331c2f5f7516bfcb0385b01c03a26eb6c02c694922a3dccdd3` |
| `fixture-phase-6444-uprof-r1-d128/report.csv` | `d0a084df6e84cb70261c3e77122228f1e6a29c1ff4ba1a8d19702258c0ae3409` |
| `fixture-phase-6444-uprof-r2-d128/report.csv` | `705f3c1838d398432b42ba15779b6fa98a63d99a3194252809ddf0e56452a162` |

The release executable SHA-256 is
`c11d6692390a69c4b39e505bb498a84e973047bf3dffc3e67c87e3aa81a3de69`.
The initial launcher attempt failed before measurements because systemd expanded the
shell phase variable. The accepted launcher explicitly uses `--expand-environment=no`;
the failed attempt is retained as experiment artifact 676, not included in the samples.

## Qualified split-carrier repair and successor measurements

**Implementation:** `0f846b3fd`; **experiment:** 345, artifact 681. This checkpoint removes
one diagnosed source of repeated suffix reconstruction. It does not change PathMap itself
or measure an improvement to its point-lookup algorithm.

The single-visitor classifier reads the resident Expr's canonical protobuf header. A proven
non-list needs no semantic decode; a list or inconclusive header uses the same decoder and
metadata checks as before. Native counters establish zero non-list decodes and one genuine-list
decode. The conditional Rocq proof has 20 closed reports and independent kernel checking;
the 35 canonical-path, 27 manifest, seven schema-cursor and 25 EPM1/meter tests pass with the
original depth-20,000 and depth-4,096 gates. J-173 and SCP-01–08 specify the proof/native limits.

### Successor method

Immediately before measurement, a three-second `mpstat` observation showed CPU 3 at 93.25%
idle and all cores at 73.73% idle; no Cargo or rustc process was found. All owned builds and tests
were terminal. The service pins each measurement to CPU 3, uses `MemoryMax=2G`,
`MemoryHigh=1536M`, `MemorySwapMax=0`, `CPUQuota=100%`, a 600-second overall watchdog and
120 seconds per phase cell. It completes successfully in 55.397 seconds, consuming 55.135 CPU
seconds, with reported peak memory 5.9 MiB and zero swap. No cell is censored.

The same calibrated phase harness runs escaped outer construction and legacy decoding at
64/128/256, outer construction additionally at 512/1024/2048/4096, and map-key decoding at 128
as a negative control. Every cell has one pilot, three warmups and seven retained observations.
These before/after phase runs are not contemporaneously paired. The classifier-only probe
alternates frozen/successor order within each sample, balances the test-only decode counter,
and excludes checking/dropping returned values from each timed call. It has seven cases and
14 case/arm cells. Tiny cases batch 1,000 calls, list-64 batches three and the others one;
the timer is read around each call, so tiny-case clock overhead is not amortized away.

The [77 retained phase observations](split-prefilter-phases-r1-2026-09-17.tsv) and
[98 retained paired observations](split-prefilter-paired-r1-2026-09-17.tsv) preserve batch totals.
Artifact 681 also retains pilots and warmups. All 47 new metric groups were accepted by pgmcp
under distinct successor/paired arm labels; none overwrites the original 126-cell baseline.

### Outer construction: the targeted slope is removed

| Depth | Earlier time (ms) | Successor time (ms) | Earlier requested bytes | Successor requested bytes |
|---:|---:|---:|---:|---:|
| 64 | 67.088 | 1.190 | 23,439,533 | 435,777 |
| 128 | 264.489 | 2.374 | 91,187,245 | 869,633 |
| 256 | 1,035.101 | 4.717 | 359,925,677 | 1,737,473 |
| 512 | Not measured | 9.939 | Not measured | 3,473,089 |
| 1,024 | Not measured | 18.499 | Not measured | 6,944,257 |
| 2,048 | Not measured | 37.722 | Not measured | 14,200,910 |
| 4,096 | Not measured | 79.159 | Not measured | 29,074,958 |

**MEASURED:** at depth 256 this phase requests approximately 207 times fewer cumulative
allocation bytes and takes approximately 219 times less instrumented time. These are scoped
diagnostic before/after ratios, not confidence-qualified speedups for Rholang applications or
native PathMap lookup. Seven-size log-log fits versus depth yield time exponent 1.0043 and
requested-byte exponent 1.0085. The finite ladder supports the targeted linear-work hypothesis;
it is not an asymptotic proof. Requested bytes are cumulative allocation traffic, not RSS.

### Genuine-list overhead and remaining costs

| Classifier case | Frozen median (ns/call) | Successor median (ns/call) |
|---|---:|---:|
| Empty list | 419.722 | 435.202 |
| Four-element list | 43,588.818 | 43,634.008 |
| Scalar | 237.169 | 99.695 |
| Depth-64 list | 60,849,650.667 | 60,840,336.333 |
| Depth-128 list | 246,227,748 | 245,983,344 |
| Depth-64 escaped set | 63,786,420 | 3,517 |
| Depth-128 escaped set | 251,709,774 | 6,953 |

The empty-list case adds 15.48 ns, about 3.7%, in this instrumented run; that negative observation
is retained. The flat/deep genuine-list medians differ by roughly 0.1% or less. Seven samples
do not establish a general non-regression or statistical-significance verdict. The large
non-list difference is consistent with the independently asserted removal of semantic decoding,
not with a change to trie lookup.

Legacy decoding remains superlinear: successor requested bytes are 23,916,928 / 92,120,832 /
361,771,584 at depths 64/128/256, with medians 72.963 / 256.602 / 1,053.822 ms. Removing the
classification decode approximately halves the earlier depth-256 allocation traffic, but
leaves the generated decoder's resident-suffix construction intact. The map-key negative
control remains exactly 1,041,415,537 requested bytes and 6,906,667 allocation calls at depth
128; its 1,609.716 ms median is not evidence of a repair to map validation. These costs remain
required work under 5834, not accepted residual debt or deferred obligations.

### Successor artifact integrity

All log paths are relative to `target/verification/`.

| Artifact | SHA-256 |
|---|---|
| `split-prefilter-paired-r1.log` | `5d201809f68a64e45ac88c1985f9fe11b2ec53dd7943427d34bf6aaec39bf1f1` |
| `split-prefilter-phases-r1.log` | `5e77a4454387840ec055d9bd3fc02e2974fdef1c2844bad97a6881e4b7a182cd` |
| `split-prefilter-single-visitor-native-r1.log` | `3bfab381fc3abe3b3a185fdcd16fd0a6293aaa04208a5411cf502c956428eacc` |

The paired unit executable has SHA-256
`6d2065ebc4a7dfc4e336cd6626cb3a87b7e5f0bb0fc6f51286c778a1fd9149bc`;
the successor phase executable has SHA-256
`eb4e617ba6f8fb5a52bfc9ea253b7fab5e9f177770a756f39ce766672207117d`.
The opt-in tests are `rust::canonical_path::split_prefilter_tests::split_prefilter_paired_measurement`
and `fixture_phase_probe::fixture_phase_measurement`. The latter retains the original
`FIXTURE_SHAPE/PHASE/DEPTH/BATCH/SAMPLES/WARMUPS` selectors. The paired probe uses
`SPLIT_SAMPLES=7` by default. Invoke with `--exact <name> --ignored --nocapture`, CPU affinity
and the resource caps above. No profiler or compilation overlaps this measurement service.

## Extended operation ladder and explicit stopping outcomes

**MEASURED, 2026-09-17:** artifact 682 completes the accounting for all 64 initially
registered operation cells at depths 512 and 1024, at implementation `0f846b3fd`.
Construction was already measured through 4096 and is not repeated here. The
[complete observation table](final-root-fixture-phase-r5-2026-09-17.tsv) includes pilots,
warmups and partial observations; the [64-cell disposition table](final-root-fixture-phase-r5-dispositions-2026-09-17.tsv)
distinguishes completed cells, actual deadlines and cells not run.

The run retains the same one-pilot, three-warmup, seven-retained protocol, CPU 3 affinity,
2 GiB hard cap, 1536 MiB high watermark, zero swap and one-core quota. The immediate idle
check was 99.34% on CPU 3 and 83.89% machine-wide. Other worktrees had build/test activity;
there was ample idle capacity and no owned compilation/test overlap. Consequently elapsed
times remain diagnostic; allocation requests are the primary mechanism observation.
The overall watchdog was 2400 seconds and each cell had a 120-second deadline followed
by a five-second termination grace period. The service exited successfully after 450.546
seconds, consuming 446.712 CPU seconds; systemd reported 18.8 MiB peak memory and zero swap.

Service success does not mean every measurement cell completed. There are 59 complete cells
with 413 retained observations, three deadline-limited cells with seven partial retained
observations in total, and two higher-depth cells deliberately not run. All 668 emitted
pilot/warmup/retained rows are preserved. Only complete seven-observation cells enter the
new metric groups; partial rows are evidence of the stopping boundary, not a complete sample.

| Shape / operation | Depth | Disposition | Retained observations |
|---|---:|---|---:|
| Escaped / legacy decoding | 1024 | Actual 120-second cell deadline, exit 124 | 3 of 7 |
| Map / canonical-key decoding | 512 | Actual 120-second cell deadline, exit 124 | 0 of 7 |
| Map / canonical-key decoding | 1024 | Not run after the preceding rung's deadline | 0 |
| Map / decoded lookup descent | 512 | Actual 120-second cell deadline, exit 124 | 4 of 7 |
| Map / decoded lookup descent | 1024 | Not run after the preceding rung's deadline | 0 |

The deadline covers preparation, pilot, warmups, retained measurements and checks, not one
operation. It is not evidence that a single operation took 120 seconds, failed semantically,
overflowed its stack, or exhausted memory. The conditional 2048/4096 operation extensions
are not run at this checkpoint: the initial ladder has already isolated the unresolved
reconstruction mechanisms, and larger observations are reserved for the repaired successor
comparison under task 5834. No correctness-test depth or deadline was reduced.

Selected complete-cell medians:

| Operation | Depth | Time (ms) | Cumulative requested bytes |
|---|---:|---:|---:|
| Unary canonical-key decoding | 512 | 8.017 | 2,875,439 |
| Unary canonical-key decoding | 1024 | 15.488 | 5,740,335 |
| Escaped EPM1 decoding | 512 | 0.507 | 752,321 |
| Escaped EPM1 decoding | 1024 | 1.021 | 1,506,625 |
| Escaped legacy decoding | 512 | 4,241.123 | 1,434,044,544 |
| Map EPM1 decoding | 512 | 43.114 | 25,642,022 |
| Map EPM1 decoding | 1024 | 108.861 | 51,493,414 |

The legacy decoder's depth-512 requested bytes remain approximately four times the
depth-256 successor value. Map EPM1 allocation traffic approximately doubles, whereas
its time more than doubles on these two rungs; neither result establishes a universal
complexity theorem. The map validation/descent deadlines strengthen the need for the
already-required repairs; they do not show a native PathMap point-lookup problem.
Cumulative requests can exceed a gigabyte while peak live memory remains small.

**DECISION:** retain the exact-equivalent fixture migrations and qualified header
prefilter. The construction hypothesis is supported over the measured range. The stronger
claim that all codec/validation/descent costs are linear is not supported. Task 6444's
remaining closure work is evidence review and durable handoff, not silently expanding
its fixture contract into task 5834's schema-wide production repair. Those production
repairs remain required before campaign completion; no deferral or acceptance of their
costs is implied.

The raw log `target/verification/fixture-phase-6444-ladder-r5.log` has SHA-256
`8c2ccd9555ddf455072babd814515e4793360b518a969e7952b249978ab26bd7`.
The phase executable remains
`eb4e617ba6f8fb5a52bfc9ea253b7fab5e9f177770a756f39ce766672207117d`;
the post-run hash matches the qualified successor executable.

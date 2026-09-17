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
dropped. Higher-depth non-construction phases have not yet been run and are not counted here.

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

## Decision and remaining obligations

Retain the qualified fixture migrations. Do not close task 6444 or claim that all slopes
are fixed. Before a production repair, prove that a resident encoded-Expr discriminator
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
The remaining high-depth operation ladder, repair qualification, full codec gates and campaign
acceptance are open. No new production workaround was introduced to obtain these results.

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

# Certified ParBag protobuf egress measurement record

## Question and decision rule

The production serializer can encounter the same authoritative `ParBag` owner more than once
inside one protobuf frame. The control (`adaptive`) traverses the owner once per occurrence. The
treatment (`in_frame_production`) performs one owner census, emits the owner's canonical protobuf
body through its PathMap zipper once, and replays certified byte ranges for later occurrences.
Neither arm decodes a PathMap key, projects a `Vec<Par>`, or sorts trie members.

The preregistered decision rule was:

1. at reuse one, the 99% paired-bootstrap upper confidence bound of the treatment/control median
   ratio must be at most `1.05`;
2. at reuse two through sixteen, treatment must preserve exact bytes and use one authoritative
   PathMap traversal per owner; and
3. every certified frame must publish atomically or publish nothing.

The powered 101-pair run accepts the treatment. A preceding 31-pair run is preserved in scratch as
an underpowered negative result; its apparent singleton miss was not reproduced after increasing
the sample population and removing fixture/reporting work from the measured region.

## System and provenance

| Field | Value |
|---|---|
| Date | 2026-08-26 |
| Repository base | `f1r3node-rust-mettail@1766f3e4e3367e39cae0380afa57f4771425c646` plus this checkpoint's working-tree treatment |
| Benchmark binary SHA-256 | `1b01809c49ebb772702a97304f684679a36474ad86add0e75c0a04b993652e32` |
| Rust | `rustc 1.95.0-nightly (6efa357bf 2026-02-08)`, LLVM 22.1.0 |
| Host | AMD Ryzen Threadripper PRO 5975WX, 32 physical cores, one thread per core |
| Kernel | Linux 7.1.9-arch1-2 x86_64 |
| Timing affinity | CPU 1 |
| Profiling affinity | CPU 2 for Heaptrack; CPU 3 for `perf stat` |
| Resource envelope | `systemd-run --user --scope`, `MemoryMax=2G`, `MemorySwapMax=0` |
| Timing population | 20 warmups, then 101 balanced AB/BA pairs per cell |
| Statistical analysis | 100,000 pair-preserving bootstrap resamples, seed `0x4131602720260826`; paired Wilcoxon and paired t tests with Benjamini-Hochberg correction |

The benchmark fixture has width 16 and a 256-byte requested owner body. Reuse is the number of
protobuf fields in the same output frame that reference that owner. Each timing observation covers
policy selection plus complete frame emission. Allocation and traversal counters are asserted
inside the benchmark before a row is accepted.

## Powered timing and traversal result

| Reuse | Output bytes | Treatment/control paired median | 99% paired-bootstrap interval | Adaptive traversals | Treatment traversals | Adaptive/treatment allocation calls |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 4,240 | 1.017391 | [1.006930, 1.024510] | 1 | 1 | 7 / 7 |
| 2 | 8,458 | 0.556835 | [0.551625, 0.567432] | 2 | 1 | 13 / 7 |
| 4 | 16,896 | 0.300896 | [0.298716, 0.305263] | 4 | 1 | 25 / 7 |
| 8 | 33,761 | 0.175201 | [0.173288, 0.179316] | 8 | 1 | 49 / 8 |
| 16 | 67,496 | 0.174705 | [0.171765, 0.177020] | 16 | 1 | 97 / 8 |

The singleton upper bound is 1.024510, below the 1.05 budget. Reuse two already removes nearly half
the elapsed time; reuse sixteen is approximately 5.72 times faster. The allocation counters show
that the treatment's structural work is owner-bounded rather than occurrence-bounded.

Durable data:

- [`parbag-certified-protobuf-egress-2026-08-26.tsv`](parbag-certified-protobuf-egress-2026-08-26.tsv)
  contains all observations and exact work counters;
- [`parbag-certified-protobuf-egress-analysis-2026-08-26.json`](parbag-certified-protobuf-egress-analysis-2026-08-26.json)
  contains the locked analysis and confidence intervals; and
- [`parbag-certified-protobuf-egress-summary-2026-08-26.tsv`](parbag-certified-protobuf-egress-summary-2026-08-26.tsv)
  is the five-row decision table.

Their SHA-256 values are respectively
`998efc63634fd753fb858972fbcf4769c04ef3ce6ab9fb9813c19fa17a466709`,
`ecc3c3ecacb7999b41a453537124bbdb9981f604974dbcbeed813f89d6126556`, and
`b8a11015add8e550e2edd0fa9a1fea36cf9c4041eb0a51046114e0e8bae57180`.

## Headless allocation attribution

Heaptrack ran in record-only mode. The fixture and warmup were complete before the sampled loop;
the loop printed one aggregate line only after all iterations. Equal rolling checksums and output
lengths are anti-vacuity witnesses.

| Reuse and arm | Iterations | PathMap traversals | Allocation calls | Temporary allocations | Peak heap | Profiler RSS | Checksum |
|---|---:|---:|---:|---:|---:|---:|---:|
| 1 adaptive | 5,000 | 5,000 | 35,327 | 20,093 | 95.90 KiB | 6.79 MiB | 241387112881884679 |
| 1 treatment | 5,000 | 5,000 | 35,327 | 20,093 | 95.90 KiB | 7.05 MiB | 241387112881884679 |
| 16 adaptive | 1,000 | 16,000 | 99,235 | 65,353 | 223.58 KiB | 6.82 MiB | 14410217286618935996 |
| 16 treatment | 1,000 | 1,000 | 8,455 | 4,153 | 224.09 KiB | 7.22 MiB | 14410217286618935996 |

At reuse sixteen, allocation calls fall 91.48% and temporary allocations fall 93.65%. Peak heap is
output-buffer dominated and is effectively unchanged; profiler RSS is not used as an acceptance
metric. The raw record SHA-256 values for adaptive/treatment at reuse one are
`75bf3ccdc0a131b546800e262bb3878dea1eb3aaecfb006c036864643a70f042` and
`20ad8ca83f527934f2dc74d744ea8bc6f9ed9db8a9cce95532ccc6867e36e4d1`; at reuse sixteen they are
`438924af5a9ad3e4485046f62a77f9f4f60bdc284c32dfd0ef6b1df2ad049ca2` and
`7cd601d9ec56d33de7951b4851765dbeeab9271539a6dc3b48afebacb4ce5f6f`.

## Hardware-counter attribution

`perf stat` used profile-only loops with 100,000 iterations at reuse one and 10,000 iterations at
reuse sixteen. Counts are one-run causal attribution, not the timing decision authority.

| Reuse and arm | Cycles | Instructions | Branches | Branch misses | Cache references | Cache misses |
|---|---:|---:|---:|---:|---:|---:|
| 1 adaptive | 1,236,322,160 | 3,553,524,803 | 635,578,854 | 318,187 | 7,946,431 | 32,258 |
| 1 treatment | 1,215,399,398 | 3,442,204,489 | 620,463,352 | 319,552 | 13,752,994 | 29,373 |
| 16 adaptive | 1,871,141,946 | 5,361,015,986 | 967,871,086 | 659,692 | 43,009,644 | 499,880 |
| 16 treatment | 228,756,129 | 613,522,422 | 103,243,899 | 86,583 | 46,945,467 | 492,285 |

At reuse sixteen, treatment uses 12.23% of control cycles, 11.44% of instructions, and 10.67% of
branches. The increased cache-reference event is not interpreted because multiplexed generic
hardware events on this host do not identify the referenced cache level; the paired timing and
exact work counters remain primary.

## Correctness, formal equivalence, and publication

The generated encoder emits a typed pre-order transcript whose event kinds are `MessageBody`,
`ParBody`, `MapEntryBody`, and `EPathMapSnapshotBody`. Preflight reserves an event before descending,
marks it complete after the child succeeds, seals only a complete transcript, and lends replay
ranges only when event kind and length both match. The common case stores sixteen lengths and kinds
inline with a `u16` completion bitset; only larger transcripts spill. Output is private until exact
replay exhaustion, then published once.

The final executable gates passed:

- 10/10 certified-egress model tests;
- 12/12 Rholang protobuf-egress tests;
- 13/13 formal-manifest tests, 14/14 PathMap-Par tests, and 15/15 protobuf differential tests; and
- the complete Rocq, Verus, Z3, TLC, and mCRL2 stack-safe-PDA gate.

The formal-gate log SHA-256 is
`bc1ecbf6f0c2954a9a183cfbb530d2ea17bffd1ff86d19dbafa65a3c8cab6f3f`.
Seven transcript mutants must fail: duplicate completion, incomplete seal, kind swap, partial
publication, remaining-count commit, replay overrun, and spill corruption. The owner-memo model has
independent mutants for skipped census, premature emission, empty/singleton memoization, off-by-one
replay, swapped/wrong owners, incomplete commit, and whole-owner duplication. This makes a vacuous
or partially modeled proof fail the gate.

## Limits and next boundary

This checkpoint closes canonical DATA/ParBag protobuf egress. Continuation and join cold-store
egress remain a separate implementation boundary under pgmcp #6029. The full release-workspace
gate is recorded only after this checkpoint is committed and tested from that stable coordinate.
The change does not activate a new RSpace key, wire schema, history encoding, hash, matching
authority, COMM schedule, or metering rule.

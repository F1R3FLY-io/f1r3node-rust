# Headless Heaptrack attribution for canonical RSpace DATA checkpoint egress

Status: measured mechanism evidence

Date: 2026-08-24

Source coordinate: `f1r3node-rust-mettail@abd915454`

## Question and method

Does streaming the canonical bincode `Datum` identities already resident in the RSpace
`PathMap<Multiplicity>` remove the allocations attributed to typed `Datum` reconstruction and
per-occurrence encoding?

The control and treatment each ran ten checkpoint-egress operations over 4,096 distinct shared-
prefix identities at multiplicity one. Both were fresh processes under a zero-swap systemd scope
with a 12 GiB resident-set cap. Heaptrack was invoked in record-only mode, directly on the benchmark
executable; reports were rendered afterwards with `heaptrack_print`. Earlier traces attached to an
`env` wrapper instead of the benchmark are invalid and excluded.

## Results

| Measure | Legacy typed route | Canonical resident-byte route | Difference |
|---|---:|---:|---:|
| Whole-process allocation calls | 510,608 | 141,838 | -368,770 |
| Allocation calls per measured operation attributable to the treatment | — | — | -36,877 |
| Temporary allocations | 8,264 | 8,254 | -10 |
| Wall time reported by Heaptrack | 0.29 s | 0.07 s | -0.22 s |
| Peak heap | 4.18 MiB | 4.18 MiB | no displayed change |
| Peak RSS including profiler | 11.36 MiB | 10.22 MiB | -1.14 MiB |

The independent benchmark counter reports 36,913 legacy versus 36 canonical allocations per
operation, a reduction of 36,877. Its exact equality with the whole-process Heaptrack differential
divided by ten is the anti-vacuity check that the profiler attached to the intended code.

The largest removed allocation sites belong to typed bincode `Datum` decoding,
`ordered_channel_entries` materialization, per-`Datum` `encode_datums`, and collection into nested
vectors. Equal displayed peak heap is expected because fixture construction is common to both arms
and occurs before the measured egress loop; this profile supports allocation-mechanism attribution,
not a claim that the complete process peak is reduced.

## Integrity

- legacy trace SHA-256: `4a16a3d7f591707f0ea14c770aa5965426035bd484ef189b404e35277d0bbf39`
- treatment trace SHA-256: `9772029f06435d6eafcc9c0cea0239f3da33b14a7be8c59235fac17a6a13362b`
- legacy report SHA-256: `2c69d38f9f3c7ebc73f6976e9c310732e264c074515bcf275831e0dfedb34d0c`
- treatment report SHA-256: `f6642be7f0321494a802f399e8c25086b6a73f35d9f2bc029be0e7218c5628df`
- differential report SHA-256: `7df3658a786efe396a0bfe5cf52ce5f43fbfa86240b51a2149bf66350b4d27e2`
- complete raw timing matrix SHA-256: `a7a868cb52c332d741747cd958696fa5812bafe5fda4d3cf4fccd0828dde5e5d`
- durable summary TSV SHA-256: `a9676f856bcfb16c571d667eedca91a4ea81b9b828bd2eed0ec7f591beded101`

## Conclusion

The hypothesis is accepted for DATA checkpoint egress. The resident canonical identity is the
right serialization source when its encoding is exactly the destination encoding. No typed
`Datum`, occurrence vector, or second encoded identity should be built. This result does not
authorize a retained serialized snapshot or a secondary order index; those add mutation and
retained-memory costs and require their own complete-lifecycle experiment.

# RSpace candidate suffix checkpoint v2: headless Heaptrack record

Status: retained allocation-profile evidence for the deterministic-run hybrid

Source coordinate: uncommitted campaign tree based on
`a6b3ad09032c22bb10fafd6f7a890d7871b36609`

Executable:
`target/release/deps/rspace_candidate_suffix_checkpoint-8e6969cce542ad82`

Executable SHA-256:
`2a9c879ae6f64e880d75bc101d570d9fc58aa07d795f7d5339d69c49be8c79d0`

Recorder: Heaptrack in headless `--record-only` mode, analyzed with
`heaptrack_print -n 20`. Debug-info network retrieval was disabled by Heaptrack.
Each observation used the compressed topology at depth 4,096 for 100 operations.
The four result digests were identical:
`2c525fc7fecf67656c687e88a67fe0bb1d75406e187ddf158b7cf240fd4a9cdb`.

| Query | Arm | Batch time (ns) | Allocation calls | Temporary allocations | Peak heap | Peak RSS |
|---|---|---:|---:|---:|---:|---:|
| exact | frozen zipper-clone control | 39,763,556 | 26,389 | 4,416 | 739.67 KiB | 6.95 MiB |
| exact | deterministic-run hybrid | 25,077,063 | 25,689 | 4,416 | 730.84 KiB | 6.87 MiB |
| repeated capture | frozen zipper-clone control | 44,844,405 | 12,246 | 422 | 682.43 KiB | 6.69 MiB |
| repeated capture | deterministic-run hybrid | 28,419,415 | 10,846 | 422 | 673.60 KiB | 6.67 MiB |

The paired process totals attribute 700 eliminated allocation calls to 100 exact
operations, or 7 calls per operation. They attribute 1,400 eliminated calls to
100 repeated-capture operations, or 14 calls per operation. The exact hybrid
creates no production suffix checkpoint. The repeated-capture hybrid creates one
accepted-branch checkpoint per operation; its remaining `ZipperSubtries::make_map`
calls are constant in query depth. Peak heap also decreased in both comparisons.

The traces and full text reports were intentionally temporary because they are
large machine-local profiler products. Their integrity coordinates before cleanup
were:

| Profile | Trace SHA-256 | Report SHA-256 |
|---|---|---|
| exact control | `c58b30bcd4e0c2c9fc2d871ed013fa775d8d7cc7011546e705625f390b999f8f` | `731fac6aafec73bd4b92e94083bff2b9b9af9b825fe579a8ea812722f772eda5` |
| exact hybrid | `f0ae65ab7e7bbbb93b8f4358dba642cb182d7727e3126d2f4dd22602cf0ab0a5` | `9911f5764b6276b4407d2a0795d400eaa9a23d0df40dc3a6dd1665a89d369159` |
| repeated-capture control | `c59edccec61b6c57b1fbcddc59d4f0adf41393b6144fd75f2d54a4c4d89010d9` | `35d24273a6830e2337df5762fac8b96752fcfb5ebdab1665fc4bfeafbd9a2d02` |
| repeated-capture hybrid | `6d283bd105e95e023b9fb5555434d473597cdcab58014d656eb3f2b3493f5286` | `440b934aaed13ffd74bb52a5fe7489d10173dc12c658e0d3006574b53b9d69ac` |

Interpretation boundary: Heaptrack perturbs absolute timing, so the matrix TSV is
the primary latency evidence. This record supports allocation-mechanism
attribution and exact digest equivalence; it is not an independent latency trial.

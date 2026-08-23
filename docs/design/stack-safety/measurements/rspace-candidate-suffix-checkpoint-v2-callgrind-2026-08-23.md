# RSpace candidate suffix checkpoint v2: Callgrind instruction record

Status: retained deterministic instruction evidence for the deterministic-run hybrid

Source coordinate: uncommitted campaign tree based on
`a6b3ad09032c22bb10fafd6f7a890d7871b36609`

Executable:
`target/release/deps/rspace_candidate_suffix_checkpoint-8e6969cce542ad82`

Executable SHA-256:
`2a9c879ae6f64e880d75bc101d570d9fc58aa07d795f7d5339d69c49be8c79d0`

Profiler: Valgrind 3.25.1 Callgrind with cache simulation, branch simulation,
and jump collection disabled. Runs were serialized in systemd scope
`run-p3446876-i11863272` on CPUs 16–23 with `MemoryHigh=3G`,
`MemoryMax=4G`, and `MemorySwapMax=0`. Every cell used the compressed
topology and one operation. Process startup and fixture construction are
included equally in both arms at each depth; this record therefore interprets
paired arm differences, not an isolated instruction cost for the query alone.

| Query | Depth | Frozen control instructions | Hybrid instructions | Control / hybrid | Eliminated |
|---|---:|---:|---:|---:|---:|
| exact | 256 | 2,009,133 | 1,905,782 | 1.054230 | 5.144% |
| exact | 512 | 3,536,152 | 3,331,128 | 1.061548 | 5.798% |
| exact | 1,024 | 6,590,910 | 6,185,254 | 1.065584 | 6.155% |
| exact | 2,048 | 12,895,478 | 12,042,660 | 1.070816 | 6.613% |
| exact | 4,096 | 25,672,475 | 23,969,973 | 1.071026 | 6.632% |
| repeated capture | 256 | 2,243,762 | 2,143,185 | 1.046929 | 4.483% |
| repeated capture | 512 | 3,990,774 | 3,790,360 | 1.052875 | 5.022% |
| repeated capture | 1,024 | 7,459,816 | 7,074,189 | 1.054512 | 5.169% |
| repeated capture | 2,048 | 14,577,845 | 13,782,605 | 1.057699 | 5.455% |
| repeated capture | 4,096 | 28,844,216 | 27,225,478 | 1.059457 | 5.612% |

All 20 result digests were
`2c525fc7fecf67656c687e88a67fe0bb1d75406e187ddf158b7cf240fd4a9cdb`.
Instruction counts grow approximately linearly in both arms; the hybrid has a
lower constant at every registered depth. This supports the mechanism claim
that maximal deterministic runs remove work rather than shifting it into
another allocation path. The paired matrix and headless Heaptrack record remain
the primary latency and allocation evidence.

The exact invocation family was:

```text
env CANDIDATE_CHECKPOINT_PROFILE_ARM=<arm> \
    CANDIDATE_CHECKPOINT_PROFILE_SHAPE=compressed \
    CANDIDATE_CHECKPOINT_PROFILE_QUERY=<query> \
    CANDIDATE_CHECKPOINT_PROFILE_DEPTH=<depth> \
    CANDIDATE_CHECKPOINT_PROFILE_ITERATIONS=1 \
valgrind --tool=callgrind --quiet --cache-sim=no --branch-sim=no \
    --collect-jumps=no --callgrind-out-file=<temporary-output> <executable>
```

The 20 raw Callgrind outputs were temporary machine-local profiler products and
were removed after their totals and result records were checked into this note.

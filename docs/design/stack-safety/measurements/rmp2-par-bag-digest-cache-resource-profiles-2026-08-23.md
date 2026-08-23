# RMP2 ParBag digest cache: deterministic resource profiles

Status: accepted instruction, data-read, allocation, and retained-memory evidence

Source coordinate: uncommitted campaign tree based on
`a6b3ad09032c22bb10fafd6f7a890d7871b36609`

Executable:
`target/release/deps/rmp2_par_bag_digest_cache-1a2186788474e6d8`

Executable SHA-256:
`ae595a3d3216b36783ea424f77c02323698a2774b27a9145bdf030837fa10c03`

The feature-only profiler exposes two matched null controls. `null_profile`
constructs the same cold subject but does not request its summary.
`warm_null_profile` primes the same subject outside the profiled request loop,
then performs the same checksum fold without reading the cache. These controls
do not exist in an ordinary production build.

## Registered sentinel cells

| Cell | Distinct endpoints | Body bytes | Multiplicity | Transcript bytes |
|---|---:|---:|---:|---:|
| small | 1 | 16 | 1 | 69 |
| midpoint | 64 | 256 | 4 | 17,697 |
| large | 1,024 | 4,096 | 32 | 4,214,817 |

Every compared pair produced the same checksum. Multiplicity remains one
encoded field per distinct endpoint; neither profiler observes an
occurrence-expanded loop.

## Cachegrind instruction and data-access evidence

Valgrind 3.25.1 Cachegrind ran in systemd scope
`run-p3773271-i12214279` for cold/null and
`run-p3767143-i12149635` for warm/warm-null. Both scopes used CPUs 28–31,
`MemoryHigh=5G`, `MemoryMax=7G`, and `MemorySwapMax=0`. Cache simulation was
enabled and branch simulation disabled.

Cold rows contain one request. Values below subtract the identical cold null
process, including fixture construction and process startup:

| Cell | Instructions | Data reads | Data writes | L1 data-read misses | L1 data-write misses |
|---|---:|---:|---:|---:|---:|
| small | 6,775 | 1,511 | 1,048 | 2 | 0 |
| midpoint | 454,408 | 100,866 | 54,753 | 1,167 | 17 |
| large | 100,787,310 | 22,256,815 | 11,783,874 | 296,338 | 165,017 |

Warm rows reuse one prepared subject for 10,000 requests. Values below subtract
the matched warm-null process and divide by 10,000:

| Cell | Instructions/request | Data reads/request | Data writes/request | L1 read misses/request | L1 write misses/request |
|---|---:|---:|---:|---:|---:|
| small | 29.0003 | 13.0000 | 9.0000 | 0.000000 | -0.000100 |
| midpoint | 28.9779 | 12.9992 | 9.0000 | -0.000100 | 0.000500 |
| large | 29.1625 | 13.0106 | 8.9999 | 0.003200 | -0.002400 |

Tiny negative miss differences are matched whole-process layout noise, not
negative operations. The instruction, read, and write counts are constant to
measurement precision across a 61,084-fold transcript-size range. This is the
registered deterministic evidence for a warm `$`\Theta(1)`$ request.

## Headless Heaptrack evidence

Heaptrack ran only in headless `--record-only` mode in systemd scope
`run-p3776261-i12191249` on CPUs 24–27 with `MemoryHigh=5G`,
`MemoryMax=7G`, and `MemorySwapMax=0`. `heaptrack_print -n 15` produced the
text reports. Cold profiles use one request; warm profiles reuse one prepared
subject for 10,000 requests.

| Cell | Arm | Allocation calls | Temporary allocations | Peak heap | Peak RSS, including profiler | Leaked |
|---|---|---:|---:|---:|---:|---:|
| small | cold null | 129 | 28 | 81.12 KiB | 7.20 MiB | 544 B |
| small | cold | 133 | 29 | 81.11 KiB | 7.15 MiB | 544 B |
| midpoint | cold null | 2,296 | 57 | 239.52 KiB | 7.32 MiB | 544 B |
| midpoint | cold | 2,303 | 61 | 239.52 KiB | 7.38 MiB | 544 B |
| large | cold null | 221,885 | 62 | 38.24 MiB | 46.83 MiB | 544 B |
| large | cold | 221,900 | 66 | 38.24 MiB | 46.73 MiB | 544 B |
| small | warm null, 10,000 | 133 | 30 | 81.12 KiB | 7.36 MiB | 544 B |
| small | warm, 10,000 | 133 | 30 | 81.11 KiB | 7.17 MiB | 544 B |
| midpoint | warm null, 10,000 | 2,303 | 62 | 239.52 KiB | 7.59 MiB | 544 B |
| midpoint | warm, 10,000 | 2,303 | 62 | 239.52 KiB | 7.30 MiB | 544 B |
| large | warm null, 10,000 | 221,900 | 67 | 38.24 MiB | 41.55 MiB | 544 B |
| large | warm, 10,000 | 221,900 | 67 | 38.24 MiB | 41.54 MiB | 544 B |

Cold minus null adds 4, 7, and 15 transient allocation calls at the three
sentinels. The harness's operation-local allocator records 992, 2,784, and
56,544 allocated bytes respectively, all deallocated before the request ends.
The 32-byte digest resides inline in the already-budgeted `ParBagAux`
`OnceLock`; cold and null leak totals and displayed peak heap are equal.

Warm and matched warm-null allocation-call totals are exactly equal at every
sentinel across 10,000 requests. Displayed peak heap is also equal within
Heaptrack's precision. Peak resident-set differences are small, unsigned, and
do not grow monotonically; they are profiler and page-residency noise rather
than retained cache allocation.

## Reproduction templates

Cold and null Cachegrind:

```text
env RMP2_CACHE_ARM=<null_profile|cold_cache> \
    RMP2_CACHE_WIDTH=<width> RMP2_CACHE_BODY_BYTES=<body> \
    RMP2_CACHE_MULTIPLICITY=<multiplicity> RMP2_CACHE_ITERATIONS=1 \
    RMP2_CACHE_PROFILE_QUIET=1 \
valgrind --tool=cachegrind --quiet --cache-sim=yes --branch-sim=no \
    --cachegrind-out-file=<temporary-output> <executable>
```

Warm and warm-null Heaptrack or Cachegrind adds:

```text
RMP2_CACHE_ARM=<warm_null_profile|warm_cache>
RMP2_CACHE_ITERATIONS=10000
RMP2_CACHE_REUSE_PREPARED=1
```

The raw profiler traces were temporary machine-local products. Their semantic
coordinates are the executable digest, exact environment above, scope IDs,
registered cells, pair checksums, and full counters retained in this report.
The traces were removed after analysis to avoid retaining derived gigabyte-scale
profiler state in the repository or `/tmp`.

## Decision

Accept the clone-family cache. The cold route retains the expected transcript-
linear work and temporary allocation, while a warm request has constant
instructions, data accesses, zero additional allocation calls, and no retained
heap slope. Combined with the full wall/allocation matrix and the powered tiny-
cell replication, all preregistered resource thresholds pass. Semantic,
concurrency, mutation, byte-equivalence, and auxiliary-size gates are reported
in J-009; broader COMM/replay and full-release validation remain lifecycle
gates rather than resource hypotheses.

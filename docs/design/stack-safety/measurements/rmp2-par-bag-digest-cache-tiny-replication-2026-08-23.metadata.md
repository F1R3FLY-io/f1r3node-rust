# RMP2 ParBag digest cache: tiny-transcript replication metadata

Status: preregistered cold-regression diagnostic replication

Raw data:
[`rmp2-par-bag-digest-cache-tiny-replication-2026-08-23.tsv`](rmp2-par-bag-digest-cache-tiny-replication-2026-08-23.tsv)

Raw-data SHA-256:
`9469c7f153a802c689e9999e68dcf558b4f5aa08541b5c7c1405af226293aa62`

Source coordinate: uncommitted campaign tree based on
`a6b3ad09032c22bb10fafd6f7a890d7871b36609`

Executable:
`target/release/deps/rmp2_par_bag_digest_cache-1a2186788474e6d8`

Executable SHA-256:
`df24cf95d8a9247bf1a1896cd76dd7ea0829084c2d900edb8a7a71c29e294d18`

The run used systemd scope `run-p3591352-i11979537`, CPUs 24–25,
`MemoryHigh=2G`, `MemoryMax=3G`, and `MemorySwapMax=0`. The selected cell had
16 distinct endpoints, exact 16-byte canonical protobuf bodies, multiplicity
one, and a 609-byte RMP2 transcript. Thirty warmups preceded 300 retained
rotated six-lane pairs. Initial one-, five-, and fifteen-minute load averages
were 10.10, 10.39, and 12.93 on a 32-core host. The command incurred no major
page faults and no swap; peak resident set size was 23,412 KiB.

| Lane | Retained samples | Median (ns) | Mean (ns) | Allocation calls | Allocated bytes |
|---|---:|---:|---:|---:|---:|
| uncached control | 300 | 1,703 | 1,742.497 | 4 | 992 |
| cold cache | 300 | 1,704 | 1,727.997 | 4 | 992 |
| warm cache | 300 | 50 | 48.100 | 0 | 0 |
| prewarmed CoW clone | 300 | 50 | 48.797 | 0 | 0 |
| first post-mutation | 300 | 1,703 | 1,845.570 | 4 | 992 |
| second post-mutation | 300 | 50 | 47.710 | 0 | 0 |

Cold versus uncached had a paired median ratio of 0.994474 and a paired median
difference of -9.5 ns. The paired mean difference was -14.500 ns, with an
approximate two-sided 99% Student interval from -113.769 ns to 84.769 ns. A
200,000-replicate paired bootstrap of the ratio of means used NumPy 2.5.2's
PCG64 generator with seed `0x41315984`; its two-sided percentile 99% interval
was [0.934846, 1.049629], with median 0.991411. Its upper endpoint remains below
the preregistered 1.05 cold-regression ceiling.

The full 30-pair matrix had reported a 1.0736 median cold/control ratio in this
one cell. This replication does not delete that negative observation. It shows
that the small original sample did not reproduce with greater power, while the
warm and clone-shared lanes still perform zero allocations and remain about 34
times faster at the selected cell. No production threshold, bypass, or special
case was introduced in response.

Exact invocation:

```text
env RMP2_CACHE_WIDTH=16 RMP2_CACHE_BODY_BYTES=16 \
    RMP2_CACHE_MULTIPLICITY=1 RMP2_CACHE_REPS=300 RMP2_CACHE_WARMUP=30 \
    /usr/bin/time -v <executable>
```

Interpretation boundary: the time distributions include short-duration timer
noise and 301 observed involuntary context switches. The paired rotation and
bootstrap quantify that uncertainty; this is not evidence for an exact
nanosecond production cost.

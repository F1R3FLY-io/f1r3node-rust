# Guarded PathMap final teardown and loaded-channel replacement — 2026-09-01

## Status and scope

This report records the causal resource experiment attached to pgmcp work item
`measure-fresh-rebuild-channel-replacement-and-final-root-teardown`, a child of
`guarded-pathmap-final-root-allocation-teardown` and root epic 4131. The implementation coordinate
is `68534b6fc`; benchmark coordinates are `a5c08c87c`, `e94735461`, and `9ab98a331`.

The experiment measures two consequences of ownership-disjoint structural publication:

1. replacement of one **already-loaded** RSpace channel, which streams all retained Bag endpoints
   into a fresh `PathMap<Multiplicity>` before replaying the replacement prefix; and
2. final destruction of the last `GuardedPathMap` owner, whose two-phase pushdown automaton (PDA)
   disposes values before dismantling value-free topology one logical edge at a time.

This checkpoint changes no Rholang value, matching result, multiplicity, candidate order, COMM
schedule, protobuf/bincode/EPM1/RMP2 byte, hash, compute charge, storage charge, or persistence
format. It introduces no dependency, manifest change, `RUST_MIN_STACK`, `stacker`, traversal limit,
PathMap fork, flattened `Vec<Par>`, or sorting pass.

## Questions and preregistered hypotheses

Let $`B`$ be retained logical Bag bytes, $`E`$ be valued endpoints, and $`T`$ be logical topology
bytes presented to final teardown.

- **H1 — loaded replacement.** One loaded-channel replacement performs $`O(B + E)`$ work because it
  visits the retained source once with a read zipper and publishes one fresh candidate with a
  moving write zipper.
- **H2 — final teardown time.** Final teardown performs $`O(T + E)`$ work with constant native stack
  in path depth.
- **H3 — final teardown retention.** A removed real payload is disposed before the next traversal
  step, so detached-payload retention remains $`O(1)`$.
- **H4 — final teardown allocation.** The f1r3node compatibility layer should be measured rather
  than presumed allocation-optimal because PathMap 0.2.2 exposes no nonrecursive, ownership-safe
  physical-node drain.

## Method

### Controls and isolation

- The existing `rspace_channel_replacement` executable preserves its `prefix-local` production arm
  and frozen `whole-store-clone` causal control unchanged.
- The new `loaded-channel` arm builds the resident store before its timer, replaces
  `channel-00000000` with one fixed-size datum, and asserts exact distinct, cardinality, and loaded
  counts after every sample.
- The ignored final-teardown test constructs its topology before its timer, then times the real
  `AnchorRoot::drop`. Each trial constructs an independent guard.
- Teardown shapes are: a value-free line; a valued comb; dense fixed-width `u64` paths; and
  939-byte keys with a 203-byte common prefix, matching the key geometry exercised by the real
  Rholang reducer regression.
- Every heavy invocation ran through `systemd-run` with `MemorySwapMax=0`. Compile checks used a
  12 GiB hard ceiling; ordinary timing used 2–4 GiB; Heaptrack used 8 GiB; report generation used
  2 GiB. Observed swap was zero in every accepted invocation.
- The host exposed 32 CPUs. These first matrices ran while unrelated Rust work remained active but
  aggregate idleness was approximately 55–66%. They are therefore causal scaling evidence, not the
  final idle-host absolute-throughput control.

### Statistical reduction

Each geometric point contains five independent observations. The reported center is the median and
dispersion is the median absolute deviation (MAD). Ordinary least squares fits
$`\log(t)=\alpha+\beta\log(n)`$ to the five medians. The two-sided 95% interval for $`\beta`$ uses
Student's $`t`$ distribution with three degrees of freedom. A parser guard rejects any point that
does not contain exactly five observations; this caught and corrected an initial analysis that had
omitted the first line prefixed by Rust's test console.

## Results

### Loaded-channel replacement

| resident endpoints | median elapsed | invariant result |
|---:|---:|---|
| 1,000 | 53.682189 ms | exact |
| 2,000 | 107.458105 ms | exact |
| 4,000 | 214.810740 ms | exact |
| 8,000 | 427.619576 ms | exact |
| 16,000 | 858.899973 ms | exact |

The fitted slope is **0.999250304**, with 95% confidence interval
**[0.996032204, 1.002468403]**. H1 is accepted for the mechanism and scaling class: the measured
structural replacement is linear in retained resident endpoints. The run peaked at 17.1 MiB RSS
with zero swap. This result does not supersede the distinct-new-channel SS-C32 measurement: new
channels continue to take the prefix-rooted in-place branch and do not scan the resident Bag.

### Final-owner teardown

The table reports `median / MAD` in milliseconds.

| shape | geometric sizes | median / MAD at smallest size | median / MAD at largest size | fitted slope (95% CI) |
|---|---|---:|---:|---:|
| value-free line | 1,024–16,384 bytes | 0.336638 / 0.024346 | 4.496256 / 0.007774 | 0.957847144 [0.854760911, 1.060933376] |
| valued comb | 128–2,048 teeth | 0.063190 / 0.000921 | 0.976742 / 0.009117 | 0.989113725 [0.932310627, 1.045916824] |
| dense fixed-width | 1,000–16,000 endpoints | 0.250025 / 0.007906 | 3.800978 / 0.028775 | 0.985569487 [0.952530443, 1.018608531] |
| 939-byte realistic | 16–256 endpoints | 3.741365 / 0.007193 | 57.860517 / 0.389448 | 0.984322909 [0.966366526, 1.002279292] |

All one hundred observations passed exact value-disposal assertions. Every interval contains or is
statistically consistent with linear scaling. Together with the existing 65,536-byte line and comb
tests on 256 KiB native stacks, this accepts H2 and H3: final teardown is time-linear, uses no
depth-growing native call stack, and retains no slope of detached real payloads.

### CPU attribution with AMD uProf

AMD uProf 5.3.521.0 profiled twenty realistic size-256 trials with 1 ms time-based hotspots. The
target completed in 1.18 s; the generated report contains 1.169 s of target-thread CPU time. The
largest self-time rows were:

| function family | sampled self CPU time |
|---|---:|
| `fast_slice_utils::count_shared_reference` | 0.192 s |
| PathMap write-zipper `child_mask` | 0.121 s |
| `GuardedPathMap::AnchorRoot::drop` | 0.107 s |
| PathMap mutable child lookup | 0.078 s |
| PathMap write-zipper descent | 0.070 s |
| PathMap branch counting | 0.053 s |
| PathMap prune internals | 0.049 s |
| PathMap line-list split | 0.042 s |

The profiler reported 291.4 MiB process peak RSS and zero swap. uProf stated that its Hotspots mode
ignored the explicitly requested DWARF mode, although the report records call-stack sampling as
enabled with frame-pointer-omission recovery. Function-level attribution is accepted; deeper
call-tree interpretation is not used.

PathMap's `node_count_branches_recursive` name does not denote Rust call recursion: source review
shows one optional child-reference follow followed by a local `count_branches` call. It is not a
remaining stack-overflow site.

### Allocation attribution with headless Heaptrack

Heaptrack 1.5.0 recorded one realistic size-256 trial with `--record-only`; only
`heaptrack_print -n 30` rendered the report. No graphical analyzer was invoked.

| metric | result |
|---|---:|
| allocation calls | 187,109 |
| temporary allocations | 177,403 |
| profiler peak heap | 386.49 KiB |
| profiler peak RSS including overhead | 13.05 MiB |
| systemd collector peak RSS | 71.9 MiB |
| swap | 0 B |

The dominant stack contains **181,708** `pathmap::line_list_node::split_0` allocations beneath
`AnchorRoot::drop`. Phase B installs a synthetic value at the immediate parent of each edge so
PathMap's `prune_ascend` cannot detach and recursively destroy an unbounded physical subtrie. When
the logical parent lies inside a compressed line-list node, setting that barrier splits the node.
The resulting allocation count is linear in topology bytes and almost entirely temporary, which
explains the low peak heap and absence of retained-memory slope.

## Engineering conclusion

H1–H3 are accepted for their scaling and safety claims. H4 is also confirmed: current final teardown
is correct, stack-safe, time-linear, and retained-space bounded, but is **not allocation-optimal**.

No released PathMap 0.2.2 public operation closes the remaining trade-off:

- `prune_path` may detach an arbitrarily deep physical subtrie whose ordinary destructor recurses;
- `take_map` transfers, rather than removes, that recursive-drop obligation; and
- public logical-byte zippers do not expose a non-allocating physical-node dismantling cursor.

Removing the synthetic barriers locally would therefore weaken a proved stack-safety invariant.
The principled local decision is to retain the measured linear compatibility layer and refine
upstream request PMU-005: PathMap should provide intrinsic iterative destruction or a consuming
physical-node drain whose owned temporaries are themselves stack-safe on every exit path. No
PathMap fork is authorized by this result.

## Reproducibility artifacts

| artifact | SHA-256 |
|---|---|
| `/var/tmp/f1r3node-rspace-channel-replacement-concurrent-2026-09-01.tsv` | `b592e0946c959fe373e182e55f6695318871b6aa9a8aa30afd319481dc3d71a5` |
| `/var/tmp/f1r3node-rspace-loaded-replacement-medians-2026-09-01.tsv` | `fca32bc0dbb35657dd0408c777f19e83908ebb84be411c56b7c404ad1aa089b3` |
| `/var/tmp/f1r3node-final-teardown-concurrent-2026-09-01-v1.tsv` | `be26fe36c6049be49515dcdd07c5b8378b46eb5383dd12183e075b5e42fd9f48` |
| `/var/tmp/f1r3node-final-teardown-concurrent-medians-2026-09-01-v2.tsv` | `4774e15535ccc8a941b08009c750aaf0dd19d9387d76743234cd0faff3252d9f` |
| `/var/tmp/f1r3node-final-teardown-concurrent-slopes-2026-09-01-v2.tsv` | `d916cc47f1cc19ff08f55a2c817d12c530cdb00bbfcf71247e4eb9451ba31a81` |
| `/var/tmp/f1r3node-final-teardown-uprof-report-2026-09-01-v2.csv` | `12268e98c2e52d310f5c8653f9bfd97eeeb9d4f4b9c6db9b9fb0f073eea50ca3` |
| `/var/tmp/f1r3node-final-teardown-uprof-driver-2026-09-01-v2.log` | `4831ac329838ef685b22cb844c6b27693e179949ba06809d43618f9953aac554` |
| `/var/tmp/f1r3node-final-teardown-realistic-256-heaptrack-2026-09-01-v1.zst` | `0365ffe1128de6508990de4a86377d15c6ec5040a9ab016acc0f729d0d1e6802` |
| `/var/tmp/f1r3node-final-teardown-realistic-256-heaptrack-report-2026-09-01-v1.txt` | `0591bc2d3fd6d1cd59c0bbcaaca73558068cedfb985ae82b95d5a29634e4dc78` |
| `/var/tmp/f1r3node-final-teardown-heaptrack-driver-2026-09-01-v1.log` | `c983bb9745bac8cec6218f223fc9e39d0dc6a9861eb40e7ca3dd4c4299512324` |

## Remaining evidence boundary

The geometric scaling conclusions are robust to the observed low MAD and large separation between
linear and quadratic growth. Absolute latency remains provisional until the same decisive endpoints
are repeated on an otherwise idle host. PMU-005 remains open until an upstream release supplies and
passes the fixed-stack, ownership, allocation, CoW, and failure-path gates; this campaign does not
block its correct local implementation on that future API.

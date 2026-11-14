# Guarded PathMap barrier-free final teardown — 2026-11-14

## Scope

This record measures how many heap allocation calls the final teardown of a `GuardedPathMap` makes after commit `6642c0d13`, which removed the synthetic barriers from that teardown, and how long one final teardown takes on the measured commit and on the parent of that commit. PathMap is the trie library that the `rspace_plus_plus` crate (directory `rspace++`) stores its tuple-space indices in. `GuardedPathMap` is that crate's stack-safe owner of a PathMap trie, and its final teardown is the `Drop` implementation of its root owner `AnchorRoot`, which runs when the last owner of a trie is dropped.

[The record of 2026-11-12](guarded-pathmap-final-teardown-pathmap-0.3.4-2026-11-12.md) counted, on PathMap 0.3.4,
184,489 allocation calls beneath the final teardown of the realistic shape with 256 endpoints and 32,489
beneath that of a value-free line of 16,384 bytes; 182,623 and 31,984 of them were calls of `split_0`. Phase B
of the teardown then installed a synthetic barrier value at the parent of every edge it pruned, and a barrier
inside a compressed line-list key splits the key and allocates one trie node.
[The remedy study](../guarded-pathmap-teardown-allocation-remedy-study.md) chose remedy R2a for bug 6441: the barrier-free
one-edge cut of its Algorithm 2, which uses only the public interface of PathMap 0.3.4. Commit
`6642c0d13` implemented it.

This is the re-measurement that the acceptance of bug 6441 requires under remedy R2a: leaf 14365 (`guarded-pathmap-teardown-barrier-free-remeasure`, milestone 7919). It repeats M1, M2, M4 and M5 of the 2026-11-12 record with that record's protocol, pre-registered as pgmcp experiment 488: Heaptrack 1.5.0 in record-only mode, one `heaptrack_print` rendering per recording, the record's extraction script and an independent recount. M1 and M2 are the two coordinates, M4 is the negative control of the attribution, and M5 is the 20,000-bind guarded matching test. The record's M3, M6 and positive control are not repeated. In place of M6, the wall time of one final teardown was compared on the commits before and after the cut, under the protocol registered as pgmcp experiment 498. The record reports measurements and states whether the leaf's thresholds hold; it contains no decision.

No production source, test source or manifest was changed: every executable was built from an unmodified export of commit `f6afb445e3fe56af479ac5404b9823995df818ee`, which holds the cut, and, for the wall-time comparison only, of commit `f7c88aa914fb521f4e805f600fbed21b148405a9`, the parent of the cut. All runs took place on 2026-11-14 in Coordinated Universal Time (UTC), from 08:47:01 to 08:47:34.

## Terms and symbols

| Term | Meaning |
|---|---|
| PathMap | The byte-keyed trie crate `pathmap`; the measured commit locks release 0.3.4 (see the `Cargo.lock` lines in the environment table). |
| `GuardedPathMap` | The owner type in [`guarded_pathmap.rs`](../../../../rspace++/src/rspace/guarded_pathmap.rs) that wraps one PathMap trie and tears it down without recursion. |
| `AnchorRoot` drop | The function `<AnchorRoot<V, D, B> as Drop>::drop` for value type `V`, disposer `D` and barrier type `B`; it runs the two-phase teardown of the last owner. |
| two-phase teardown | Phase A removes and disposes every value in byte preorder; phase B removes the value-free topology that phase A leaves. Before commit `6642c0d13`, phase B installed a synthetic barrier value at the parent of each edge and pruned one edge per step. |
| barrier-free cut | Phase B since commit `6642c0d13`: at a value-free parent with exactly one child, `remove_branches(false)` detaches the edge to that child and the walk climbs on; at the root, at a parent that holds a value or at a parent with more than one child, `prune_ascend` removes exactly one byte. No barrier value is installed. |
| `split_0` | `pathmap::line_list_node::LineListNode::split_0`, which allocates a new trie node when a compressed line-list node is split, for example to hold a barrier value. |
| final owner | The `GuardedPathMap<u64, MeasurementDisposer, MeasurementBarrier>` that the measured test builds and then drops inside its timer. |
| donor | A value-free `GuardedPathMap<V, ValueFreeDisposer, B>` that `materialize_missing_suffix` in [`pathmap_mutation.rs`](../../../../rspace++/src/rspace/pathmap_mutation.rs) builds, grafts into a trie under construction and then drops; dropping it runs its own `AnchorRoot` drop while the measured trie is still being built. |
| Heaptrack | The heap profiler (version 1.5.0) that intercepts every call to an allocation function; `heaptrack --record-only` records a run and `heaptrack_print` renders the recording as text. |
| allocation call | One call to an allocation function (for example `malloc` or `realloc`) as counted by Heaptrack. |
| temporary allocation | An allocation that Heaptrack classifies as freed at once. Heaptrack prints two such counters: the `heaptrack_print` report line `temporary memory allocations` and the end-of-run summary line `temporary allocations` printed when the recorded process exits. They differ and are reported separately. |
| peak heap | The `heaptrack_print` line `peak heap memory consumption`, copied as printed. |
| RSS | Resident set size: the physical memory of a process; peak RSS is its maximum. |
| folded stack | One line of `heaptrack_print -F <file> --flamegraph-cost-type allocations`: the frames of one call stack joined by `;`, root first, each frame written as a symbol and a source file name, followed by its number of allocation calls. |
| independent recount | The remedy study's script `41-recount.py`, which sums the same folded stacks by matching the Rust v0-mangled symbols directly, without the demangler and without any code of the extraction script. |
| TSV | Tab-separated values, the format of the record's evidence table file. |
| SHA-256 | Secure Hash Algorithm 2 with a 256-bit digest (SHA-256), used to fingerprint every file. |
| load average | The three exponentially damped run-queue averages over 1, 5 and 15 minutes from the kernel's `loadavg` file. |
| median | The middle value of an odd number of sorted observations. |
| percentile bootstrap | An interval for a statistic obtained by recomputing it on many samples drawn with replacement from the observations and taking two quantiles of the recomputed values (Efron's bootstrap, see the references). |
| SplitMix64 | The pseudorandom generator of Steele, Lea and Flood (see the references) that drew the bootstrap samples. |

A run $`x`$ is one execution of a test binary under one configuration; runs are named by measurement, shape and size, for example $`x = (\mathrm{realistic}, 256)`$. For a run $`x`$, let $`S(x)`$ be the set of distinct folded-stack strings (identical strings merged), $`c_x(s)`$ the allocation calls of $`s \in S(x)`$, $`D`$ the set of frames whose demangled name is an `AnchorRoot` drop (any generic arguments), $`f_{\mathrm{split}}`$ the frame of `split_0`, $`f_T`$ the frame of the test function `final_anchor_teardown_measurement`, and $`f_M`$ the frame of its construction helper `measurement_map`. The protocol's attribution, its `split_0` subset and its cross-check are

```math
A_{\mathrm{drop}}(x) = \sum_{s \in S(x),\ s \cap D \neq \emptyset} c_x(s),
\qquad
N_{\mathrm{split}}(x) = \sum_{s \in S(x),\ s \cap D \neq \emptyset,\ f_{\mathrm{split}} \in s} c_x(s),
\qquad
C(x) = \sum_{s \in S(x),\ f_T \in s,\ f_M \notin s} c_x(s).
```

The relative difference between the cross-check and the attribution is

```math
\delta(x) = \frac{100 \cdot \lvert C(x) - A_{\mathrm{drop}}(x) \rvert}{A_{\mathrm{drop}}(x)}.
```

The leaf's acceptance is the conjunction of three predicates. The thresholds of $`P_1`$ and $`P_2`$ are those that experiment 488 registered (one tenth of the 181,708 calls measured on PathMap 0.2.2, and one tenth of one call per topology byte of the line); each is less than one tenth of the 2026-11-12 figure at its coordinate, which is 18,448.9 and 3,248.9.

```math
P_1 :\ A_{\mathrm{drop}}(\mathrm{realistic}, 256) \le 18170,
\qquad
P_2 :\ A_{\mathrm{drop}}(\mathrm{line}, 16384) \le 1638,
\qquad
P_3 :\ N_{\mathrm{split}}(\mathrm{realistic}, 256) = N_{\mathrm{split}}(\mathrm{line}, 16384) = 0.
```

For a shape $`s`$ and a tree $`t`$, either the commit before the cut or the measured commit, let $`\tilde{t}_{s,t}`$ be the median of the observed times of one final teardown. The comparison's statistic and its interval are

```math
\rho_s = \frac{\tilde{t}_{s,\mathrm{fixed}}}{\tilde{t}_{s,\mathrm{before}}},
\qquad
I_s = \left[\rho^{*}_{s,(\lceil 0.025 B \rceil)},\ \rho^{*}_{s,(\lceil 0.975 B \rceil)}\right],
```

where $`\rho^{*}_{s,(1)} \le \dots \le \rho^{*}_{s,(B)}`$ are the ratios of medians of $`B`$ bootstrap resamples in ascending order; a resample draws, for each tree independently, as many observations with replacement as the tree has. $`I_s`$ is the 95 % percentile interval. The registered conclusion rule: the teardown of the measured commit is faster when the upper end of $`I_s`$ is below 1, slower when the lower end is above 1, and no conclusion can be drawn when $`I_s`$ contains 1.

## Evidence files and citation format

Every measured or derived value in the tables below is followed by a citation `alias:line`, naming the file by its alias and the 1-based line on which the value appears; numbers in row labels, such as run sizes, are not values and carry no citation. The raw files sit beside this record and share its stem. Before their SHA-256 was computed, every absolute path in them was rewritten: the repository root became `[WORKSPACE_ROOT]`, the home directory became `[HOME]`, the user name became `[USER]`, and any other absolute path received the prefix `[ROOT]` (so a system library path reads `[ROOT]/usr/lib/libc.so.6`). The work directory of the measurement is `[WORKSPACE_ROOT]/target/scratch/session-401d0c58/c45/l14365/w`; its `src` directory held the plain export of the measured commit and its `src-before` directory that of the commit before the cut.

| Alias | File | Content | SHA-256 |
|---|---|---|---|
| `rep-m1` | [M1 report](guarded-pathmap-final-teardown-barrier-free-2026-11-14-m1-heaptrack-report.txt) | `heaptrack_print -t 0 -n 30` report of M1 | `fd21d3b581d348c4fea49f14731ad45d18a2a4da3ffea5d7dcdd0ecb4c50ef5f` |
| `rep-m2` | [M2 report](guarded-pathmap-final-teardown-barrier-free-2026-11-14-m2-heaptrack-report.txt) | `heaptrack_print -t 0 -n 30` report of M2 | `5427e2c478fcf40e8c8edc8f9a72eb8ccbcc969eeb0dd81c101c1bbff94a925a` |
| `rep-m5` | [M5 report](guarded-pathmap-final-teardown-barrier-free-2026-11-14-m5-heaptrack-report.txt) | `heaptrack_print -t 0 -n 30` report of M5 | `aebacb5b8754c4313170c2f335d75546569ea7d05fa8ee5a05252f604464645d` |
| `tsv` | [allocation table](guarded-pathmap-final-teardown-barrier-free-2026-11-14-allocations.tsv) | every run's totals, attribution, cross-check, recount and twenty heaviest attributed stacks, the derived figures, the comparison's observations and statistics, the environment and the recordings | `b17769a969be36cfa25bc0c7ba57b40f5c030a861c924779a883b323e5f3ff63` |
| `time-m5` | [M5 wall time](guarded-pathmap-final-teardown-barrier-free-2026-11-14-m5-wall-time.txt) | the unprofiled M5 run under GNU `time -v` | `46e877edfbc3d5ecc46485b59afd6d850b5226064cd775bf6531f2cb9454f7f2` |
| `obs-td` | [teardown comparison](guarded-pathmap-final-teardown-barrier-free-2026-11-14-teardown-comparison.txt) | the comparison's run log: each run's order, load average, processor frequency and `TEARDOWN_OBS` line | `5d442b2b93e8ba29f3f0fb900cc64f7c3cfe61552b33644479b71ef0d35212cb` |
| `ref-1112` | [record of 2026-11-12](guarded-pathmap-final-teardown-pathmap-0.3.4-2026-11-12.md) | the figures before the cut | committed earlier |
| `study` | [remedy study](../guarded-pathmap-teardown-allocation-remedy-study.md) | remedy R2a and the figures of its prototype | committed earlier |

## Environment

| Item | Value | Source |
|---|---|---|
| measured commit | `f6afb445e3fe56af479ac5404b9823995df818ee` | `tsv:573` |
| commit before the cut | `f7c88aa914fb521f4e805f600fbed21b148405a9` | `tsv:576` |
| commit of the cut | `6642c0d13f2efbca38d92738886d809fb507c5fb` | `tsv:575` |
| `pathmap` entry of [`Cargo.lock`](../../../../Cargo.lock), lines 4068 and 4069 | `name = "pathmap"`, `version = "0.3.4"` | `tsv:577`, `tsv:578` |
| `pathmap` line of `rspace++/Cargo.toml`, line 77 | `pathmap = { version = "=0.3.4", features = ["arena_compact"] }` | `tsv:579` |
| `rustc --version` | `rustc 1.95.0-nightly (6efa357bf 2026-02-08)` | `tsv:580` |
| `cargo --version` | `cargo 1.95.0-nightly (fe2f314ae 2026-01-30)` | `tsv:581` |
| Heaptrack | `heaptrack 1.5.0` | `tsv:582` |
| `heaptrack_print --version` | `heaptrack_print 1.5.0` | `tsv:583` |
| symbol demangler `rustfilt --version` | `rust_demangle 0.2.1` | `tsv:584` |
| first heading line of the hardware description | `# Hardware Specifications` | `tsv:585` |
| processor | `AMD Ryzen Threadripper PRO 5975WX 32-Cores` | `tsv:586` |
| logical processors | 32 | `tsv:587` |
| threads per core | 1 | `tsv:588` |
| kernel | `Linux 7.2.15-arch1-1` | `tsv:590` |
| `kernel.perf_event_paranoid` | 2 | `tsv:591` |
| timing processor | 27 | `tsv:592` |
| thread siblings of the timing processor | 27 | `tsv:593` |
| frequency governor of the timing processor | `performance` | `tsv:594` |
| highest frequency of the timing processor, kHz | 4561833 | `tsv:595` |
| frequency ceiling of the timing processor, kHz | 4561833 | `tsv:596` |
| frequency boost | `enabled` | `tsv:589` |
| build setting | `CARGO_PROFILE_RELEASE_DEBUG=true`: the release profile with debug information | `tsv:638` |
| test binary of the measured commit `rspace_plus_plus-0e4696c61247f2bd`, SHA-256 | `986f30d3243217e9171c3da1d5f9f149cbd723fdded6450657061ff1da8ee03c` | `tsv:643` |
| test binary of the measured commit `guarded_matching_tests-8777c1ec11857ea6`, SHA-256 | `17097ebd3d42e186f2d6cde1cff29af330935846a104c52f7e31b0703ade2763` | `tsv:644` |
| library test binary of the commit before the cut `rspace_plus_plus-0e4696c61247f2bd`, SHA-256 | `1144226bf1f842644e8c9ece99e3a6ef1627203e2772b208776ed5dbd2052e11` | `tsv:645` |
| extraction script `40-analyse-heaptrack.py`, SHA-256 | `2f94c122147bf5e700ff2a86787b6bc6beef5d3d487a9ec238e91e5b43757042` | `tsv:640` |
| independent recount `41-recount.py`, SHA-256 | `a17c3b4360666f1778c35075ca1115aa344bc374084346f3846696aed6cd8a48` | `tsv:641` |
| checker `check-record.py`, SHA-256 | `23dff077050deb4a38d8de515bd6a7e9f23522eb61b343b801498fc8a70d1c5b` | `tsv:642` |

The host is shared with other work, so the load average was read before and after every run. All times are UTC on 2026-11-14; the load averages are the 1, 5 and 15 minute values.

| Run | Measurement | Start | Source | Load average before | Source | Load average after | Source |
|---|---|---|---|---|---|---|---|
| `m4-realistic-0` | M4 | 08:47:01 | `tsv:196` | 10.85 7.03 4.91 | `tsv:194` | 10.85 7.03 4.91 | `tsv:195` |
| `m1-realistic-256` | M1 | 08:47:03 | `tsv:203` | 10.85 7.03 4.91 | `tsv:201` | 10.85 7.03 4.91 | `tsv:202` |
| `m2-line-16384` | M2 | 08:47:04 | `tsv:210` | 10.85 7.03 4.91 | `tsv:208` | 10.85 7.03 4.91 | `tsv:209` |
| `m5-guarded-matching-20000` | M5, Heaptrack | 08:47:05 | `tsv:217` | 10.85 7.03 4.91 | `tsv:215` | 9.91 7.01 4.94 | `tsv:216` |
| `m5-wall-time` | M5, unprofiled | 08:47:18 | `tsv:273` | 9.91 7.01 4.94 | `tsv:271` | 9.60 6.99 4.95 | `tsv:272` |

The wall-time comparison ran from 08:47:25 (`obs-td:2`) to 08:47:34 (`obs-td:2320`); the 1-minute load average read before and after its runs lay between 9.60 (`tsv:289`) and 10.11 (`tsv:290`), and every run's values are in the comparison's run log.

## Method

### Builds and isolation

The executables were built in the exports with `--locked --offline -j 16` (`tsv:639`). The workspace manifest has no `[profile.release]` section, so the default release profile carries no debug information, and Heaptrack can name inlined frames only from debug information. The environment setting `CARGO_PROFILE_RELEASE_DEBUG=true` adds debug information to the release profile without editing any file, as in the 2026-11-12 record. Every build and every run went through the campaign wrapper `heavy.sh`, which starts the command in a `systemd-run --user --scope` unit with `MemoryMax=32G` and `MemorySwapMax=0` and pins it with `taskset` to the campaign CPUs, which on this 32-CPU host are CPUs 16 to 31. Every timed run was further pinned to CPU 27.

The two exports are one workspace at two paths and shared one target directory. Cargo names a workspace member's artifacts by its path relative to the workspace root, so both builds wrote executables of the same names, and it decides freshness from modification times. Each export was therefore extracted with the extraction time as the modification time of every file, so that the second build recompiled every workspace member it needed, and each executable was copied aside right after its build. The copied library executable of the measured commit contains the name of the function `cut_value_free_leaf_and_bubble`, which the cut added, on 4 lines (`tsv:646`); that of the commit before the cut contains it on 0 (`tsv:647`).

The stock Heaptrack 1.5.0 wrapper script creates its named pipe under `[ROOT]/tmp`, which this host forbids. The runs used a copy of the stock script with exactly two lines changed, made by the 2026-11-12 record's script: the line `pipe=[ROOT]/tmp/heaptrack_fifo$$` became `pipe="${TMPDIR:?}/heaptrack_fifo$$"`, and the line `EXE_PATH=$(readlink -f "$SCRIPT_DIR")` became an assignment of the stock installation directory `[ROOT]/usr/bin`, because the copy no longer sits beside the stock script. The preload library, the interpreter and every flag are the stock ones. Recording ran with `DEBUGINFOD_URLS` unset, so symbol resolution used only local debug information.

### Runs

M1, M2 and M4 run the ignored library test `final_anchor_teardown_measurement` of [`two_phase_teardown_properties.rs`](../../../../rspace++/tests/guarded_pathmap/two_phase_teardown_properties.rs), exact, with one test thread and one trial, under `heaptrack --record-only`. The test builds the trie with `measurement_map` and then drops it inside its timer, printing one `TEARDOWN_OBS` line per trial. The `realistic` shape stores one value under each of `RSPACE_TEARDOWN_SIZE` keys of 939 bytes that share a 203-byte prefix. The `line` shape creates one value-free path of `RSPACE_TEARDOWN_SIZE` bytes with `create_path`. M5 runs `selector_and_enumerator_are_stack_safe_at_twenty_thousand_binds` of [`guarded_matching_tests.rs`](../../../../rspace++/tests/guarded_matching_tests.rs) under Heaptrack and once more, unprofiled, under GNU `time -v`. The wall-time comparison runs the same library test of both commits without a profiler.

Every recording was rendered once by one `heaptrack_print` invocation that wrote both the report and the folded stacks. The option `-t 0` turns off template shortening, which would otherwise replace the generic arguments of `<AnchorRoot<V, D, B> as Drop>::drop` (the whole `Self` type of the frame) by an ellipsis.

### Attribution

The symbols of the pinned nightly toolchain are mangled in the Rust v0 scheme, which `heaptrack_print` leaves mangled, so the extraction script demangled every frame with `rustfilt` before matching. No mangled symbol remained after demangling.

**Algorithm 1 (Folded-stack attribution).**

```pseudocode
input:  the folded-stack lines F of one run x, and the demangler
output: A_drop(x), N_split(x), C(x), A_drop(x) per AnchorRoot instantiation, the twenty heaviest A_drop stacks

merged := empty map from stack string to allocation calls
for each line in F:
    split the line at its last space into (stack, cost)
    merged[stack] := merged[stack] + cost              -- identical stack strings are merged
A := 0;  N := 0;  C := 0;  per_instantiation := empty map;  heavy := empty list
for each (stack, cost) in merged:
    names := [ demangle(symbol of frame) for each frame of stack split at ";" ]
    drops := { generic arguments g : some name equals
               "<rspace_plus_plus::rspace::guarded_pathmap::AnchorRoot<g> as core::ops::drop::Drop>::drop" }
    if drops is not empty:
        A := A + cost
        if some name is <pathmap::line_list_node::LineListNode<..>>::split_0: N := N + cost
        for each g in drops: per_instantiation[g] := per_instantiation[g] + cost
        append (cost, stack) to heavy
    if f_T is in names and f_M is not in names:
        C := C + cost
sort heavy by decreasing cost and keep its first twenty entries
return A, N, C, per_instantiation, heavy
```

A stack counts once toward $`A_{\mathrm{drop}}(x)`$ however many `AnchorRoot` drop frames it contains, so the sum is exactly the number of allocation calls made while some `AnchorRoot` drop was on the stack, and $`N_{\mathrm{split}}(x)`$ is the part of it whose stack also passes through `split_0`. The same loop records, for each attributed stack, whether `measurement_map` is on it and the call sites that explain an attributed stack inside `measurement_map`; all of these appear as rows of the `tsv` file. The cost sums over all folded stacks equal `heaptrack_print`'s total allocation calls in every run (rows `stack_cost_sum` of the `tsv` file).

The independent recount reads the same folded-stack files and shares no code with the extraction script: it counts a stack toward $`A_{\mathrm{drop}}`$ when one of its frames is the v0-mangled symbol of a `Drop::drop` implementation whose self type is `guarded_pathmap::AnchorRoot` itself, and toward $`N_{\mathrm{split}}`$ when such a stack also has a frame whose mangled symbol ends in `split_0`. Its results are rows `recount_*` of the `tsv` file.

The following diagram shows how the allocation calls of one library-test run fall into the two attributions.

```text
 all allocation calls of one library-test run
 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │ stacks through final_anchor_teardown_measurement (the test function)            │
 │ ┌───────────────────────────────────────┐ ┌───────────────────────────────────┐ │
 │ │ through measurement_map: construction │ │ not through measurement_map       │ │
 │ │ ┌───────────────────────────────────┐ │ │ ┌───────────────────────────────┐ │ │
 │ │ │ donor AnchorRoot drop        [A]  │ │ │ │ final-owner AnchorRoot drop   │ │ │
 │ │ │ (ValueFreeDisposer)               │ │ │ │ (MeasurementDisposer)  [A][C] │ │ │
 │ │ └───────────────────────────────────┘ │ │ └───────────────────────────────┘ │ │
 │ │ other construction calls              │ │ environment read (getenv)  [C]    │ │
 │ └───────────────────────────────────────┘ └───────────────────────────────────┘ │
 └─────────────────────────────────────────────────────────────────────────────────┘
 stacks outside the test function: test harness and process start-up
```

Legend: each box is a set of folded stacks, nested boxes are subsets, and the label `[A]` marks the stacks counted by $`A_{\mathrm{drop}}`$ while `[C]` marks those counted by the cross-check $`C`$. Hence $`A_{\mathrm{drop}}`$ is the final-owner part plus the donor part, and $`C`$ is the final-owner part plus the environment reads of the test; the cross-check section below tests both identities in every library-test run.

### Wall-time comparison

The comparison follows the protocol registered as pgmcp experiment 498 before any of its runs. It ran 64 rounds (`tsv:285`); the leaf asks for at least 30 trials per commit and shape (`tsv:286`).

**Algorithm 2 (Interleaved teardown timing).**

```pseudocode
input:  the library test executables E_before and E_fixed, the number of rounds n, the timing processor p
output: n observed teardown times per commit and shape

for r := 1 to n:
    order := (before, fixed) if r is odd, else (fixed, before)
    for shape in ((realistic, 256), (line, 16384)):
        for t in order:
            read the load average and the current frequency of p
            run E_t pinned to p with RSPACE_TEARDOWN_TRIALS=1:
                a fresh process builds the shape with measurement_map,
                times the drop of the final owner with Instant and prints one TEARDOWN_OBS line
            read the load average and the current frequency of p again
            record (t, shape, r, elapsed_ns of the TEARDOWN_OBS line)
```

Each run is one observation, so every commit and shape receives one observation per round. The alternating order puts each commit first in half of the rounds, so a drift of the host's state during the comparison does not favour either commit. All rounds ran inside one `heavy.sh` scope. The analysis computes the medians and $`\rho_s`$ exactly, and draws 10,000 bootstrap resamples per shape (`tsv:293`) from SplitMix64 seeded with 14365 (`tsv:294`), the realistic shape first, mapping each draw to an observation index by rejection so that every index is equally likely. The interval's ends are the resampled ratios of rank 250 (`tsv:298`) and 9,750 (`tsv:299`).

## Exact commands

The commands below use the same placeholders as the raw files. They are the commands of the scripts that produced the logs, in the order in which they ran.

```bash
# Placeholders as in the raw files: [WORKSPACE_ROOT] is the repository root, [ROOT] the file-system root.
W='[WORKSPACE_ROOT]/target/scratch/session-401d0c58/c45/l14365/w'
HEAVY='[WORKSPACE_ROOT]/target/scratch/session-401d0c58/scripts/heavy.sh'
export TMPDIR="$W/tmp" TEMP="$W/tmp" TMP="$W/tmp"
export CARGO_TARGET_DIR="$W/target" CARGO_PROFILE_RELEASE_DEBUG=true

# Export with fresh modification times, build, and copy each executable right after its build.
git -C '[WORKSPACE_ROOT]' archive --format=tar f6afb445e3fe56af479ac5404b9823995df818ee | tar -x -m -C "$W/src"
cd "$W/src"
"$HEAVY" cargo test --release --locked --offline -p rspace_plus_plus --lib --no-run -j 16
cp "$W/target/release/deps/rspace_plus_plus-0e4696c61247f2bd" "$W/bin/fixed/"
"$HEAVY" cargo test --release --locked --offline -p rspace_plus_plus --test guarded_matching_tests --no-run -j 16
cp "$W/target/release/deps/guarded_matching_tests-8777c1ec11857ea6" "$W/bin/fixed/"
git -C '[WORKSPACE_ROOT]' archive --format=tar f7c88aa914fb521f4e805f600fbed21b148405a9 | tar -x -m -C "$W/src-before"
cd "$W/src-before"
"$HEAVY" cargo test --release --locked --offline -p rspace_plus_plus --lib --no-run -j 16
cp "$W/target/release/deps/rspace_plus_plus-0e4696c61247f2bd" "$W/bin/before/"

LIB="$W/bin/fixed/rspace_plus_plus-0e4696c61247f2bd"
GM="$W/bin/fixed/guarded_matching_tests-8777c1ec11857ea6"
LIB_BEFORE="$W/bin/before/rspace_plus_plus-0e4696c61247f2bd"
TEST='rspace::guarded_pathmap::two_phase_teardown_properties::final_anchor_teardown_measurement'
GM_TEST='selector_and_enumerator_are_stack_safe_at_twenty_thousand_binds'
HT="$W/out/scripts/heaptrack-1.5.0-fifo-in-tmpdir.sh"
REC="$W/out/recordings"
LOGS="$W/out/logs"

# Exact test names, taken from the executables' own lists.
cd "$W/src/rspace++"
"$HEAVY" "$LIB" --list --ignored
"$HEAVY" "$GM" --list

# Heaptrack recordings: M4, M1, M2, then M5.
"$HEAVY" env -u DEBUGINFOD_URLS RSPACE_TEARDOWN_SHAPE=realistic RSPACE_TEARDOWN_SIZE=0 RSPACE_TEARDOWN_TRIALS=1 \
  "$HT" --record-only -o "$REC/heaptrack-m4-realistic-0" "$LIB" --exact "$TEST" --ignored --test-threads=1 --nocapture
"$HEAVY" env -u DEBUGINFOD_URLS RSPACE_TEARDOWN_SHAPE=realistic RSPACE_TEARDOWN_SIZE=256 RSPACE_TEARDOWN_TRIALS=1 \
  "$HT" --record-only -o "$REC/heaptrack-m1-realistic-256" "$LIB" --exact "$TEST" --ignored --test-threads=1 --nocapture
"$HEAVY" env -u DEBUGINFOD_URLS RSPACE_TEARDOWN_SHAPE=line RSPACE_TEARDOWN_SIZE=16384 RSPACE_TEARDOWN_TRIALS=1 \
  "$HT" --record-only -o "$REC/heaptrack-m2-line-16384" "$LIB" --exact "$TEST" --ignored --test-threads=1 --nocapture
"$HEAVY" env -u DEBUGINFOD_URLS \
  "$HT" --record-only -o "$REC/heaptrack-m5-guarded-matching-20000" "$GM" --exact "$GM_TEST" --test-threads=1 --nocapture

# Each recording was rendered right after it was made; shown for M1, the others differ only in the run name.
"$HEAVY" '[ROOT]/usr/bin/heaptrack_print' -f "$REC/heaptrack-m1-realistic-256.zst" -t 0 -n 30 \
  -F "$LOGS/heaptrack-m1-realistic-256.stacks.txt" --flamegraph-cost-type allocations \
  > "$LOGS/heaptrack-m1-realistic-256.report.txt"

# The 2026-11-12 record's extraction script, then the remedy study's independent recount.
python3 -I "$W/out/scripts/40-analyse-heaptrack.py" "$LOGS" rustfilt "$LOGS/40-analysis.json" "$LOGS/40-adrop.tsv" \
  m4-realistic-0:lib m1-realistic-256:lib m2-line-16384:lib m5-guarded-matching-20000:gm
python3 -I "$W/out/scripts/41-recount.py" "$LOGS" m4-realistic-0 m1-realistic-256 m2-line-16384 m5-guarded-matching-20000

# M5 run time, pinned, with the load average read before and after.
cat '[ROOT]/proc/loadavg'
"$HEAVY" taskset -c 27 '[ROOT]/usr/bin/time' -v "$GM" --exact "$GM_TEST" --test-threads=1 --nocapture
cat '[ROOT]/proc/loadavg'

# The wall-time comparison of experiment 498: Algorithm 2 in one heavy.sh scope.
"$HEAVY" bash "$W/out/scripts/56-comparison-driver.sh" 64 27 "$LIB_BEFORE" "$LIB" "$TEST"
```

The Heaptrack recordings are not committed: they are reproducible with these commands, and their sizes and SHA-256 are listed at the end of this record.

## Results

### Allocation counts before and after the cut

The 2026-11-12 values are copied as written in that record, which measured commit `1bbd43283` before the cut; see the limitations for the other changes between the two commits.

| Measurement | Metric | 2026-11-12 | Source | This record | Source |
|---|---|---|---|---|---|
| M1 realistic 256 | allocation calls | 190,832 | `ref-1112:345` | 8,165 | `rep-m1:4886` |
| M1 realistic 256 | temporary allocations, `heaptrack_print` counter | 178,833 | `ref-1112:346` | 1,103 | `rep-m1:4887` |
| M1 realistic 256 | temporary allocations, end-of-run counter | 1,102 | `ref-1112:347` | 1,102 | `tsv:200` |
| M1 realistic 256 | peak heap | 403.75K | `ref-1112:348` | 403.30K | `rep-m1:4888` |
| M1 realistic 256 | peak RSS including Heaptrack overhead | 15.28M | `ref-1112:349` | 16.58M | `rep-m1:4889` |
| M1 realistic 256 | $`A_{\mathrm{drop}}`$ | 184,489 | `ref-1112:350` | 1,866 | `tsv:22` |
| M1 realistic 256 | $`A_{\mathrm{drop}}`$, final-owner instantiation | 183,522 | `ref-1112:351` | 1,814 | `tsv:241` |
| M1 realistic 256 | $`A_{\mathrm{drop}}`$, donor instantiation | 967 | `ref-1112:352` | 52 | `tsv:242` |
| M1 realistic 256 | cross-check $`C`$ | 183,525 | `ref-1112:353` | 1,817 | `tsv:32` |
| M1 realistic 256 | $`N_{\mathrm{split}}`$: calls in $`A_{\mathrm{drop}}`$ whose stack also contains `split_0` | 182,623 | `ref-1112:354` | 0 | `tsv:28` |
| M1 realistic 256 | heaviest single stack under the `AnchorRoot` drop | 181,708 | `ref-1112:355` | 1,024 | `tsv:29` |
| M2 line 16384 | allocation calls | 33,602 | `ref-1112:356` | 1,574 | `rep-m2:4271` |
| M2 line 16384 | temporary allocations, `heaptrack_print` counter | 31,283 | `ref-1112:357` | 82 | `rep-m2:4272` |
| M2 line 16384 | temporary allocations, end-of-run counter | 81 | `ref-1112:358` | 81 | `tsv:207` |
| M2 line 16384 | peak heap | 252.20K | `ref-1112:359` | 269.49K | `rep-m2:4273` |
| M2 line 16384 | peak RSS including Heaptrack overhead | 13.88M | `ref-1112:360` | 16.30M | `rep-m2:4274` |
| M2 line 16384 | $`A_{\mathrm{drop}}`$ | 32,489 | `ref-1112:361` | 505 | `tsv:81` |
| M2 line 16384 | $`A_{\mathrm{drop}}`$, final-owner instantiation | 16,049 | `ref-1112:362` | 57 | `tsv:247` |
| M2 line 16384 | $`A_{\mathrm{drop}}`$, donor instantiation | 16,440 | `ref-1112:363` | 448 | `tsv:248` |
| M2 line 16384 | cross-check $`C`$ | 16,052 | `ref-1112:364` | 60 | `tsv:91` |
| M2 line 16384 | $`N_{\mathrm{split}}`$: calls in $`A_{\mathrm{drop}}`$ whose stack also contains `split_0` | 31,984 | `ref-1112:365` | 0 | `tsv:87` |
| M2 line 16384 | heaviest single stack under the `AnchorRoot` drop | 15,992 | `ref-1112:366` | 390 | `tsv:88` |

### Acceptance

| Predicate | Value | Source | Threshold | Holds |
|---|---|---|---|---|
| $`P_1`$: $`A_{\mathrm{drop}}(\mathrm{realistic}, 256) \le 18170`$ | 1,866 | `tsv:254` | 18,170 | yes |
| $`P_2`$: $`A_{\mathrm{drop}}(\mathrm{line}, 16384) \le 1638`$ | 505 | `tsv:255` | 1,638 | yes |
| $`P_3`$, realistic: $`N_{\mathrm{split}}(\mathrm{realistic}, 256) = 0`$ | 0 | `tsv:256` | 0 | yes |
| $`P_3`$, line: $`N_{\mathrm{split}}(\mathrm{line}, 16384) = 0`$ | 0 | `tsv:257` | 0 | yes |

All three predicates hold (`tsv:258`): no `split_0` call remains beneath the final teardown, and $`A_{\mathrm{drop}}`$ is 98.9 times smaller at the realistic coordinate (`tsv:261`) and 64.3 times smaller at the line coordinate (`tsv:264`) than in the 2026-11-12 record.

### M4: negative control of the attribution

| Quantity for `realistic` size 0 | Value | Source |
|---|---|---|
| allocation calls | 628 | `tsv:2` |
| $`A_{\mathrm{drop}}`$ | 0 | `tsv:7` |
| cross-check $`C`$ | 3 | `tsv:14` |

$`A_{\mathrm{drop}}(\mathrm{realistic}, 0) = 0 \le 16`$, so the attribution passes its negative control (`tsv:253`).

### Cross-check

| Run | $`A_{\mathrm{drop}}`$ | Source | $`C`$ | Source | $`C - A_{\mathrm{drop}}`$ | Source | $`\delta`$ in % | Source |
|---|---|---|---|---|---|---|---|---|
| M1 realistic 256 | 1,866 | `tsv:22` | 1,817 | `tsv:32` | -49 | `tsv:33` | 2.626 | `tsv:34` |
| M2 line 16384 | 505 | `tsv:81` | 60 | `tsv:91` | -445 | `tsv:92` | 88.119 | `tsv:93` |

| Run | Final owner | Source | Donor | Source | Calls of $`C`$ without an `AnchorRoot` drop | Source | $`A_{\mathrm{drop}}`$ = final + donor | Source | $`C`$ = final + calls without a drop | Source |
|---|---|---|---|---|---|---|---|---|---|---|
| M4 realistic 0 | 0 | `tsv:235` | 0 | `tsv:236` | 3 | `tsv:238` | true | `tsv:239` | true | `tsv:240` |
| M1 realistic 256 | 1,814 | `tsv:241` | 52 | `tsv:242` | 3 | `tsv:244` | true | `tsv:245` | true | `tsv:246` |
| M2 line 16384 | 57 | `tsv:247` | 448 | `tsv:248` | 3 | `tsv:250` | true | `tsv:251` | true | `tsv:252` |

In every library-test run, $`A_{\mathrm{drop}}`$ equals the final owner's part plus the donors' part, and $`C`$ equals the final owner's part plus the calls of the test function that run under no `AnchorRoot` drop and outside `measurement_map`; the last frames of each of those stacks name `getenv`, the test's reads of its environment variables (rows `cross_check_line_without_anchor_drop` of the `tsv` file).

### Independent recount

| Run | $`A_{\mathrm{drop}}`$, extraction | Source | $`A_{\mathrm{drop}}`$, recount | Source | $`N_{\mathrm{split}}`$, extraction | Source | $`N_{\mathrm{split}}`$, recount | Source | Equal | Source |
|---|---|---|---|---|---|---|---|---|---|---|
| M4 realistic 0 | 0 | `tsv:7` | 0 | `tsv:220` | 0 | `tsv:10` | 0 | `tsv:221` | yes | `tsv:222` |
| M1 realistic 256 | 1,866 | `tsv:22` | 1,866 | `tsv:224` | 0 | `tsv:28` | 0 | `tsv:225` | yes | `tsv:226` |
| M2 line 16384 | 505 | `tsv:81` | 505 | `tsv:228` | 0 | `tsv:87` | 0 | `tsv:229` | yes | `tsv:230` |
| M5 20,000 binds | 4,599,923 | `tsv:140` | 4,599,923 | `tsv:232` | 0 | `tsv:149` | 0 | `tsv:233` | yes | `tsv:234` |

The recount gives the extraction's total, $`A_{\mathrm{drop}}`$ and $`N_{\mathrm{split}}`$ in every run.

### M5: the 20,000-bind guarded matching test

M5 sizes the defect and has no threshold. Its three figures beside the 2026-11-12 record's:

| Figure | 2026-11-12 | Source | This record | Source | Ratio | Source |
|---|---|---|---|---|---|---|
| all allocation calls | 56,666,258 | `ref-1112:367` | 12,744,222 | `tsv:275` | 0.22490 | `tsv:277` |
| calls beneath an `AnchorRoot` drop, $`A_{\mathrm{drop}}`$ | 48,461,906 | `ref-1112:368` | 4,599,923 | `tsv:278` | 0.09492 | `tsv:280` |
| run time, unprofiled, seconds | 13.18 | `ref-1112:378` | 6.55 | `tsv:281` | 0.49697 | `tsv:283` |

The run time is GNU `time`'s elapsed wall-clock time of one unprofiled run (`time-m5:17`, `tsv:267`); its peak RSS was 173,456 KiB (`time-m5:22`).

The M5 attribution splits over the following `AnchorRoot` instantiations, named by value type and disposer:

| Value type | Disposer | Allocation calls | Source |
|---|---|---|---|
| `Multiplicity` | `ValueFreeDisposer` | 1,800,040 | `tsv:141` |
| `Multiplicity` | `MultiplicityDisposer` | 1,599,834 | `tsv:142` |
| `CandidatePosition` | `ValueFreeDisposer` | 600,000 | `tsv:143` |
| `CandidatePosition` | `CandidatePositionDisposer` | 480,000 | `tsv:144` |
| `ChannelSlot` | `ChannelSlotDisposer` | 120,033 | `tsv:145` |
| `ChannelSlot` | `ValueFreeDisposer` | 16 | `tsv:146` |

### Wall time of one final teardown

| Shape | Median before the cut, ms | Source | Median of the measured commit, ms | Source | $`\rho_s`$ | Source | 95 % interval $`I_s`$ | Source | Conclusion | Source |
|---|---|---|---|---|---|---|---|---|---|---|
| realistic 256 | 55.412171 | `tsv:365` | 25.965803 | `tsv:431` | 0.468594 | `tsv:432` | 0.467871 to 0.469453 | `tsv:433`, `tsv:434` | measured commit faster: the whole interval lies below 1 | `tsv:435` |
| line 16384 | 4.569038 | `tsv:501` | 2.003378 | `tsv:567` | 0.438468 | `tsv:568` | 0.436725 to 0.439468 | `tsv:569`, `tsv:570` | measured commit faster: the whole interval lies below 1 | `tsv:571` |

Every run passed and every commit and shape has the registered number of observations (`tsv:572`). By the registered rule, at the realistic coordinate the interval lies below 1, so a conclusion can be drawn: one final teardown of the measured commit is faster than before the cut; at the line coordinate the interval lies below 1, so a conclusion can be drawn: one final teardown of the measured commit is faster than before the cut. The host was shared, so these are statements about this host under the load listed above.

The observations behind the medians, in nanoseconds:

| Round | Realistic 256, before the cut | Source | Realistic 256, measured commit | Source | Line 16384, before the cut | Source | Line 16384, measured commit | Source |
|---|---|---|---|---|---|---|---|---|
| 1 | 55,445,659 | `tsv:300`, `obs-td:16` | 25,975,397 | `tsv:366`, `obs-td:25` | 4,666,467 | `tsv:436`, `obs-td:34` | 2,005,532 | `tsv:502`, `obs-td:43` |
| 2 | 55,175,797 | `tsv:301`, `obs-td:61` | 25,846,923 | `tsv:367`, `obs-td:52` | 4,588,500 | `tsv:437`, `obs-td:79` | 2,002,236 | `tsv:503`, `obs-td:70` |
| 3 | 55,600,683 | `tsv:302`, `obs-td:88` | 25,974,114 | `tsv:368`, `obs-td:97` | 4,594,451 | `tsv:438`, `obs-td:106` | 2,005,542 | `tsv:504`, `obs-td:115` |
| 4 | 56,123,815 | `tsv:303`, `obs-td:133` | 26,757,369 | `tsv:369`, `obs-td:124` | 4,578,941 | `tsv:439`, `obs-td:151` | 1,988,320 | `tsv:505`, `obs-td:142` |
| 5 | 55,131,914 | `tsv:304`, `obs-td:160` | 26,829,145 | `tsv:370`, `obs-td:169` | 4,595,682 | `tsv:440`, `obs-td:178` | 2,000,002 | `tsv:506`, `obs-td:187` |
| 6 | 55,456,910 | `tsv:305`, `obs-td:205` | 25,908,270 | `tsv:371`, `obs-td:196` | 4,535,038 | `tsv:441`, `obs-td:223` | 1,993,048 | `tsv:507`, `obs-td:214` |
| 7 | 55,213,960 | `tsv:306`, `obs-td:232` | 27,238,761 | `tsv:372`, `obs-td:241` | 4,631,150 | `tsv:442`, `obs-td:250` | 1,995,353 | `tsv:508`, `obs-td:259` |
| 8 | 55,411,334 | `tsv:307`, `obs-td:277` | 25,948,926 | `tsv:373`, `obs-td:268` | 4,552,701 | `tsv:443`, `obs-td:295` | 2,002,587 | `tsv:509`, `obs-td:286` |
| 9 | 55,857,760 | `tsv:308`, `obs-td:304` | 26,987,605 | `tsv:374`, `obs-td:313` | 4,563,212 | `tsv:444`, `obs-td:322` | 2,038,335 | `tsv:510`, `obs-td:331` |
| 10 | 55,131,604 | `tsv:309`, `obs-td:349` | 25,955,869 | `tsv:375`, `obs-td:340` | 4,609,118 | `tsv:445`, `obs-td:367` | 2,004,330 | `tsv:511`, `obs-td:358` |
| 11 | 58,571,565 | `tsv:310`, `obs-td:376` | 26,020,071 | `tsv:376`, `obs-td:385` | 4,595,793 | `tsv:446`, `obs-td:394` | 2,000,994 | `tsv:512`, `obs-td:403` |
| 12 | 55,132,695 | `tsv:311`, `obs-td:421` | 27,208,184 | `tsv:377`, `obs-td:412` | 4,520,470 | `tsv:447`, `obs-td:439` | 2,000,312 | `tsv:513`, `obs-td:430` |
| 13 | 55,922,934 | `tsv:312`, `obs-td:448` | 26,002,568 | `tsv:378`, `obs-td:457` | 4,578,420 | `tsv:448`, `obs-td:466` | 2,018,146 | `tsv:514`, `obs-td:475` |
| 14 | 55,229,108 | `tsv:313`, `obs-td:493` | 25,987,189 | `tsv:379`, `obs-td:484` | 4,560,566 | `tsv:449`, `obs-td:511` | 2,007,597 | `tsv:515`, `obs-td:502` |
| 15 | 55,086,809 | `tsv:314`, `obs-td:520` | 25,874,686 | `tsv:380`, `obs-td:529` | 4,593,168 | `tsv:450`, `obs-td:538` | 1,999,110 | `tsv:516`, `obs-td:547` |
| 16 | 56,141,188 | `tsv:315`, `obs-td:565` | 25,971,859 | `tsv:381`, `obs-td:556` | 4,557,100 | `tsv:451`, `obs-td:583` | 1,995,302 | `tsv:517`, `obs-td:574` |
| 17 | 55,744,767 | `tsv:316`, `obs-td:592` | 25,979,854 | `tsv:382`, `obs-td:601` | 4,542,301 | `tsv:452`, `obs-td:610` | 2,001,605 | `tsv:518`, `obs-td:619` |
| 18 | 55,897,826 | `tsv:317`, `obs-td:637` | 26,011,144 | `tsv:383`, `obs-td:628` | 5,529,984 | `tsv:453`, `obs-td:655` | 1,996,725 | `tsv:519`, `obs-td:646` |
| 19 | 55,765,175 | `tsv:318`, `obs-td:664` | 25,993,380 | `tsv:384`, `obs-td:673` | 4,515,250 | `tsv:454`, `obs-td:682` | 2,026,793 | `tsv:520`, `obs-td:691` |
| 20 | 55,972,067 | `tsv:319`, `obs-td:709` | 26,131,052 | `tsv:385`, `obs-td:700` | 4,636,220 | `tsv:455`, `obs-td:727` | 2,012,386 | `tsv:521`, `obs-td:718` |
| 21 | 55,207,628 | `tsv:320`, `obs-td:736` | 25,813,209 | `tsv:386`, `obs-td:745` | 4,517,455 | `tsv:456`, `obs-td:754` | 2,013,517 | `tsv:522`, `obs-td:763` |
| 22 | 55,277,079 | `tsv:321`, `obs-td:781` | 25,899,702 | `tsv:387`, `obs-td:772` | 4,569,463 | `tsv:457`, `obs-td:799` | 2,003,388 | `tsv:523`, `obs-td:790` |
| 23 | 54,955,420 | `tsv:322`, `obs-td:808` | 25,937,545 | `tsv:388`, `obs-td:817` | 4,608,016 | `tsv:458`, `obs-td:826` | 2,014,559 | `tsv:524`, `obs-td:835` |
| 24 | 55,211,164 | `tsv:323`, `obs-td:853` | 25,913,219 | `tsv:389`, `obs-td:844` | 4,605,972 | `tsv:459`, `obs-td:871` | 2,001,876 | `tsv:525`, `obs-td:862` |
| 25 | 55,017,628 | `tsv:324`, `obs-td:880` | 26,621,511 | `tsv:390`, `obs-td:889` | 4,547,522 | `tsv:460`, `obs-td:898` | 2,002,566 | `tsv:526`, `obs-td:907` |
| 26 | 55,658,753 | `tsv:325`, `obs-td:925` | 26,181,226 | `tsv:391`, `obs-td:916` | 4,546,149 | `tsv:461`, `obs-td:943` | 2,007,135 | `tsv:527`, `obs-td:934` |
| 27 | 55,501,876 | `tsv:326`, `obs-td:952` | 26,007,969 | `tsv:392`, `obs-td:961` | 4,548,664 | `tsv:462`, `obs-td:970` | 2,000,233 | `tsv:528`, `obs-td:979` |
| 28 | 55,356,450 | `tsv:327`, `obs-td:997` | 25,952,002 | `tsv:393`, `obs-td:988` | 4,694,791 | `tsv:463`, `obs-td:1015` | 2,003,399 | `tsv:529`, `obs-td:1006` |
| 29 | 55,182,951 | `tsv:328`, `obs-td:1024` | 25,902,369 | `tsv:394`, `obs-td:1033` | 4,508,918 | `tsv:464`, `obs-td:1042` | 2,001,124 | `tsv:530`, `obs-td:1051` |
| 30 | 55,440,409 | `tsv:329`, `obs-td:1069` | 25,941,142 | `tsv:395`, `obs-td:1060` | 4,593,339 | `tsv:465`, `obs-td:1087` | 2,004,941 | `tsv:531`, `obs-td:1078` |
| 31 | 55,322,986 | `tsv:330`, `obs-td:1096` | 25,933,697 | `tsv:396`, `obs-td:1105` | 4,571,637 | `tsv:466`, `obs-td:1114` | 1,999,681 | `tsv:532`, `obs-td:1123` |
| 32 | 55,440,079 | `tsv:331`, `obs-td:1141` | 25,826,384 | `tsv:397`, `obs-td:1132` | 4,568,612 | `tsv:467`, `obs-td:1159` | 2,009,239 | `tsv:533`, `obs-td:1150` |
| 33 | 55,347,322 | `tsv:332`, `obs-td:1168` | 26,100,915 | `tsv:398`, `obs-td:1177` | 4,528,245 | `tsv:468`, `obs-td:1186` | 2,001,355 | `tsv:534`, `obs-td:1195` |
| 34 | 55,627,624 | `tsv:333`, `obs-td:1213` | 26,028,538 | `tsv:399`, `obs-td:1204` | 4,661,407 | `tsv:469`, `obs-td:1231` | 2,007,295 | `tsv:535`, `obs-td:1222` |
| 35 | 55,390,876 | `tsv:334`, `obs-td:1240` | 26,003,159 | `tsv:400`, `obs-td:1249` | 4,610,160 | `tsv:470`, `obs-td:1258` | 2,005,412 | `tsv:536`, `obs-td:1267` |
| 36 | 55,296,496 | `tsv:335`, `obs-td:1285` | 27,244,472 | `tsv:401`, `obs-td:1276` | 4,559,144 | `tsv:471`, `obs-td:1303` | 2,010,311 | `tsv:537`, `obs-td:1294` |
| 37 | 55,932,532 | `tsv:336`, `obs-td:1312` | 26,907,083 | `tsv:402`, `obs-td:1321` | 4,565,976 | `tsv:472`, `obs-td:1330` | 2,003,368 | `tsv:538`, `obs-td:1339` |
| 38 | 55,294,723 | `tsv:337`, `obs-td:1357` | 25,839,289 | `tsv:403`, `obs-td:1348` | 4,542,672 | `tsv:473`, `obs-td:1375` | 2,006,584 | `tsv:539`, `obs-td:1366` |
| 39 | 55,523,757 | `tsv:338`, `obs-td:1384` | 25,909,291 | `tsv:404`, `obs-td:1393` | 4,551,850 | `tsv:474`, `obs-td:1402` | 2,001,414 | `tsv:540`, `obs-td:1411` |
| 40 | 55,393,901 | `tsv:339`, `obs-td:1429` | 25,826,224 | `tsv:405`, `obs-td:1420` | 4,547,201 | `tsv:475`, `obs-td:1447` | 2,028,717 | `tsv:541`, `obs-td:1438` |
| 41 | 55,289,904 | `tsv:340`, `obs-td:1456` | 25,820,513 | `tsv:406`, `obs-td:1465` | 4,534,517 | `tsv:476`, `obs-td:1474` | 2,004,380 | `tsv:542`, `obs-td:1483` |
| 42 | 56,049,845 | `tsv:341`, `obs-td:1501` | 25,870,628 | `tsv:407`, `obs-td:1492` | 4,528,165 | `tsv:477`, `obs-td:1519` | 2,044,035 | `tsv:543`, `obs-td:1510` |
| 43 | 57,113,641 | `tsv:342`, `obs-td:1528` | 25,882,350 | `tsv:408`, `obs-td:1537` | 4,647,131 | `tsv:478`, `obs-td:1546` | 1,997,858 | `tsv:544`, `obs-td:1555` |
| 44 | 55,235,040 | `tsv:343`, `obs-td:1573` | 25,940,029 | `tsv:409`, `obs-td:1564` | 4,558,823 | `tsv:479`, `obs-td:1591` | 2,007,977 | `tsv:545`, `obs-td:1582` |
| 45 | 55,891,354 | `tsv:344`, `obs-td:1600` | 25,928,978 | `tsv:410`, `obs-td:1609` | 4,524,589 | `tsv:480`, `obs-td:1618` | 2,010,742 | `tsv:546`, `obs-td:1627` |
| 46 | 55,147,635 | `tsv:345`, `obs-td:1645` | 25,928,077 | `tsv:411`, `obs-td:1636` | 4,562,601 | `tsv:481`, `obs-td:1663` | 2,015,210 | `tsv:547`, `obs-td:1654` |
| 47 | 55,662,230 | `tsv:346`, `obs-td:1672` | 25,959,747 | `tsv:412`, `obs-td:1681` | 4,579,803 | `tsv:482`, `obs-td:1690` | 2,004,741 | `tsv:548`, `obs-td:1699` |
| 48 | 55,525,440 | `tsv:347`, `obs-td:1717` | 25,941,443 | `tsv:413`, `obs-td:1708` | 4,625,279 | `tsv:483`, `obs-td:1735` | 1,997,948 | `tsv:549`, `obs-td:1726` |
| 49 | 55,548,434 | `tsv:348`, `obs-td:1744` | 25,988,341 | `tsv:414`, `obs-td:1753` | 4,544,025 | `tsv:484`, `obs-td:1762` | 2,002,476 | `tsv:550`, `obs-td:1771` |
| 50 | 55,552,933 | `tsv:349`, `obs-td:1789` | 25,914,291 | `tsv:415`, `obs-td:1780` | 4,655,456 | `tsv:485`, `obs-td:1807` | 1,989,442 | `tsv:551`, `obs-td:1798` |
| 51 | 55,384,233 | `tsv:350`, `obs-td:1816` | 25,874,235 | `tsv:416`, `obs-td:1825` | 4,609,299 | `tsv:486`, `obs-td:1834` | 1,994,701 | `tsv:552`, `obs-td:1843` |
| 52 | 55,407,036 | `tsv:351`, `obs-td:1861` | 25,996,356 | `tsv:417`, `obs-td:1852` | 4,636,471 | `tsv:487`, `obs-td:1879` | 2,004,670 | `tsv:553`, `obs-td:1870` |
| 53 | 55,428,416 | `tsv:352`, `obs-td:1888` | 25,823,699 | `tsv:418`, `obs-td:1897` | 4,609,520 | `tsv:488`, `obs-td:1906` | 2,008,488 | `tsv:554`, `obs-td:1915` |
| 54 | 55,554,746 | `tsv:353`, `obs-td:1933` | 25,932,906 | `tsv:419`, `obs-td:1924` | 4,566,567 | `tsv:489`, `obs-td:1951` | 1,989,692 | `tsv:555`, `obs-td:1942` |
| 55 | 55,063,915 | `tsv:354`, `obs-td:1960` | 25,927,296 | `tsv:420`, `obs-td:1969` | 4,611,152 | `tsv:490`, `obs-td:1978` | 2,031,802 | `tsv:556`, `obs-td:1987` |
| 56 | 55,000,586 | `tsv:355`, `obs-td:2005` | 26,007,968 | `tsv:421`, `obs-td:1996` | 4,566,999 | `tsv:491`, `obs-td:2023` | 2,000,543 | `tsv:557`, `obs-td:2014` |
| 57 | 55,324,980 | `tsv:356`, `obs-td:2032` | 25,815,062 | `tsv:422`, `obs-td:2041` | 4,602,085 | `tsv:492`, `obs-td:2050` | 1,993,329 | `tsv:558`, `obs-td:2059` |
| 58 | 55,413,007 | `tsv:357`, `obs-td:2077` | 26,723,004 | `tsv:423`, `obs-td:2068` | 4,582,929 | `tsv:493`, `obs-td:2095` | 2,000,302 | `tsv:559`, `obs-td:2086` |
| 59 | 55,424,078 | `tsv:358`, `obs-td:2104` | 25,985,426 | `tsv:424`, `obs-td:2113` | 4,559,494 | `tsv:494`, `obs-td:2122` | 2,030,320 | `tsv:560`, `obs-td:2131` |
| 60 | 55,436,121 | `tsv:359`, `obs-td:2149` | 25,812,739 | `tsv:425`, `obs-td:2140` | 4,536,922 | `tsv:495`, `obs-td:2167` | 2,029,237 | `tsv:561`, `obs-td:2158` |
| 61 | 55,044,198 | `tsv:360`, `obs-td:2176` | 27,098,466 | `tsv:426`, `obs-td:2185` | 4,518,607 | `tsv:496`, `obs-td:2194` | 1,995,934 | `tsv:562`, `obs-td:2203` |
| 62 | 55,181,068 | `tsv:361`, `obs-td:2221` | 26,051,751 | `tsv:427`, `obs-td:2212` | 4,572,178 | `tsv:497`, `obs-td:2239` | 1,994,301 | `tsv:563`, `obs-td:2230` |
| 63 | 55,699,751 | `tsv:362`, `obs-td:2248` | 25,997,208 | `tsv:428`, `obs-td:2257` | 4,587,307 | `tsv:498`, `obs-td:2266` | 2,026,241 | `tsv:564`, `obs-td:2275` |
| 64 | 54,992,741 | `tsv:363`, `obs-td:2293` | 26,022,335 | `tsv:429`, `obs-td:2284` | 4,521,964 | `tsv:499`, `obs-td:2311` | 1,993,760 | `tsv:565`, `obs-td:2302` |

## Limitations

- The 2026-11-12 column was measured at commit `1bbd43283`. Between that commit and the measured commit, 28 commits changed files under `rspace++/src` (`tsv:597`; the commits are the rows `rspace_src_commit` of the `tsv` file), the cut among them. A difference between the two allocation columns, and between the two M5 columns, is therefore not attributable to the cut alone. The wall-time comparison is between the measured commit and the parent of the cut, so it is not free of this confound either: between those two commits, 7 commits changed files under `rspace++/src` (`tsv:626`; rows `rspace_src_commit_since_the_parent_of_the_cut` of the `tsv` file), the cut among them, and 3 of them changed `guarded_pathmap.rs` or the measurement test's file `two_phase_teardown_properties.rs` (`tsv:634`; rows `teardown_file_commit_since_the_parent_of_the_cut`). Its result is that of the cut together with the other commits' changes.
- $`A_{\mathrm{drop}}`$ follows the pre-registered definition, which also counts the `AnchorRoot` drops of donors during construction; the cross-check section separates them.
- The M5 run time is a single unprofiled run on a shared host; it is indicative only.
- The Heaptrack recordings used the copied wrapper script described in the method. Its two changed lines move the named pipe and name the installation directory; no run used the stock script, so the copy's equivalence rests on that two-line difference.
- `heaptrack_print` prints the end-of-run counters and its own report counters with different values for temporary allocations; both are reported as printed, and the difference is not explained here.
- The release profile was built with debug information through `CARGO_PROFILE_RELEASE_DEBUG`, as in the 2026-11-12 record.
- The host was shared during the wall-time comparison. The timing processor ran the `performance` governor with frequency boost enabled, and its frequency was not locked: the current frequency read before and after the runs lay between 3,413,440 and 4,310,173 kHz (`tsv:291`, `tsv:292`). Each observation is one teardown in a fresh process, so it includes the cost of running the teardown's code for the first time in that process.

## Recordings (not committed)

| Recording | Bytes | Source | SHA-256 | Source |
|---|---|---|---|---|
| `heaptrack-m1-realistic-256.zst` | 37,086 | `tsv:653` | `09f42a3fb16420ab1a0569709a0158ccc8fed3050d4184301b0eb7220852c9f3` | `tsv:654` |
| `heaptrack-m2-line-16384.zst` | 36,256 | `tsv:655` | `5247993602f7a202a78332cee95a6cb64245152218d453a5c9b41cf2c23b1ae2` | `tsv:656` |
| `heaptrack-m4-realistic-0.zst` | 30,057 | `tsv:657` | `51c5ca453a2ad8a8de6e2ec99c7db28e94f5cf2f1b956a9023c19dd70eb39822` | `tsv:658` |
| `heaptrack-m5-guarded-matching-20000.zst` | 771,296 | `tsv:659` | `1b7f016dada58f57bfe17fc6df2b783ef4c23e0e532eb92f30429f795c300b07` | `tsv:660` |

## References

- Bradley Efron, [*Bootstrap methods: another look at the jackknife*](https://doi.org/10.1214/aos/1176344552), The Annals of Statistics.
- Guy L. Steele Jr., Doug Lea and Christine H. Flood, [*Fast splittable pseudorandom number generators*](https://doi.org/10.1145/2660193.2660195), proceedings of the conference on Object-Oriented Programming, Systems, Languages and Applications.

# Guarded PathMap final-teardown allocations on PathMap 0.3.4 — 2026-11-12

## Scope

This record re-measures, on PathMap 0.3.4, how many heap allocation calls the final teardown
of a `GuardedPathMap` makes. PathMap is the trie library that the `rspace_plus_plus` crate
(directory `rspace++`) stores its tuple-space indices in. `GuardedPathMap` is that crate's
stack-safe owner of a PathMap trie, and its final teardown is the `Drop` implementation of its
root owner `AnchorRoot`, which runs when the last owner of a trie is dropped. The figures of
bug 6441 (`guarded-pathmap-final-root-allocation-teardown`) were taken on PathMap 0.2.2, while
[`rspace++/Cargo.toml`](../../../../rspace++/Cargo.toml) pins `pathmap = "=0.3.4"`.

The user's two sentences that started this measurement, quoted from the leaf task:

> The user asked on 2026-11-11: "hasn't the pathmap dependency been updated to 0.3.4? Doesn't
> that fix the issue automatically?"
>
> The user ruled on 2026-11-12: "For Q3, re-measure on 0.3.4 and decide."

This is the re-measurement of leaf 14343 (parent bug 6441, milestone 7919). It follows the
pre-registered protocol of pgmcp experiment 488
(`c4131-e-td1-guarded-pathmap-final-teardown-allocations-on-pathmap-0-3-4`) as restated in the
leaf. It reports measurements and states which of the two pre-registered outcomes the numbers
select. It contains no decision and no recommendation; the decision rule is written in bug 6441.
No production source, test source or manifest was changed: every binary was built from an
unmodified export of commit `1bbd43283c3c2ed1fb9deb7469c4a05a739535aa`. All runs took place on
2026-11-12 in Coordinated Universal Time (UTC), from 02:49:33 to 02:57:18.

## Terms and symbols

| Term | Meaning |
|---|---|
| PathMap | The byte-keyed trie crate `pathmap`; this record compares its releases 0.2.2 and 0.3.4. |
| `GuardedPathMap` | The owner type in [`guarded_pathmap.rs`](../../../../rspace++/src/rspace/guarded_pathmap.rs) that wraps one PathMap trie and tears it down without recursion. |
| `AnchorRoot` drop | The function `<AnchorRoot<V, D, B> as Drop>::drop` for value type `V`, disposer `D` and barrier type `B`; it runs the two-phase teardown of the last owner. |
| two-phase teardown | Phase A removes and disposes every value in byte preorder; phase B installs a synthetic barrier value at the parent of each edge and prunes the trie one edge at a time ([description in the 0.2.2 record](guarded-pathmap-final-teardown-and-loaded-replacement-2026-09-01.md)). |
| `split_0` | `pathmap::line_list_node::LineListNode::split_0`, which allocates a new trie node when a compressed line-list node is split, for example to hold a barrier value. |
| final owner | The `GuardedPathMap<u64, MeasurementDisposer, MeasurementBarrier>` that the measured test builds and then drops inside its timer. |
| donor | A value-free `GuardedPathMap<V, ValueFreeDisposer, B>` that `materialize_missing_suffix` in [`pathmap_mutation.rs`](../../../../rspace++/src/rspace/pathmap_mutation.rs) builds, grafts into a trie under construction and then drops; dropping it runs its own `AnchorRoot` drop while the measured trie is still being built. |
| Heaptrack | The heap profiler (version 1.5.0) that intercepts every call to an allocation function; `heaptrack --record-only` records a run and `heaptrack_print` renders the recording as text. |
| allocation call | One call to an allocation function (for example `malloc` or `realloc`) as counted by Heaptrack. |
| temporary allocation | An allocation that Heaptrack classifies as freed at once. Heaptrack prints two such counters: the `heaptrack_print` report line `temporary memory allocations` and the end-of-run summary line `temporary allocations` printed when the recorded process exits. They differ and are reported separately. |
| peak heap | The `heaptrack_print` line `peak heap memory consumption`. `heaptrack_print` prints sizes with the suffixes `K` and `M`; values are copied as printed. |
| RSS | Resident set size: the physical memory of a process; peak RSS is its maximum. |
| folded stack | One line of `heaptrack_print -F <file> --flamegraph-cost-type allocations`: the frames of one call stack joined by `;`, root first, each frame written as a symbol and a source file name, followed by its number of allocation calls. |
| TSV | Tab-separated values, the format of the record's evidence table file. |
| SHA-256 | Secure Hash Algorithm 2 with a 256-bit digest (SHA-256), used to fingerprint every file. |
| MAD | Median absolute deviation: the median of the absolute differences between the observations and their median. |
| AMD uProf | The sampling CPU profiler of Advanced Micro Devices (AMD), driven by `AMDuProfCLI`. |
| load average | The three exponentially damped run-queue averages over 1, 5 and 15 minutes from the kernel's `loadavg` file. |

A run $`x`$ is one execution of a test binary under one configuration; runs are named by
measurement, shape and size, for example $`x = (\mathrm{realistic}, 256)`$. For a run $`x`$, let
$`S(x)`$ be the set of distinct folded-stack strings (identical strings merged), $`c_x(s)`$ the
allocation calls of $`s \in S(x)`$, $`D`$ the set of frames whose demangled name is an
`AnchorRoot` drop (any generic arguments), $`f_T`$ the frame of the test function
`final_anchor_teardown_measurement`, and $`f_M`$ the frame of its construction helper
`measurement_map`. The protocol's attribution and its cross-check are

```math
A_{\mathrm{drop}}(x) = \sum_{s \in S(x),\ s \cap D \neq \emptyset} c_x(s),
\qquad
C(x) = \sum_{s \in S(x),\ f_T \in s,\ f_M \notin s} c_x(s).
```

The relative difference that the protocol requires to be explained when it exceeds 1 % is

```math
\delta(x) = \frac{100 \cdot \lvert C(x) - A_{\mathrm{drop}}(x) \rvert}{A_{\mathrm{drop}}(x)}.
```

For the scaling control M3 the protocol forms the ratios

```math
\rho_{\mathrm{realistic}} = \frac{A_{\mathrm{drop}}(\mathrm{realistic}, 256)}{A_{\mathrm{drop}}(\mathrm{realistic}, 128)},
\qquad
\rho_{\mathrm{line}} = \frac{A_{\mathrm{drop}}(\mathrm{line}, 16384)}{A_{\mathrm{drop}}(\mathrm{line}, 8192)},
```

and a cost proportional to topology bytes predicts $`\rho \in [1.8, 2.2]`$. The two
pre-registered outcomes are the conjunction $`P_1 \wedge P_2`$ and its negation, where

```math
P_1 :\ A_{\mathrm{drop}}(\mathrm{realistic}, 256) \le 18170,
\qquad
P_2 :\ A_{\mathrm{drop}}(\mathrm{line}, 16384) \le 1638.
```

For five timed observations $`t_1, \dots, t_5`$ of one shape, the reported centre and spread are

```math
\tilde{t} = \operatorname{median}_{i} t_i,
\qquad
\mathrm{MAD} = \operatorname{median}_{i} \lvert t_i - \tilde{t} \rvert.
```

## Evidence files and citation format

Every measured or derived value in the tables below is followed by a citation `alias:line`,
naming the file by its alias and the 1-based line on which the value appears; numbers in row
labels, such as run sizes, are not values and carry no citation. The raw files sit beside this
record and share its stem. Before their SHA-256 was computed, every absolute path in them was
rewritten: the repository root became `[WORKSPACE_ROOT]`, the home directory became `[HOME]`,
the user name became `[USER]`, and any other absolute path received the prefix `[ROOT]` (so a
system library path reads `[ROOT]/usr/lib/libc.so.6`). The work directory of the measurement is
`[WORKSPACE_ROOT]/target/scratch/session-401d0c58/c44/w14343`, and its `src` directory held the
plain export of the measured commit.

| Alias | File | Content | SHA-256 |
|---|---|---|---|
| `rep-m1` | [M1 report](guarded-pathmap-final-teardown-pathmap-0.3.4-2026-11-12-m1-heaptrack-report.txt) | `heaptrack_print -t 0 -n 30` report of M1 | `6ff114b251f0d51917526bf5f10d5437430b654517907a1f0349cb6eb84820a2` |
| `rep-m2` | [M2 report](guarded-pathmap-final-teardown-pathmap-0.3.4-2026-11-12-m2-heaptrack-report.txt) | `heaptrack_print -t 0 -n 30` report of M2 | `6f85aa44948d66eedbf299679fca4d27879cd439011bab21d88f2e60aa27baaa` |
| `rep-m5` | [M5 report](guarded-pathmap-final-teardown-pathmap-0.3.4-2026-11-12-m5-heaptrack-report.txt) | `heaptrack_print -t 0 -n 30` report of M5 | `14edb694d88085584c1786e2c6dc92f2c3bcee118e224b1fb018b5ef2ed4903e` |
| `tsv` | [allocation table](guarded-pathmap-final-teardown-pathmap-0.3.4-2026-11-12-allocations.tsv) | every run's totals, attribution, cross-check and twenty heaviest attributed stacks, the derived figures, the environment and the recordings | `0874d58f7dbec2481bee9fa5bf731aded1a6a1c7e4512eef2be22a1c15f2484c` |
| `obs-m6` | [M6 observations](guarded-pathmap-final-teardown-pathmap-0.3.4-2026-11-12-m6-teardown-obs.txt) | the two M6 run logs with their `TEARDOWN_OBS` lines and load averages | `cde5981a3b9053a54b21950c20a1564e6fee97a2f9069403aaf489212483dcef` |
| `time-m5` | [M5 wall time](guarded-pathmap-final-teardown-pathmap-0.3.4-2026-11-12-m5-wall-time.txt) | the unprofiled M5 run under GNU `time -v` | `4b575eccc8234f6a4af223af3adf528de8c0d8fca812be7fcc6e63357933f76e` |
| `uprof-m5` | [M5 uProf failure](guarded-pathmap-final-teardown-pathmap-0.3.4-2026-11-12-m5-uprof-error.txt) | the exact uProf command, its error text and the profiler's own log, in place of the function table that could not be produced | `49986d459d55525d4b80a6d23b65375595053ea0ae60b3181e301238b573423f` |
| `ref-0901` | [PathMap 0.2.2 record of 2026-09-01](guarded-pathmap-final-teardown-and-loaded-replacement-2026-09-01.md) | the M1 and M6 reference figures | committed earlier |
| `ref-0831` | [PathMap 0.2.2 record of 2026-08-31](rspace-channel-prefix-publication-2026-08-31.md) | the M5 reference figures | committed earlier |

## Environment

| Item | Value | Source |
|---|---|---|
| measured commit | `1bbd43283c3c2ed1fb9deb7469c4a05a739535aa` | `tsv:414` |
| `pathmap` entry of [`Cargo.lock`](../../../../Cargo.lock), lines 4068 and 4069 | `name = "pathmap"`, `version = "0.3.4"` | `tsv:416`, `tsv:417` |
| `pathmap` line of `rspace++/Cargo.toml`, line 72 | `pathmap = { version = "=0.3.4", features = ["arena_compact"] }` | `tsv:418` |
| `rustc --version` | `rustc 1.95.0-nightly (6efa357bf 2026-02-08)` | `tsv:419` |
| `cargo --version` | `cargo 1.95.0-nightly (fe2f314ae 2026-01-30)` | `tsv:420` |
| Heaptrack | `heaptrack 1.5.0`, `heaptrack_print 1.5.0` | `tsv:421`, `tsv:422` |
| uProf | `AMDuProfCLI Version 5.3.521.0` | `tsv:423` |
| symbol demangler | `rustfilt`, whose `--version` prints `rust_demangle 0.2.1` | `tsv:432` |
| first heading line of the hardware description | `# Hardware Specifications` | `tsv:424` |
| processor | `AMD Ryzen Threadripper PRO 5975WX 32-Cores` | `tsv:425` |
| logical processors, threads per core | 32, 1 | `tsv:426`, `tsv:427` |
| kernel | `Linux 7.2.15-arch1-1` | `tsv:428` |
| `kernel.perf_event_paranoid` | 2 | `tsv:429` |
| frequency governor of CPU 27 | `performance` | `tsv:430` |
| build setting | `CARGO_PROFILE_RELEASE_DEBUG=true`: the release profile with debug information | `tsv:431` |
| library test binary `rspace_plus_plus-284e98559b92ff6f` | SHA-256 `47a0706eddbbdafcf29b5c878dfa541b97cc04f8f041ad6a30d71d5af1365d8a` | `tsv:433` |
| test binary `guarded_matching_tests-c2d38880f7e4ed42` | SHA-256 `1c7edd7e813c1b09cc33429cc8296d79bb622b12ca6df2d76e5cfdfa9d8d1d1c` | `tsv:434` |

The host is shared with other work, so the load average was read before and after every run.
All times are UTC on 2026-11-12; the load averages are the 1, 5 and 15 minute values.

| Run | Measurement | Start | Source | Load average before | Source | Load average after | Source |
|---|---|---|---|---|---|---|---|
| `m4-realistic-0` | M4 | 02:49:33 | `tsv:314` | 15.16 20.34 16.36 | `tsv:312` | 15.16 20.34 16.36 | `tsv:313` |
| `m1-realistic-256` | M1 | 02:49:33 | `tsv:321` | 15.16 20.34 16.36 | `tsv:319` | 15.16 20.34 16.36 | `tsv:320` |
| `m2-line-16384` | M2 | 02:53:34 | `tsv:335` | 25.32 21.16 17.46 | `tsv:333` | 25.32 21.16 17.46 | `tsv:334` |
| `m3-realistic-128` | M3 | 02:53:35 | `tsv:328` | 25.32 21.16 17.46 | `tsv:326` | 25.32 21.16 17.46 | `tsv:327` |
| `m3-line-8192` | M3 | 02:53:36 | `tsv:342` | 25.32 21.16 17.46 | `tsv:340` | 27.94 21.77 17.67 | `tsv:341` |
| `m5-guarded-matching-20000` | M5, Heaptrack | 02:53:43 | `tsv:349` | 28.34 21.95 17.76 | `tsv:347` | 20.87 20.90 17.57 | `tsv:348` |
| `m5-uprof` | M5, uProf | 02:56:26 | `uprof-m5:2` | 8.07 16.02 16.19 | `uprof-m5:5` | 8.07 16.02 16.19 | `uprof-m5:17` |
| `m5-wall-time` | M5, unprofiled | 02:57:04 | `tsv:401` | 7.82 15.00 15.84 | `tsv:399` | 6.91 14.44 15.65 | `tsv:400` |
| `m6-realistic-256` | M6 | 02:57:18 | `tsv:380` | 6.91 14.44 15.65 | `tsv:378` | 6.91 14.44 15.65 | `tsv:379` |
| `m6-line-16384` | M6 | 02:57:18 | `tsv:393` | 6.91 14.44 15.65 | `tsv:391` | 6.91 14.44 15.65 | `tsv:392` |

## Method

### Builds and isolation

Both binaries were built in the export with `--locked --offline -j 8`. The workspace manifest
has no `[profile.release]` section, so the default release profile carries no debug
information, and Heaptrack can name inlined frames only from debug information. The
environment setting `CARGO_PROFILE_RELEASE_DEBUG=true` adds debug information to the release
profile without editing any file. Every build and every run went through the campaign wrapper
`heavy.sh`, which starts the command in a `systemd-run --user --scope` unit with
`MemoryMax=32G` and `MemorySwapMax=0` and pins it with `taskset` to the campaign CPUs, which
on this 32-CPU host are CPUs 16 to 31. Single-run timings (M5 wall time and M6) were further
pinned to CPU 27.

The stock Heaptrack 1.5.0 wrapper script creates its named pipe under `[ROOT]/tmp`, which this
host forbids. The runs used a copy of the stock script with exactly two lines changed: the line
`pipe=[ROOT]/tmp/heaptrack_fifo$$` became `pipe="${TMPDIR:?}/heaptrack_fifo$$"`, and the line
`EXE_PATH=$(readlink -f "$SCRIPT_DIR")` became an assignment of the stock installation directory
`[ROOT]/usr/bin`, because the copy no longer sits beside the stock script. The preload library,
the interpreter and every flag are the stock ones. Recording ran with `DEBUGINFOD_URLS` unset,
so symbol resolution used only local debug information.

### Runs

M1 to M4 run the ignored library test `final_anchor_teardown_measurement` of
[`two_phase_teardown_properties.rs`](../../../../rspace++/tests/guarded_pathmap/two_phase_teardown_properties.rs),
exact, with one test thread and one trial, under `heaptrack --record-only`. The test builds the
trie with `measurement_map` and then drops it inside its timer, printing one `TEARDOWN_OBS`
line per trial. The `realistic` shape stores one value under each of `RSPACE_TEARDOWN_SIZE`
keys of 939 bytes that share a 203-byte prefix. The `line` shape creates one value-free path
of `RSPACE_TEARDOWN_SIZE` bytes with `create_path`. M5 runs
`selector_and_enumerator_are_stack_safe_at_twenty_thousand_binds` of
[`guarded_matching_tests.rs`](../../../../rspace++/tests/guarded_matching_tests.rs) under
Heaptrack, under uProf and under GNU `time -v`. M6 runs the library test with five trials per
shape and no profiler.

Every recording was rendered once by one `heaptrack_print` invocation that wrote both the
report and the folded stacks. The option `-t 0` turns off template shortening, which would
otherwise replace the generic arguments of `<AnchorRoot<V, D, B> as Drop>::drop` (the whole
`Self` type of the frame) by an ellipsis.

### Attribution

The symbols of the pinned nightly toolchain are mangled in the Rust v0 scheme, which
`heaptrack_print` 1.5.0 leaves mangled, so every frame was demangled with `rustfilt` before
matching. No mangled symbol remained after demangling.

**Algorithm 1 (Folded-stack attribution).**

```pseudocode
input:  the folded-stack lines F of one run x, and the demangler
output: A_drop(x), C(x), A_drop(x) per AnchorRoot instantiation, the twenty heaviest A_drop stacks

merged := empty map from stack string to allocation calls
for each line in F:
    split the line at its last space into (stack, cost)
    merged[stack] := merged[stack] + cost              -- identical stack strings are merged
A := 0;  C := 0;  per_instantiation := empty map;  heavy := empty list
for each (stack, cost) in merged:
    names := [ demangle(symbol of frame) for each frame of stack split at ";" ]
    drops := { generic arguments g : some name equals
               "<rspace_plus_plus::rspace::guarded_pathmap::AnchorRoot<g> as core::ops::drop::Drop>::drop" }
    if drops is not empty:
        A := A + cost
        for each g in drops: per_instantiation[g] := per_instantiation[g] + cost
        append (cost, stack) to heavy
    if f_T is in names and f_M is not in names:
        C := C + cost
sort heavy by decreasing cost and keep its first twenty entries
return A, C, per_instantiation, heavy
```

A stack counts once toward $`A_{\mathrm{drop}}(x)`$ however many `AnchorRoot` drop frames it
contains, so the sum is exactly the number of allocation calls made while some `AnchorRoot`
drop was on the stack. The same loop also records, for each attributed stack, whether
`measurement_map` is on it, whether `split_0` is on it, and the call sites that explain an
attributed stack inside `measurement_map`; all of these appear as rows of the `tsv` file. The
cost sums over all folded stacks equal `heaptrack_print`'s total allocation calls in every run
(rows `stack_cost_sum` of the `tsv` file).

The following diagram shows how the allocation calls of one library-test run fall into the
two attributions.

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

Legend: each box is a set of folded stacks, nested boxes are subsets, and the label `[A]`
marks the stacks counted by $`A_{\mathrm{drop}}`$ while `[C]` marks those counted by the
cross-check $`C`$. Hence $`A_{\mathrm{drop}}`$ is the final-owner part plus the donor part,
and $`C`$ is the final-owner part plus the environment reads of the test, which the tables
below confirm in every library-test run.

## Exact commands

The commands below use the same placeholders as the raw files. They are the commands of the
scripts that produced the logs, in the order in which the runs took place.

```bash
# Placeholders as in the raw files: [WORKSPACE_ROOT] is the repository root, [ROOT] the file-system root.
W='[WORKSPACE_ROOT]/target/scratch/session-401d0c58/c44/w14343'
HEAVY='[WORKSPACE_ROOT]/target/scratch/session-401d0c58/scripts/heavy.sh'
export TMPDIR="$W/tmp" TEMP="$W/tmp" TMP="$W/tmp" AMDUPROF_LOGDIR="$W/tmp"
export CARGO_TARGET_DIR="$W/target" CARGO_PROFILE_RELEASE_DEBUG=true

# Builds, in the export of the measured commit.
cd "$W/src"
"$HEAVY" cargo test --release --locked --offline -p rspace_plus_plus --lib --no-run -j 8
"$HEAVY" cargo test --release --locked --offline -p rspace_plus_plus --test guarded_matching_tests --no-run -j 8

LIB="$W/target/release/deps/rspace_plus_plus-284e98559b92ff6f"
GM="$W/target/release/deps/guarded_matching_tests-c2d38880f7e4ed42"
TEST='rspace::guarded_pathmap::two_phase_teardown_properties::final_anchor_teardown_measurement'
GM_TEST='selector_and_enumerator_are_stack_safe_at_twenty_thousand_binds'
HT="$W/out/scripts/heaptrack-1.5.0-fifo-in-tmpdir.sh"
REC="$W/out/recordings"
LOGS="$W/out/logs"

# Exact test names, taken from the binaries' own lists.
cd "$W/src/rspace++"
"$HEAVY" "$LIB" --list --ignored
"$HEAVY" "$GM" --list

# Heaptrack recordings: M4 and M1, then M2, M3 and M5.
"$HEAVY" env -u DEBUGINFOD_URLS RSPACE_TEARDOWN_SHAPE=realistic RSPACE_TEARDOWN_SIZE=0 RSPACE_TEARDOWN_TRIALS=1 \
  "$HT" --record-only -o "$REC/heaptrack-m4-realistic-0" "$LIB" --exact "$TEST" --ignored --test-threads=1 --nocapture
"$HEAVY" env -u DEBUGINFOD_URLS RSPACE_TEARDOWN_SHAPE=realistic RSPACE_TEARDOWN_SIZE=256 RSPACE_TEARDOWN_TRIALS=1 \
  "$HT" --record-only -o "$REC/heaptrack-m1-realistic-256" "$LIB" --exact "$TEST" --ignored --test-threads=1 --nocapture
"$HEAVY" env -u DEBUGINFOD_URLS RSPACE_TEARDOWN_SHAPE=line RSPACE_TEARDOWN_SIZE=16384 RSPACE_TEARDOWN_TRIALS=1 \
  "$HT" --record-only -o "$REC/heaptrack-m2-line-16384" "$LIB" --exact "$TEST" --ignored --test-threads=1 --nocapture
"$HEAVY" env -u DEBUGINFOD_URLS RSPACE_TEARDOWN_SHAPE=realistic RSPACE_TEARDOWN_SIZE=128 RSPACE_TEARDOWN_TRIALS=1 \
  "$HT" --record-only -o "$REC/heaptrack-m3-realistic-128" "$LIB" --exact "$TEST" --ignored --test-threads=1 --nocapture
"$HEAVY" env -u DEBUGINFOD_URLS RSPACE_TEARDOWN_SHAPE=line RSPACE_TEARDOWN_SIZE=8192 RSPACE_TEARDOWN_TRIALS=1 \
  "$HT" --record-only -o "$REC/heaptrack-m3-line-8192" "$LIB" --exact "$TEST" --ignored --test-threads=1 --nocapture
"$HEAVY" env -u DEBUGINFOD_URLS \
  "$HT" --record-only -o "$REC/heaptrack-m5-guarded-matching-20000" "$GM" --exact "$GM_TEST" --test-threads=1 --nocapture

# Each recording was rendered right after it was made; shown for M1, the other five differ only in the run name.
"$HEAVY" '[ROOT]/usr/bin/heaptrack_print' -f "$REC/heaptrack-m1-realistic-256.zst" -t 0 -n 30 \
  -F "$LOGS/heaptrack-m1-realistic-256.stacks.txt" --flamegraph-cost-type allocations \
  > "$LOGS/heaptrack-m1-realistic-256.report.txt"

# M5 CPU attribution: 1 ms time-based hotspots with call-stack sampling; this collection failed.
"$HEAVY" '[ROOT]/usr/bin/AMDuProfCLI' collect --config hotspots --timer-interval 1 \
  --call-graph-mode fpo --call-graph-size 8192 --call-graph-depth 128 --affinity 27 \
  --log-path "$TMPDIR" -o "$REC/uprof-m5" "$GM" --exact "$GM_TEST" --test-threads=1 --nocapture

# M5 wall time, then M6, each pinned to CPU 27, with the load average read before and after.
cat '[ROOT]/proc/loadavg'
"$HEAVY" taskset -c 27 '[ROOT]/usr/bin/time' -v "$GM" --exact "$GM_TEST" --test-threads=1 --nocapture
cat '[ROOT]/proc/loadavg'
"$HEAVY" taskset -c 27 env RSPACE_TEARDOWN_SHAPE=realistic RSPACE_TEARDOWN_SIZE=256 RSPACE_TEARDOWN_TRIALS=5 \
  "$LIB" --exact "$TEST" --ignored --test-threads=1 --nocapture
cat '[ROOT]/proc/loadavg'
"$HEAVY" taskset -c 27 env RSPACE_TEARDOWN_SHAPE=line RSPACE_TEARDOWN_SIZE=16384 RSPACE_TEARDOWN_TRIALS=5 \
  "$LIB" --exact "$TEST" --ignored --test-threads=1 --nocapture
cat '[ROOT]/proc/loadavg'
```

The Heaptrack recordings are not committed: they are reproducible with these commands, and
their sizes and SHA-256 are listed at the end of this record.

## Results

### Comparison of PathMap 0.2.2 and PathMap 0.3.4

The PathMap 0.2.2 values are copied as written in the two earlier records. Those records were
measured at other commits: the 2026-09-01 record at implementation commit `68534b6fc` with
benchmark commits up to `9ab98a331`, and the 2026-08-31 M5 figures before the two-phase
teardown of commit `68534b6fc` (see Limitations). "Not measured" means the earlier records
contain no such run; "not computed" means the run exists but the quantity was not derived.

| Measurement | Metric | PathMap 0.2.2 | Source | PathMap 0.3.4 | Source |
|---|---|---|---|---|---|
| M1 realistic 256 | allocation calls | 187,109 | `ref-0901:135` | 190,832 | `rep-m1:5028` |
| M1 realistic 256 | temporary allocations, `heaptrack_print` counter | 177,403 | `ref-0901:136` | 178,833 | `rep-m1:5029` |
| M1 realistic 256 | temporary allocations, end-of-run counter | not reported | none | 1,102 | `tsv:318` |
| M1 realistic 256 | peak heap | 386.49 KiB | `ref-0901:137` | 403.75K | `rep-m1:5030` |
| M1 realistic 256 | peak RSS including Heaptrack overhead | 13.05 MiB | `ref-0901:138` | 15.28M | `rep-m1:5031` |
| M1 realistic 256 | $`A_{\mathrm{drop}}`$ | not computed | none | 184,489 | `tsv:22` |
| M1 realistic 256 | $`A_{\mathrm{drop}}`$, final-owner instantiation | not computed | none | 183,522 | `tsv:23` |
| M1 realistic 256 | $`A_{\mathrm{drop}}`$, donor instantiation | not computed | none | 967 | `tsv:24` |
| M1 realistic 256 | cross-check $`C`$ | not computed | none | 183,525 | `tsv:32` |
| M1 realistic 256 | calls in $`A_{\mathrm{drop}}`$ whose stack also contains `split_0` | not computed | none | 182,623 | `tsv:28` |
| M1 realistic 256 | heaviest single stack under the `AnchorRoot` drop, ending in `split_0` | 181,708 | `ref-0901:142` | 181,708 | `tsv:29` |
| M2 line 16384 | allocation calls | not measured | none | 33,602 | `rep-m2:4375` |
| M2 line 16384 | temporary allocations, `heaptrack_print` counter | not measured | none | 31,283 | `rep-m2:4376` |
| M2 line 16384 | temporary allocations, end-of-run counter | not measured | none | 81 | `tsv:332` |
| M2 line 16384 | peak heap | not measured | none | 252.20K | `rep-m2:4377` |
| M2 line 16384 | peak RSS including Heaptrack overhead | not measured | none | 13.88M | `rep-m2:4378` |
| M2 line 16384 | $`A_{\mathrm{drop}}`$ | not measured | none | 32,489 | `tsv:140` |
| M2 line 16384 | $`A_{\mathrm{drop}}`$, final-owner instantiation | not measured | none | 16,049 | `tsv:142` |
| M2 line 16384 | $`A_{\mathrm{drop}}`$, donor instantiation | not measured | none | 16,440 | `tsv:141` |
| M2 line 16384 | cross-check $`C`$ | not measured | none | 16,052 | `tsv:150` |
| M2 line 16384 | calls in $`A_{\mathrm{drop}}`$ whose stack also contains `split_0` | not measured | none | 31,984 | `tsv:146` |
| M2 line 16384 | heaviest single stack under the `AnchorRoot` drop, ending in `split_0` | not measured | none | 15,992 | `tsv:147` |
| M5 20,000 binds | allocation calls | 33,318,622 | `ref-0831:55` | 56,666,258 | `rep-m5:10582` |
| M5 20,000 binds | $`A_{\mathrm{drop}}`$ | not computed | none | 48,461,906 | `tsv:258` |
| M5 20,000 binds | temporary allocations, end-of-run counter (0.2.2 counter inferred) | 376,232 | `ref-0831:55` | 746,015 | `tsv:346` |
| M5 20,000 binds | temporary allocations, `heaptrack_print` counter | not reported | none | 40,303,806 | `rep-m5:10583` |
| M5 20,000 binds | leaked allocations, end-of-run counter | 69 | `ref-0831:55` | 69 | `tsv:345` |
| M5 20,000 binds | peak heap | not reported | none | 128.57M | `rep-m5:10584` |
| M5 20,000 binds | peak RSS including Heaptrack overhead | not reported | none | 180.75M | `rep-m5:10585` |
| M5 20,000 binds | calls on PathMap line-list split or drop paths, as selected in 2026-08-31 | 23,208,116 | `ref-0831:58` | not computed | none |
| M5 20,000 binds | calls in $`A_{\mathrm{drop}}`$ whose stack also contains `split_0` | not computed | none | 40,745,844 | `tsv:267` |
| M5 20,000 binds | self CPU share of the `AnchorRoot` drop | 0.69 % | `ref-0831:59` | not measured | `uprof-m5:10` |
| M5 20,000 binds | inclusive CPU share of the `AnchorRoot` drop | 63.90 % | `ref-0831:59` | not measured | `uprof-m5:10` |
| M5 20,000 binds | wall time, unprofiled | 7.52 s | `ref-0831:50` | 13.18 s | `time-m5:16` |
| M5 20,000 binds | peak RSS, unprofiled | 151,408 KiB | `ref-0831:50` | 172,656 KiB | `time-m5:21` |
| M5 20,000 binds | runtime under Heaptrack | not reported | none | 37.73 s | `rep-m5:10581` |
| M6 realistic 256 | median teardown time | 57.860517 ms | `ref-0901:95` | 52.839339 ms | `tsv:376` |
| M6 realistic 256 | MAD of the teardown time | 0.389448 ms | `ref-0901:95` | 0.040618 ms | `tsv:377` |
| M6 line 16384 | median teardown time | 4.496256 ms | `ref-0901:92` | 4.318837 ms | `tsv:389` |
| M6 line 16384 | MAD of the teardown time | 0.007774 ms | `ref-0901:92` | 0.006071 ms | `tsv:390` |
| M6 both shapes | host load during the timings | aggregate idleness about 55 to 66 % | `ref-0901:56` | load average 6.91 14.44 15.65 before and after | `obs-m6:4`, `obs-m6:18`, `obs-m6:23`, `obs-m6:37` |

The five M6 observations behind the medians, in nanoseconds:

| Trial | Realistic 256 | Source | Line 16384 | Source |
|---|---|---|---|---|
| 0 | 52,839,339 | `obs-m6:8` | 4,287,988 | `obs-m6:27` |
| 1 | 52,850,961 | `obs-m6:9` | 4,313,016 | `obs-m6:28` |
| 2 | 52,714,562 | `obs-m6:10` | 4,318,837 | `obs-m6:29` |
| 3 | 52,784,606 | `obs-m6:11` | 4,332,612 | `obs-m6:30` |
| 4 | 52,879,957 | `obs-m6:12` | 4,324,908 | `obs-m6:31` |
| median | 52,839,339 | `tsv:374` | 4,318,837 | `tsv:387` |
| MAD | 40,618 | `tsv:375` | 6,071 | `tsv:388` |

The M5 attribution $`A_{\mathrm{drop}}`$ splits over six `AnchorRoot` instantiations, named
here by value type and disposer; no stack contains two of them, so the six rows sum to
$`A_{\mathrm{drop}}`$.

| Value type | Disposer | Allocation calls | Source |
|---|---|---|---|
| `Multiplicity` | `MultiplicityDisposer` | 19,752,282 | `tsv:259` |
| `Multiplicity` | `ValueFreeDisposer` | 14,746,976 | `tsv:260` |
| `CandidatePosition` | `ValueFreeDisposer` | 4,915,560 | `tsv:261` |
| `CandidatePosition` | `CandidatePositionDisposer` | 4,795,560 | `tsv:262` |
| `ChannelSlot` | `ChannelSlotDisposer` | 4,251,302 | `tsv:263` |
| `ChannelSlot` | `ValueFreeDisposer` | 226 | `tsv:264` |

### M3: scaling control

| Shape | $`A_{\mathrm{drop}}`$ at the full size | Source | $`A_{\mathrm{drop}}`$ at half size | Source | Ratio | Source | Inside [1.8, 2.2] |
|---|---|---|---|---|---|---|---|
| realistic, 256 over 128 | 184,489 | `tsv:22` | 92,841 | `tsv:81` | 1.98715 | `tsv:351` | yes |
| line, 16384 over 8192 | 32,489 | `tsv:140` | 16,286 | `tsv:199` | 1.99490 | `tsv:354` | yes |

Both pre-registered ratios lie inside [1.8, 2.2]. The same ratios formed from the
final-owner instantiation alone are 1.99754 (`tsv:352`) and 1.99490 (`tsv:355`), and from the
cross-check 1.99751 (`tsv:353`) and 1.99453 (`tsv:356`).

### M4: negative control of the attribution

| Quantity for `realistic` size 0 | Value | Source |
|---|---|---|
| allocation calls | 672 | `tsv:2` |
| $`A_{\mathrm{drop}}`$ | 0 | `tsv:7` |
| cross-check $`C`$ | 3 | `tsv:14` |

$`A_{\mathrm{drop}}(\mathrm{realistic}, 0) = 0 \le 16`$, so the attribution passes its
negative control (`tsv:357`). The three calls of $`C`$ are the test's reads of its environment
variables (`tsv:16`).

### Cross-check

| Run | $`A_{\mathrm{drop}}`$ | Source | $`C`$ | Source | $`C - A_{\mathrm{drop}}`$ | Source | $`\delta`$ in % | Source |
|---|---|---|---|---|---|---|---|---|
| M1 realistic 256 | 184,489 | `tsv:22` | 183,525 | `tsv:32` | -964 | `tsv:33` | 0.523 | `tsv:34` |
| M2 line 16384 | 32,489 | `tsv:140` | 16,052 | `tsv:150` | -16,437 | `tsv:151` | 50.593 | `tsv:152` |
| M3 realistic 128 | 92,841 | `tsv:81` | 91,877 | `tsv:91` | -964 | `tsv:92` | 1.038 | `tsv:93` |
| M3 line 8192 | 16,286 | `tsv:199` | 8,048 | `tsv:209` | -8,238 | `tsv:210` | 50.583 | `tsv:211` |

The differences are explained from the stacks, and exactly, by the decomposition below.
$`A_{\mathrm{drop}}`$ counts two instantiations: the final owner's
`AnchorRoot<u64, MeasurementDisposer, MeasurementBarrier>` and the donors'
`AnchorRoot<u64, ValueFreeDisposer, MeasurementBarrier>`. Every donor stack also contains
`measurement_map` and the call site `materialize_missing_suffix` (`tsv:26`, `tsv:144`), so the
cross-check excludes it. The cross-check in turn counts three allocation calls that are not
under any `AnchorRoot` drop: `getenv` copies the test's environment variables (`tsv:35`,
`tsv:153`). In the `line` shape the test calls `create_path` on an empty trie, so the missing
suffix that `NativePathCursor::create_path` hands to `materialize_missing_suffix` is the whole
16,384-byte path; the donor therefore holds the whole path, and dropping it after the graft
accounts for 16,440 calls beside the final owner's 16,049. In the `realistic` shape the donor
part is 967 calls at both sizes 128 and 256.

| Run | Final owner | Source | Donor | Source | `getenv` calls | Source |
|---|---|---|---|---|---|---|
| M1 realistic 256 | 183,522 | `tsv:23` | 967 | `tsv:24` | 3 | `tsv:35` |
| M2 line 16384 | 16,049 | `tsv:142` | 16,440 | `tsv:141` | 3 | `tsv:153` |
| M3 realistic 128 | 91,874 | `tsv:82` | 967 | `tsv:83` | 3 | `tsv:94` |
| M3 line 8192 | 8,045 | `tsv:201` | 8,241 | `tsv:200` | 3 | `tsv:212` |

In every run, $`A_{\mathrm{drop}}`$ equals final owner plus donor, and $`C`$ equals final owner
plus the `getenv` calls.

### Positive control

| Quantity | Value | Source |
|---|---|---|
| $`A_{\mathrm{drop}}(\mathrm{realistic}, 256)`$ | 184,489 | `tsv:22` |
| PathMap 0.2.2 reference | 181,708 | `ref-0901:142` |
| difference | 2,781 | `tsv:358` |
| absolute difference in % of the reference | 1.530 | `tsv:359` |

The positive control runs only if the absolute difference exceeds 10 % of 181,708. It is
1.530 %, so the condition does not hold (`tsv:359`), and the positive control on commit
`9ab98a331` was not run.

### Outcome selected by the numbers

| Predicate | Value | Source | Threshold | Holds |
|---|---|---|---|---|
| $`P_1`$: $`A_{\mathrm{drop}}(\mathrm{realistic}, 256) \le 18170`$ | 184,489 | `tsv:360` | 18,170 | no |
| $`P_2`$: $`A_{\mathrm{drop}}(\mathrm{line}, 16384) \le 1638`$ | 32,489 | `tsv:361` | 1,638 | no |

$`P_1`$ and $`P_2`$ are both false (`tsv:362`). The numbers therefore select the second
pre-registered outcome: it is not the case that
$`A_{\mathrm{drop}}(\mathrm{realistic}, 256) \le 18170`$ and
$`A_{\mathrm{drop}}(\mathrm{line}, 16384) \le 1638`$. The selection is the same
when the final-owner instantiation alone (183,522 and 16,049, `tsv:363` to `tsv:365`) or the
cross-check (183,525 and 16,052, `tsv:366` to `tsv:368`) replaces $`A_{\mathrm{drop}}`$.

## Limitations

- The pgmcp experiment 488 itself could not be read: the read tool was not available to the
  measuring agent and the service's web route refused the request without a token. The protocol
  was taken from its restatement in the leaf.
- The PathMap 0.2.2 and PathMap 0.3.4 values differ in more than the PathMap release: they were
  measured at different commits of this repository. The M5 reference figures of 2026-08-31 also
  predate the two-phase teardown of commit `68534b6fc`. A difference between the two columns is
  therefore not attributable to PathMap alone. No PathMap 0.2.2 Heaptrack figure exists for the
  `line` shape.
- The 2026-08-31 record does not name the Heaptrack counter of its 376,232 temporary
  allocations. Its 69 leaks match the end-of-run summary format, so the table pairs it with the
  end-of-run counter; this pairing is an inference.
- The 2026-08-31 selection of 23,208,116 calls on line-list split or drop paths is not defined
  precisely enough to be repeated, so it has no PathMap 0.3.4 counterpart; $`A_{\mathrm{drop}}`$
  and its `split_0` subset are different selections.
- The 2026-09-01 record does not state how its dominant stack of 181,708 calls was aggregated.
  Here the heaviest stack is the sum of the folded lines with an identical stack string.
- $`A_{\mathrm{drop}}`$ follows the pre-registered definition, which also counts the
  `AnchorRoot` drops of donors during construction; the cross-check table separates them.
- M5's CPU shares were not measured. The one uProf collection failed with
  `ERROR: Failed to start profiling. Access denied. (error code 0x80070005)` (`uprof-m5:10`);
  the profiler's log shows `sys_perf_event_open failed - Permission denied.` (`uprof-m5:66`)
  under `kernel.perf_event_paranoid` 2 (`uprof-m5:4`). It also reported that Hotspots ignores
  the requested frame-pointer call-graph mode (`uprof-m5:8`). No other profiler was used.
- The M6 timings and the M5 wall time are indicative only: the host was shared, and the load
  averages are listed above. The M5 wall time is a single run.
- The release profile was built with debug information through `CARGO_PROFILE_RELEASE_DEBUG`;
  the earlier records do not state their debug setting.
- The Heaptrack recordings used the copied wrapper script described in the method. Its two
  changed lines move the named pipe and name the installation directory; no run used the stock
  script, so the copy's equivalence rests on that two-line difference.
- `heaptrack_print` prints the end-of-run counters and its own report counters with different
  values for temporary allocations; both are reported as printed, and the difference is not
  explained here.

## Recordings (not committed)

| Recording | Bytes | Source | SHA-256 | Source |
|---|---|---|---|---|
| `heaptrack-m1-realistic-256.zst` | 38,927 | `tsv:435` | `c5cc0ccd7048a54efbadbc71fe8c80a251e96447c50f1da81420423e8086f09d` | `tsv:436` |
| `heaptrack-m2-line-16384.zst` | 37,217 | `tsv:437` | `050bebe4c6463783d82bf3cfe3a2222f0f6246d5e3eb0086b40aa8e9d8a07b5e` | `tsv:438` |
| `heaptrack-m3-line-8192.zst` | 36,909 | `tsv:439` | `e2c9db5826dba9e4384d8358a733dae02c65ceb80f881197e4b6fecbef45e634` | `tsv:440` |
| `heaptrack-m3-realistic-128.zst` | 38,352 | `tsv:441` | `c76a5f90e3089c2cacfc07cbeb56b2ac6d7f32a8a5b677eff753906296f51d35` | `tsv:442` |
| `heaptrack-m4-realistic-0.zst` | 30,237 | `tsv:443` | `5edb1be05cf2674eeafcabd39e60f50bcfe6c6893b8c7a30fea792be6e088556` | `tsv:444` |
| `heaptrack-m5-guarded-matching-20000.zst` | 1,242,518 | `tsv:445` | `7ad03a0dee005642719ab72241a9746c61bfcfacaf044226129026ac1aa5d5b2` | `tsv:446` |
| `uprof-m5/session.uprof` | 16,591 | `tsv:447` | `fee3873d1f61c43c1be8f94c9b240167c884b288c6c0d714368f09287dc9992d` | `tsv:448` |
| `uprof-m5/cpu/CpuProfile_Nov-11-2026_21-56-26.caperf` | 160 | `tsv:449` | `26dd18e26ae609e5737f99476072add44786384a68a6ad99ed6767cda676e744` | `tsv:450` |

The two uProf files are what the failed collection left behind; no profile data was recorded.
The uProf file name carries the host's local time, which is UTC minus five hours.

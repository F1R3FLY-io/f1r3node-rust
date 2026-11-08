# Fixed-delta trusted commit scaling: decision measurements of 2026-11-03

These files are the committed summary copies of the decision population of pgmcp experiment 446 (task 7929). That
experiment is the preregistered measurement of whether one trusted RSpace commit costs only what it changes. The
population ran from 2026-11-01 10:17:51 UTC to 2026-11-03 04:58:48 UTC, against the release lib test binary built
from commit `941ff07d821f9b7b5e8dc5d0a8e62310c328b32c`. The file names carry 2026-11-03, the date on which the decision
data were complete.

The protocol, the cells, the record contract of the measuring child and the gates are defined in the invariant
catalog, section
[Fixed-delta trusted commit scaling (2026-10-29)](../../pathmap/generation-checkpoint-invariant-catalog-2026-08-28.md#fixed-delta-trusted-commit-scaling-2026-10-29).
This document covers the five files of this measurement:

- it defines every column and field of each file;
- it records the program that derives them from the archived raw data;
- it lists the pgmcp artifacts that hold that archive, with their digests.

The four data files are `rspace-fixed-delta-scaling-2026-11-03-process-summary.tsv`,
`rspace-fixed-delta-scaling-2026-11-03-verify.tsv`, `rspace-fixed-delta-scaling-2026-11-03-analysis.json` and
`rspace-fixed-delta-scaling-2026-11-03-manifest.json`. Below they are shortened to `…-process-summary.tsv`,
`…-verify.tsv`, `…-analysis.json` and `…-manifest.json`.

## Contents

1. [Terms and symbols](#1-terms-and-symbols)
2. [The files and the cells](#2-the-files-and-the-cells)
3. [Provenance](#3-provenance)
4. [`…-process-summary.tsv`](#4--process-summarytsv)
5. [`…-verify.tsv`](#5--verifytsv)
6. [`…-analysis.json`](#6--analysisjson)
7. [`…-manifest.json`](#7--manifestjson)
8. [The derivation program](#8-the-derivation-program)
9. [Reproducing and verifying](#9-reproducing-and-verifying)
10. [References](#10-references)

**The outcome at a glance.** The JSON paths below are fields of `…-analysis.json` (section 6).

| Quantity | Path | Value | Bound | Result |
|---|---|---|---|---|
| H1, U: upper 95% bound of the thread CPU exponent | `gates.H1.arms.U.upper95` | 0.2086 | at most 0.25 | passes |
| H1, R: the same | `gates.H1.arms.R.upper95` | 0.1395 | at most 0.25 | passes |
| G4, U: upper bound of the allocation-byte exponent, both runs | `gates.G4.metrics.alloc_bytes.runs.1.arms.U.upper95` | 0.4174 | at most 0.25 | fails |
| G4, R: the same | `gates.G4.metrics.alloc_bytes.runs.1.arms.R.upper95` | 0.3234 | at most 0.25 | fails |
| G4, U and R: the allocation-count exponent's upper bounds | `gates.G4.metrics.alloc_count.runs.1.arms.<axis>.upper95` | 0.1130 and 0.0576 | at most 0.25 | pass |

The population is qualified, so `qualified` is true. H1, G1 to G3 and G5 to G9 pass. G4 fails on allocation bytes, so
`accepted` is false. Bug item `rspace-ledger-trial-cow-allocation-growth` records the cause and its consensus-change
assessment.

## 1. Terms and symbols

**Archive terms.**

| Term | Meaning |
|---|---|
| pgmcp artifact | A text record attached to a pgmcp experiment by the tool `experiment_log_artifact`, identified by its artifact id |
| content | The text of an artifact |
| content digest | The lowercase hexadecimal SHA-256 [1] of the UTF-8 bytes of an artifact's content. pgmcp computes this value when the artifact is logged and stores it as `content_sha256` (function `tool_experiment_log_artifact` in pgmcp's `src/mcp/tools/tool_experiments.rs`) |
| archived file | A file that was uploaded as one or more artifacts |
| part | An artifact that holds one slice of an archived file's gzip [2] and base64 [4] encoding, in the format of section 7.2 |
| bundle | A deterministic GNU tar of a population directory or of a list of files, made by task 13979 and archived as parts |
| record set | One local archive record file, `archive-records/<name>.jsonl`: the output of one archiving run. Its name groups the artifacts in `…-manifest.json` |
| collection manifest | The `manifest.json` that the driver writes into a population directory: the size and SHA-256 of every file there. It is not `…-manifest.json` |

**Measurement terms.**

| Term | Meaning |
|---|---|
| population | One collection run of the driver `scripts/bench/rspace-fixed-delta-scaling.py`: the decision population, a pilot or a negative control. These files hold the decision population |
| cell | One combination of the twelve cell parameters, written as `key=value` pairs (section 2) |
| cell id | A cell's short name, `<axis>-k<N>-h<H>-d<d>-m<m>-<writer>-<case>`, for example `U-k1024-h1024-d1-m0-v2-ordinary` |
| axis | U, R, D, L or E. U varies the keys, R the history, D the replaced keys per commit, L the deleted keys per commit; E is the eviction case |
| stratum | The value of an axis's varied parameter: $`N`$ on U and E, $`H`$ on R, $`d`$ on D and $`m`$ on L |
| child | One process of the measured test binary, running the ignored test `scale_child` in build, verify or time mode |
| verify run | One verify child per cell. The decision population has two, numbered 1 and 2 |
| time process | One time child |
| round | One visit of every cell by a time process, in an order shuffled with seed 7929. Population `warmup` holds rounds 0 to 2, population `decision` rounds 0 to 30 |
| commit | One trusted commit of a child, labeled `warmup`, `deferred`, `reclaim` or `timed` |
| phase | One timed or probed step of a commit: seal, bind, prepare, publish and, in time mode only, release |
| clock | Thread CPU time, process CPU time or monotonic wall time, in nanoseconds |
| probe | A per-phase counter of a verify child: the allocation count, allocated bytes, peak live bytes and largest allocation of the counting allocator, and the `syscr`, `syscw`, `rchar` and `wchar` deltas of `/proc/self/io` |
| calibration | The cost of the measurement itself, which the driver subtracts (section 5) |
| exponent | The stratified log-log slope of section 6.2 |

**Symbols.**

| Symbol | Meaning |
|---|---|
| $`N`$ | The kept data keys of a cell |
| $`H`$ | The historical states of a cell |
| $`d`$ | The keys that a commit replaces |
| $`m`$ | The keys that a commit deletes |
| $`W`$ | The warm-up commits of a child, 4 |
| $`K`$ | The deferred commits of an eviction cell, 8 (0 elsewhere) |
| $`T`$ | The timed commits of a child: 32, or 16 for an eviction cell |
| $`C`$ | The commits of a child: $`W + T`$ for an ordinary cell, $`W + K + 1 + T`$ for an eviction cell |
| $`B`$, $`T`$ in set expressions | The base and target semantic manifests of one commit. $`\mathrm{cl}(S)`$ is the set of objects reachable from the manifest $`S`$, the manifest included, and $`U`$ is the retained union before the commit (as in the catalog) |
| $`X_D`$, $`\mathrm{refs}_D`$ | The objects that the ledger trial of domain $`D`$ expands and the references it charges, for $`D`$ one of current, retained and metadata (the catalog's oracle formulas) |
| $`\mathrm{len}(x)`$ | The length of the canonical bytes of the object $`x`$ |

## 2. The files and the cells

| File | What it holds | Shape |
|---|---|---|
| `…-process-summary.tsv` | One row per time process | 714 rows: 63 of population `warmup` and 651 of population `decision`; 39 columns |
| `…-verify.tsv` | One row per commit of each verify run, beside its child's process record | 1,470 rows, 735 per run; 80 columns |
| `…-analysis.json` | The driver's analysis: qualification, H1, the gates G1 to G10 and the descriptive statistics | One JSON object |
| `…-manifest.json` | The 114 pgmcp artifacts of the archive, with their content digests, and the digests of the three summaries | One JSON object |
| `rspace-fixed-delta-scaling-2026-11-03.md` | This document | |

**The table format.** Each table is UTF-8, tab-separated, with one header line, and every line ends with a line feed.
An empty field means no value. Integers are canonical decimal. The driver's `table_value` writes a median of an even
number of integers as Python writes the float: an integral value as an integer, any other as `repr`, for example
`488741.5`.

**What is not committed.** The raw per-commit time samples (`samples.tsv`, 5,804,354 bytes) and the raw child output
(`children.log`, 14,650,466 bytes as collected) stay out of the repository: section 5.10 of task 7929's reviewed plan
commits summary files only. Both are in the archived decision bundle, and the derivation program reads them from it.
The bundle holds the sanitized copy of `children.log` (section 7.2), which is 14,528,036 bytes long.

**The cells.** All 21 cells use the V2 writer and seed 7929. A cell that belongs to two axes is one cell and one
fixture, and its rows list both axes in the column `axes`. An L cell keeps 36 reserve groups of $`m`$ keys, so its
`reserve` is $`36m`$.

| Cell id | Axes | $`N`$ | $`H`$ | $`d`$ | $`m`$ | Case | $`W`$, $`K`$, $`T`$ |
|---|---|---|---|---|---|---|---|
| `U-k1024-h1024-d1-m0-v2-ordinary` | U | 1,024 | 1,024 | 1 | 0 | ordinary | 4, 0, 32 |
| `U-k4096-h1024-d1-m0-v2-ordinary` | U, R | 4,096 | 1,024 | 1 | 0 | ordinary | 4, 0, 32 |
| `U-k16384-h1024-d1-m0-v2-ordinary` | U, D | 16,384 | 1,024 | 1 | 0 | ordinary | 4, 0, 32 |
| `U-k65536-h1024-d1-m0-v2-ordinary` | U | 65,536 | 1,024 | 1 | 0 | ordinary | 4, 0, 32 |
| `U-k262144-h1024-d1-m0-v2-ordinary` | U | 262,144 | 1,024 | 1 | 0 | ordinary | 4, 0, 32 |
| `R-k4096-h256-d1-m0-v2-ordinary` | R | 4,096 | 256 | 1 | 0 | ordinary | 4, 0, 32 |
| `R-k4096-h4096-d1-m0-v2-ordinary` | R | 4,096 | 4,096 | 1 | 0 | ordinary | 4, 0, 32 |
| `R-k4096-h16384-d1-m0-v2-ordinary` | R | 4,096 | 16,384 | 1 | 0 | ordinary | 4, 0, 32 |
| `R-k4096-h65536-d1-m0-v2-ordinary` | R | 4,096 | 65,536 | 1 | 0 | ordinary | 4, 0, 32 |
| `D-k16384-h1024-d4-m0-v2-ordinary` | D | 16,384 | 1,024 | 4 | 0 | ordinary | 4, 0, 32 |
| `D-k16384-h1024-d16-m0-v2-ordinary` | D | 16,384 | 1,024 | 16 | 0 | ordinary | 4, 0, 32 |
| `D-k16384-h1024-d64-m0-v2-ordinary` | D | 16,384 | 1,024 | 64 | 0 | ordinary | 4, 0, 32 |
| `D-k16384-h1024-d256-m0-v2-ordinary` | D | 16,384 | 1,024 | 256 | 0 | ordinary | 4, 0, 32 |
| `L-k16384-h1024-d0-m1-v2-ordinary` | L | 16,384 | 1,024 | 0 | 1 | ordinary | 4, 0, 32 |
| `L-k16384-h1024-d0-m4-v2-ordinary` | L | 16,384 | 1,024 | 0 | 4 | ordinary | 4, 0, 32 |
| `L-k16384-h1024-d0-m16-v2-ordinary` | L | 16,384 | 1,024 | 0 | 16 | ordinary | 4, 0, 32 |
| `L-k16384-h1024-d0-m64-v2-ordinary` | L | 16,384 | 1,024 | 0 | 64 | ordinary | 4, 0, 32 |
| `L-k16384-h1024-d0-m256-v2-ordinary` | L | 16,384 | 1,024 | 0 | 256 | ordinary | 4, 0, 32 |
| `E-k1024-h1024-d1-m0-v2-evict` | E | 1,024 | 1,024 | 1 | 0 | evict | 4, 8, 16 |
| `E-k16384-h1024-d1-m0-v2-evict` | E | 16,384 | 1,024 | 1 | 0 | evict | 4, 8, 16 |
| `E-k262144-h1024-d1-m0-v2-evict` | E | 262,144 | 1,024 | 1 | 0 | evict | 4, 8, 16 |

The collection ran 777 children in this order:

- one build child per cell (21);
- verify run 1 over all cells, then verify run 2 (42);
- the 3 warm-up rounds and the 31 decision rounds of time processes (714).

A verify run of an ordinary cell writes $`C = 36`$ commit rows, and one of an eviction cell $`C = 29`$. That gives
$`18 \cdot 36 + 3 \cdot 29 = 735`$ rows per run.

## 3. Provenance

The summaries are copies of files that the driver wrote during the collection. The derivation program shows that each
of them follows from the raw child output by the driver's own code, and that the copy is byte-identical to the archived
original:

```text
 ┌─ collection, 2026-11-01 to 2026-11-03 ──┐        ┌─ archive, task 13979 ──────────────────┐
 │ population directory                     │  tar   │ decision-20261101T101751Z.tar           │
 │   children.log         raw child output  │ ─────▶ │   868 archived files, 63 digest rows,   │
 │   samples.tsv          raw time samples  │        │   and MANIFEST.tsv                      │
 │   verify.tsv           verify records    │        └────────────────────┬────────────────────┘
 │   process-summary.tsv  time processes    │                             │ gzip, base64,
 │   analysis.json        the analysis      │                             │ 9 parts
 │   metadata.json        host and protocol │                             ▼
 │   fixtures.tsv         fixture builds    │        ┌─ pgmcp experiment 446 ─────────────────┐
 │   manifest.json        collection digests│        │ artifacts 1616 to 1624                  │
 └──────────────────────────────────────────┘        └────────────────────┬────────────────────┘
                                                                          │ restore (section 7.2)
                                                                          ▼
 ┌─ derivation program, section 8 ─────────────────────────────────────────────────────────────┐
 │ children.log ──▶ classify_child ──┬──▶ verify_rows ────▶ verify.tsv                          │
 │                                   ├──▶ process_row ────▶ process-summary.tsv                 │
 │                                   └──▶ sample_rows ────▶ samples.tsv, checked, not committed │
 │ metadata.json, fixtures.tsv, verify.tsv, process-summary.tsv                                 │
 │                       ──▶ analyze_directory ──▶ write_json ──▶ analysis.json                 │
 │ each regenerated file equals its archived copy and its digest in the collection manifest     │
 └──────────────────────────────────────────────┬──────────────────────────────────────────────┘
                                                ▼
                       the three committed summaries, byte copies
```

`…-manifest.json` is derived from the archive records and the local copies of the archived files. For each archived
file, the program rebuilds the exact text of every part that the archiver uploaded, checks the rebuild three ways and
then hashes it:

```text
 archive-records/<record set>.jsonl
            │ one record per archived file: path, SHA-256, size, gzip SHA-256, artifact ids
            ▼
 the local copy of the file ── its SHA-256 and size must equal the record's
            │
            ▼
 part_texts: gzip (level 9, mtime 0) ─▶ base64 ─▶ slices of 900,000 characters ─▶ header and 76-character lines
            │
            ├──▶ the gzip stream's SHA-256 and size equal the record's
            ├──▶ restoring the parts gives back the file
            └──▶ for the task 13979 bundles, each part's SHA-256 equals the digest recorded before upload
            │
            ▼
 content_sha256 = SHA-256 of each part text's UTF-8 bytes ──▶ …-manifest.json
```

## 4. `…-process-summary.tsv`

A row is one time process, and the rows are in collection order, that is, by ascending `sequence`. For a time process
with timed commits $`t = 1, \dots, T`$, let $`x_{t,p,k}`$ be the reading of clock $`k`$ in phase $`p`$ of commit $`t`$.
The phases are $`P = \{\mathrm{seal}, \mathrm{bind}, \mathrm{prepare}, \mathrm{publish}, \mathrm{release}\}`$. The
driver's `summarize_time_process` computes, for each clock $`k`$ and each phase $`p \in P`$:

```math
\mathtt{k} = \operatorname{median}_{t} \sum_{p \in P} x_{t,p,k},
\qquad
\mathtt{p\_k} = \operatorname{median}_{t} x_{t,p,k}.
```

The first is the process's statistic for the clock: the median, over its timed commits, of the five phases' per-commit
sum. The second is the median of one phase alone. A median of the phase medians is not the sum's median, so the phase
columns do not add up to the statistic.

| Column | Meaning |
|---|---|
| `cell_id` | The cell id |
| `axes` | The cell's axes, comma-separated, for example `U,R` |
| `population` | `warmup` or `decision`; only `decision` rows enter the analysis |
| `round` | The round within the population, from 0 |
| `sequence` | The child's position in collection order, from 0, over all 777 children |
| `exit_status` | The child's exit status, or `timeout`; here always `0` |
| `failure` | Empty for a valid child; otherwise the driver's `failure_text`: `<kind>: <reason>` on one line, from its strict record check |
| `duration_ns` | The child's wall time as the driver measured it, from launch to exit, in nanoseconds |
| `commits` | $`C`$, the commit records of the child |
| `timed_commits` | $`T`$ |
| `thread_ns`, `process_ns`, `wall_ns` and the 15 phase columns | The medians defined above; the table below names each column |
| `deferred_thread_ns` | Eviction cells only: the median, over the $`K`$ deferred commits, of the per-commit thread CPU sum. Empty for other cells |
| `reclaim_thread_ns` | Eviction cells only: the thread CPU sum of the reclaim commit. Empty for other cells |
| `rss_start` | The resident set (`VmRSS`), in bytes, when the measurement window opened, just before the first commit after warm-up |
| `rss_end` | The resident set, in bytes, when the window closed after the last commit |
| `hwm_window` | The high-water mark (`VmHWM`), in bytes, when the window closed |
| `hwm_reset` | `1` when writing 5 to `/proc/self/clear_refs` brought `VmHWM` within one page of `VmRSS`, so that `hwm_window` is the window's own peak; `0` when it did not, so that `hwm_window` is the whole process's peak |
| `swap` | `VmSwap`, in bytes, when the window closed |
| `live_packs` | The live packs after the last commit: the active pack minus the first live pack, plus one |
| `admission_ns` | The wall time, in nanoseconds, of the time child's cold admission of its scratch copy of the fixture |
| `filesystem` | The class of the scratch directory's filesystem by its `statfs` magic: `ext4`, `xfs`, `btrfs` or `other`. The magic that ext2, ext3 and ext4 share counts as `ext4`, and the child refuses tmpfs and ramfs |
| `target_sha256` | The SHA-256 of the line-feed-joined target digests of all the child's commits, in commit order. G2 compares it with verify run 1's chain |

The 18 timing columns, by phase and clock:

| Phase | Thread CPU | Process CPU | Wall |
|---|---|---|---|
| All five, summed per commit | `thread_ns` | `process_ns` | `wall_ns` |
| Seal: rewrite the search paths of the changed keys | `seal_thread_ns` | `seal_process_ns` | `seal_wall_ns` |
| Bind: stage the sealed objects and run the ledger trials | `bind_thread_ns` | `bind_process_ns` | `bind_wall_ns` |
| Prepare: classify the fresh objects and frame the transaction | `prepare_thread_ns` | `prepare_process_ns` | `prepare_wall_ns` |
| Publish: append to the pack and index, and swap the catalog slot | `publish_thread_ns` | `publish_process_ns` | `publish_wall_ns` |
| Release: drop the transactions and the predecessor core | `release_thread_ns` | `release_process_ns` | `release_wall_ns` |

## 5. `…-verify.tsv`

A row is one commit of one verify run. The rows are in collection order: by ascending `sequence`, then by ascending
`commit`. Each verify child prints its process record once, after its last commit, and the driver repeats that
record's 16 fields on each of the child's rows, prefixed `process_`. A verify child checks the work, the sealed
objects, the metadata, the effects, the files and the ledger of every commit before it prints the commit's record, so
the values below have passed those checks (the catalog's verify routine).

**Identification and commit.**

| Column | Meaning |
|---|---|
| `cell_id` | The cell id |
| `axes` | The cell's axes, comma-separated |
| `run` | The verify run, `1` or `2` |
| `sequence` | The child's position in collection order |
| `duration_ns` | The verify child's wall time as the driver measured it, in nanoseconds |
| `commit` | The commit's index within the child, from 0 |
| `label` | `warmup`, `deferred`, `reclaim` or `timed` |
| `target` | The digest of the commit's target semantic manifest, lowercase hexadecimal |

**Work of the three ledger trials.** Each value equals the oracle that the verify routine computes independently: the
references $`\mathrm{refs}_D`$, the expansions $`\lvert X_D \rvert`$ and the quoted bytes
$`\sum_{x \in X_D} \mathrm{len}(x)`$.

| Domain | References | Expansions | Quoted bytes |
|---|---|---|---|
| Current, adding $`T`$ and removing $`B`$ | `cur_refs` | `cur_exp` | `cur_quoted` |
| Retained, adding $`T`$ | `ret_refs` | `ret_exp` | `ret_quoted` |
| Metadata, adding the new anchor and removing the old one | `meta_refs` | `meta_exp` | `meta_quoted` |

**Effects of the commit.**

| Column | Meaning |
|---|---|
| `logical_writes` | The ledger's logical writes: $`\sum_D \mathrm{refs}_D`$ plus the root deltas, 5 for the V2 writer |
| `metadata_bytes` | The bytes of the new metadata: the new retention anchor plus the new radix nodes staged for it |
| `fresh_objects` | The objects that the commit sealed, none of them already stored |
| `fresh_bytes` | The canonical bytes of those objects |
| `pack_delta` | The growth of the active pack's durable tail, in bytes: the prepare header, the mutation, a 48-byte record header and the bytes of each fresh object, the 8-byte trailer and the commit record |
| `index_delta` | The growth of the index's durable tail: 72 bytes per fresh object |
| `live_packs` | The live packs after the commit |
| `old_path` | $`\lvert \mathrm{cl}(B) \setminus \mathrm{cl}(T) \rvert`$ minus `deleted_objects` |
| `new_path` | $`\lvert \mathrm{cl}(T) \setminus \mathrm{cl}(B) \rvert`$ |
| `deleted_objects` | The deleted closure of a commit that removes keys (property SW-03), and 0 for a replacement |
| `loaded_nodes`, `edited_nodes`, `emitted_nodes`, `exact_path_chunks` | The seal's DAG rewrite report: the nodes it loaded, edited and emitted, and the exact path chunks it wrote |
| `unlinked` | The sorted, semicolon-joined names of the files present before the commit and absent after it, or `-` |

So the current expansions satisfy $`\mathit{cur\_exp} = \mathit{old\_path} + \mathit{new\_path} + \mathit{deleted\_objects}`$,
which G1 checks on every row.

**Probes.** Around each of the four phases, the child reads `/proc/self/io`, and inside each phase it counts the
allocations of the test build's thread-local counting allocator. The 32 probe columns, by phase and probe:

| Phase | Allocations | Allocated bytes | Peak live bytes | Largest allocation | Read calls | Write calls | Bytes read | Bytes written |
|---|---|---|---|---|---|---|---|---|
| Seal | `seal_alloc_count` | `seal_alloc_bytes` | `seal_alloc_peak` | `seal_alloc_max` | `seal_syscr` | `seal_syscw` | `seal_rchar` | `seal_wchar` |
| Bind | `bind_alloc_count` | `bind_alloc_bytes` | `bind_alloc_peak` | `bind_alloc_max` | `bind_syscr` | `bind_syscw` | `bind_rchar` | `bind_wchar` |
| Prepare | `prepare_alloc_count` | `prepare_alloc_bytes` | `prepare_alloc_peak` | `prepare_alloc_max` | `prepare_syscr` | `prepare_syscw` | `prepare_rchar` | `prepare_wchar` |
| Publish | `publish_alloc_count` | `publish_alloc_bytes` | `publish_alloc_peak` | `publish_alloc_max` | `publish_syscr` | `publish_syscw` | `publish_rchar` | `publish_wchar` |

The four I/O deltas are the differences of `/proc/self/io`'s `syscr`, `syscw`, `rchar` and `wchar` counters across the
phase [8].

**The process record.**

| Column | Meaning |
|---|---|
| `process_mode` | `verify` |
| `process_commits` | $`C`$ |
| `process_admission_ns` | The wall time, in nanoseconds, of the verify child's cold admission of its scratch directory. Before admitting, the child rebuilds the fixture, checks the fixture files against the rebuild and installs the rebuilt checkpoint image there |
| `process_rss_start`, `process_rss_end`, `process_hwm_window`, `process_hwm_reset`, `process_swap`, `process_live_packs`, `process_filesystem` | As `rss_start`, `rss_end`, `hwm_window`, `hwm_reset`, `swap`, `live_packs` and `filesystem` in section 4, for the verify child |
| `process_probe_syscr`, `process_probe_syscw`, `process_probe_rchar`, `process_probe_wchar` | The cost of one `/proc/self/io` read: the difference of two back-to-back reads. Each phase delta includes exactly one such read |
| `process_probe_alloc_count`, `process_probe_alloc_bytes` | The allocations of the effects observer's trace buffer, which grows inside each publication: 2 allocations and 24 bytes |

**Net values.** G4 and G5 use per-commit values net of the calibrations. With
$`V = \{\mathrm{seal}, \mathrm{bind}, \mathrm{prepare}, \mathrm{publish}\}`$ and a row $`r`$:

```math
\begin{aligned}
\mathrm{net}(r, \mu) &= \sum_{p \in V} r[\mathtt{p\_}\mu] - r[\mathtt{process\_probe\_}\mu],
  && \mu \in \{\mathtt{alloc\_count}, \mathtt{alloc\_bytes}\}, \\
\mathrm{net}(r, \mu) &= \sum_{p \in V} \bigl(r[\mathtt{p\_}\mu] - r[\mathtt{process\_probe\_}\mu]\bigr),
  && \mu \in \{\mathtt{syscr}, \mathtt{syscw}, \mathtt{rchar}, \mathtt{wchar}\}.
\end{aligned}
```

The trace calibration is subtracted once per commit, and the I/O calibration once per phase. A negative net value
fails its gate; it is never clamped.

## 6. `…-analysis.json`

The driver's `analyze` computes this object from four inputs: `metadata.json`, `fixtures.tsv`, the verify records and
the time processes. Each of the two tables above is byte-identical to the population file it copies. The object is
written by the driver's `write_json`: two-space indentation, keys in the driver's insertion order, no NaN, and a
final line feed.

### 6.1 Top-level fields

| Field | Meaning |
|---|---|
| `task` | 7929 |
| `mode` | `decision` |
| `control` | The negative control a control population runs; `null` here |
| `decision_data` | `true`: this population decides H1 |
| `pilot_only` | `false`: this is not a pilot |
| `sample_population` | `decision`, the time population whose processes are the samples |
| `qualified` | `true` when no qualification problem was found. The checks: `metadata.json` records the frozen protocol; the collection completed; no child failed unexpectedly; each cell has exactly one successful fixture build; each time population has exactly its scheduled rounds of each cell, none failed and none unexpected; every verify record names a known cell and run; and every verify run of every cell has records |
| `qualification_problem_count` | The number of qualification problems, 0 |
| `qualification_problems` | The first 50 problems, as text; empty |
| `gates` | H1 and G1 to G10 (section 6.4) |
| `descriptive` | The descriptive statistics (section 6.5) |
| `accepted` | `true` when the population is qualified and H1 and G1 to G9 all pass; `false` here, because G4 fails |

### 6.2 Statistics

**The exponent.** Let an axis have strata $`x_1 < \dots < x_k`$, with $`k \ge 3`$, and $`n \ge 3`$ positive samples
$`y_{i,1}, \dots, y_{i,n}`$ per stratum. The function `stratified_loglog_slope` of
`scripts/bench/paired_benchmark_statistics.py` estimates the slope of the mean log sample against the log stratum:

```math
u_i = \ln x_i - \frac{1}{k} \sum_{j=1}^{k} \ln x_j,
\qquad
\bar{\ell}_i = \frac{1}{n} \sum_{j=1}^{n} \ln y_{i,j},
\qquad
\hat{\beta} = \frac{\sum_{i=1}^{k} u_i \, \bar{\ell}_i}{\sum_{i=1}^{k} u_i^{2}}.
```

A cost that grows like $`x^{\beta}`$ has exponent $`\beta`$, so a cost independent of the stratum has exponent 0 and a
linear one has exponent 1. The interval is a percentile bootstrap [5, 6]. It takes $`R = 10{,}000`$ replicates, drawn
from numpy's `default_rng(7929)`. Each replicate resamples, with replacement, the $`n`$ logs of every stratum and
recomputes $`\hat{\beta}`$; `lower95` and `upper95` are the 2.5% and 97.5% quantiles of the replicates.

**The median interval.** `median_interval` reports a sample's median with a 95% percentile bootstrap interval of the
median, from 10,000 resamples with seed 7929.

**The difference of medians.** `rss_growth_bound` resamples two samples, $`Y_{\mathrm{large}}`$ and
$`Y_{\mathrm{small}}`$, independently and reports the 2.5% and 97.5% quantiles of
$`\operatorname{median}(Y^{*}_{\mathrm{large}}) - \operatorname{median}(Y^{*}_{\mathrm{small}})`$, again from 10,000
resamples with seed 7929.

**The additive threshold.** Let $`\overline{\beta}(c)`$ be H1's upper bound after $`c \cdot x_i`$ nanoseconds are added
to every sample of stratum $`x_i`$. Then $`c^{*}`$ is the smallest $`c`$ on the grid
$`\{2^{j/4} : j = -40, \dots, 80\}`$ with $`\overline{\beta}(c) > 0.25`$. It is 0 when
$`\overline{\beta}(0) > 0.25`$ already, and null when no grid point qualifies. It measures how large a cost per key (on
U) or per retained state (on R) the data would have shown.

### 6.3 Object types

Several gates share the same object shapes. In the paths below, `<axis>` is one of `U`, `R`, `D` and `L`; `<metric>`
is a probe metric; `<run>` is a verify run, `1` or `2`; and `<cell>` is a cell id.

**Slope arm.** One exponent on one axis. It appears at `gates.H1.arms.<axis>`, `gates.G3.arms.<axis>`,
`gates.G8.arms.<axis>` and `gates.G4.metrics.<metric>.runs.<run>.arms.<axis>`, and at the same G5 path.

| Field | Meaning |
|---|---|
| `cells` | The cell ids of the axis's strata, in stratum order |
| `strata` | The stratum values, ascending |
| `samples` | The sample count per stratum: 31 processes for a timing gate, 32 timed commits for G4 and G5 |
| `estimate` | $`\hat{\beta}`$ |
| `lower95`, `upper95` | The bootstrap interval |
| `replicates` | 10,000 |
| `seed` | 7929 |
| `passed` | Whether `upper95` is at most the gate's bound |

When a slope cannot be computed, the arm carries `error` in place of `estimate`, `lower95`, `upper95`, `replicates`
and `seed`, and `passed` is false. No arm of this population does.

**Interval.** A median with its bootstrap interval: `n` (the observations), `median`, `lower95` and `upper95`.

**Spread.** A distribution without an interval: `n`, `median`, `minimum` and `maximum`.

**Child assertion.** A condition that the verify child asserts at every commit, so that it holds exactly when every
verify run completed.

| Field | Meaning |
|---|---|
| `condition` | What the child asserts |
| `asserted_by` | Where it is asserted: in the verify child, at every commit, where a violation fails the child |
| `incomplete_verify_runs` | The verify runs with fewer records than commits; empty |
| `passed` | `true` when that list is empty |

**Memory arm** (G6). One axis's resident-set growth, `rss_end` minus `rss_start`, compared between the axis's
smallest and largest strata.

| Field | Meaning |
|---|---|
| `smallest`, `largest` | The cell ids of the two strata |
| `difference_of_medians` | The largest stratum's median growth minus the smallest's, in bytes |
| `lower2_5`, `upper97_5` | The bootstrap interval of that difference |
| `bound` | 1,048,576 bytes, 1 MiB |
| `replicates`, `seed` | 10,000 and 7929 |
| `passed` | Whether `upper97_5` is at most `bound` |

**Additive threshold** (at `descriptive.c_star.<axis>`, for U and R).

| Field | Meaning |
|---|---|
| `axis` | `U` or `R` |
| `unit` | The unit of $`c^{*}`$: `ns per key` or `ns per state` |
| `strata` | The stratum values |
| `c_star` | $`c^{*}`$ |
| `grid_index` | The index of $`c^{*}`$ in the grid, from 0 for $`2^{-10}`$ |
| `at_grid_floor` | Whether $`c^{*}`$ is the smallest grid value |
| `baseline_exceeds` | Whether $`\overline{\beta}(0) > 0.25`$ |
| `upper95_at_c_star` | $`\overline{\beta}(c^{*})`$ |
| `largest_stratum`, `largest_stratum_median_ns` | The largest stratum and its median thread CPU statistic |
| `percent_of_largest_stratum_median` | $`100 \, c^{*} x_k`$ divided by that median: the added cost as a percentage of the measured cost at the largest stratum |

**Crossover stratum** (at `gates.G7.crossover.strata[]`, one per R stratum).

| Field | Meaning |
|---|---|
| `cell` | The cell id |
| `writer` | `v2` |
| `history` | $`H`$ |
| `metadata_bytes_median` | The median `metadata_bytes` over the cell's timed verify commits |
| `v1_manifest_bytes_median` | The median, over the same commits $`c`$, of $`96 + 56(H + c + 1)`$: the bytes that the V1 writer would have written |
| `v1_over_v2` | `v1_manifest_bytes_median` divided by `metadata_bytes_median` |
| `crossover_entries` | The smallest retained-entry count whose V1 manifest exceeds the V2 median |

### 6.4 The gates

The catalog's gate table states each gate's samples and pass rule. The fields:

| Gate | Fields |
|---|---|
| `H1` | `metric` (the sample: each decision process's `thread_ns`), `statistic` (`upper95`), `bound` (0.25), `arms` (U and R, each a slope arm), `passed` |
| `G1` | `verify_runs` (2), `problem_count`, `problems` (the first 50), `passed`. It passes when every run of every cell has one record per commit, in order and labeled as scheduled. Each record must also satisfy the expansion identity of section 5, report no deleted closure in a cell without deletions, and carry a process record of mode `verify` with $`C`$ commits |
| `G2` | `compared_verify_fields`: the 27 commit-record fields that both runs must agree on. They are `commit`, `label`, `target`, the nine work columns and the 15 effect columns, that is, every field of the record except the 32 probes. Then `verify_runs_identical`, `verify_run_mismatches`, `time_chain_mismatch_count`, `time_chain_mismatches` (time processes whose `target_sha256` differs from verify run 1's chain) and `passed` |
| `G3` | As H1, on `wall_ns` |
| `G4` | `statistic`, `bound`, `samples` (a description of the samples), `metrics` (`alloc_count` and `alloc_bytes`, each with `runs`, each run with `arms` for U and R and its own `passed`, and the metric's `passed`), `passed` |
| `G5` | As G4 for `syscr`, `syscw`, `rchar` and `wchar`, plus `appends`, the child assertion that pack and index growth equal the encoded appends |
| `G6` | `arms` (U and R, each a memory arm), `swap_nonzero` (time processes and verify children that reported nonzero swap), `oom_killed` (children that failed with kind `oom`: exit status 137 or signal 9), `passed` |
| `G7` | `v2_metadata_byte_bound` (518,500), `v2_step_work_bound` (103, the bound on $`2 + \mathit{meta\_exp}`$), `v1_manifest_bytes` (the V1 formula as text), `problem_count`, `problems`, `model` (the child assertion that the sealed metadata equal the writer model's bytes), `crossover` (`definition`, the rule as text, and `strata`), `passed` |
| `G8` | `statistic`, `bound` (1.25), `arms` (D and L, each a slope arm on `thread_ns`), `problem_count`, `problems` (L commits whose deleted closure is empty or differs from the excess current expansions), `passed` |
| `G9` | `problem_count`, `problems`, `trusted_effects` (the child assertion that the publication effects are the trusted effects), `within_process_thread_cpu_differences` (per E cell, `deferred_minus_timed_ns` and `reclaim_minus_timed_ns`, each an interval over the processes of `deferred_thread_ns` or `reclaim_thread_ns` minus `thread_ns`, reported with no threshold), `passed` |
| `G10` | `applicable` (`false`: each control is its own population), `passed` (`null`), `reason` |

### 6.5 Descriptive statistics

`descriptive` has two fields. `c_star` holds one additive threshold for U and one for R. `per_cell` holds one object
per cell, keyed by the cell id, with these fields:

| Field | Meaning |
|---|---|
| `axes` | The cell's axes |
| `cell` | The cell's twelve parameters, as `key=value` pairs |
| `sample_processes` | The successful decision processes, 31 |
| `timing_ns` | For each of the 18 timing columns of section 4, an interval over the decision processes |
| `admission_ns` | An interval of `admission_ns` over the decision processes |
| `sealing_counters` | For `loaded_nodes`, `edited_nodes`, `emitted_nodes` and `exact_path_chunks`, a spread over the timed verify commits of both runs |
| `live_packs` | The distinct live-pack counts that the decision processes and verify children reported |
| `hwm_reset` | The number of decision processes with each `hwm_reset` value that occurs, keyed `0` or `1` |
| `feasibility` | `build_duration_ns` (the build child's wall time, as a one-element list), `verify_duration_ns_max`, `time_duration_ns_max`, `time_hwm_window_max` and `verify_hwm_window_max`: the largest wall times and high-water marks, which bound each cell's cost for the timeouts and memory caps |

## 7. `…-manifest.json`

### 7.1 Fields

The program writes the object with two-space indentation, ASCII only, keys in the order below, and a final line feed.

| Field | Meaning |
|---|---|
| `task` | 7929 |
| `experiment` | 446 |
| `measured_commit` | The commit whose code was measured |
| `decision_bundle` | The bundle that holds the decision population |
| `decision_data_finished_utc` | When the collection finished, from the bundle's `metadata.json` |
| `summaries` | One entry per committed summary: `file` (its name), `sha256` and `bytes` (of the committed file), and `member` (the bundle member that it copies byte for byte) |
| `artifact_count` | The number of artifacts listed, 114 |
| `archives` | One entry per archived file, in ascending order of its first artifact id |

An entry of `archives`:

| Field | Meaning |
|---|---|
| `record_set` | The record set that archived the file, or `text` for an artifact that holds one text file verbatim |
| `bundle`, `description`, `source_files`, `archived_files`, `sha256_only_files` | Task 13979 bundles only: the bundle's name, its description, and its source's file count split into files archived whole and fixture files listed by digest only |
| `path` | The file's path under `target/perf/logs/7929`, or a text artifact's file name |
| `format` | `gzip+base64` for parts, `text` for a verbatim text file |
| `file_sha256`, `file_bytes` | The SHA-256 and size of the archived file |
| `gzip_sha256`, `gzip_bytes` | Parts only: the SHA-256 and size of its gzip stream |
| `artifacts` | One entry per artifact: `artifact_id`, `part` and `parts` (its position, counted from 1), `content_sha256` (its content digest, the value that pgmcp stores) and `content_bytes` (the UTF-8 length of its content) |

`content_sha256` is the artifact's own digest. `file_sha256` and `gzip_sha256` identify what a restore yields, so that
a reader can check each step.

The record sets:

| Record set | Artifacts | Contents |
|---|---|---|
| `pilot-20261029T085245Z` | 1159 to 1172 | The treatment pilot's population files, its CPU snapshots and feasibility notes, a uProf trial and the c36 measurement scripts |
| `pilot-nc-v1-20261029T120209Z` | 1180 to 1191 | The NC-V1 control pilot |
| `pilot-nc-scan-20261029T122846Z` | 1192 to 1203 | The first NC-SCAN control pilot |
| `pilot-nc-scan-20261029T155714Z` | 1204 to 1215 | The NC-SCAN control pilot after the revision r1 build |
| `pilot-nc-clone-20261029T133559Z` | 1216 to 1227 | The NC-CLONE control pilot |
| `pilot-nc-order-20261029T144430Z` | 1228 to 1239 | The NC-ORDER control pilot |
| `control-builds` | 1240 to 1255 | The control binaries' build logs and pipeline outputs, the SW-01 run against NC-ORDER, `control-pipeline.log` and `lane.log` |
| `measurement-scripts-c41` | 1256 | The control patches and the build, pipeline and archive scripts |
| `text` | 1257 | `archive-manifest-control-pilots.md`, the manifest of artifacts 1180 to 1256 |
| `diagnostics-5-9` | 1303 | The section 5.9 diagnostics: AMD uProf, heaptrack and strace |
| `r10-13979` | 1616 to 1633 | The ten bundles of task 13979: the decision population (9 parts), the four control populations, the three interrupted NC-ORDER attempts, the lane records and the lane scripts |
| `pilot-20261029T085245Z-addendum` | 1634 | The treatment pilot's `manifest.json`, which the first pilot archive left out |
| `text` | 1635, 1636 | `manifest-13979.md`, the top-level archive manifest, and `manifest-13979.py`, its generator and coverage audit |

Three artifacts record decisions rather than archived data, so they are not listed: the feasibility amendments
(artifact 1173), the control-pilot addendum (artifact 1178) and the section 5.9 outcome note (artifact 1304).

### 7.2 The part format and restoring a file

The archivers split an archived file's encoding into parts. They compress the file with gzip at level 9 and
modification time 0, so the stream is a function of the file's bytes alone [2, 3]. They encode the stream in base64
and cut the encoding into slices of at most 900,000 characters. Each part's content is a header, then its slice in
lines of 76 characters, then a final line feed:

```text
archive-format: gzip+base64, part <i> of <n>
file: <the file's name>
file-sha256: <the file's SHA-256>
file-bytes: <the file's size>
gzip-sha256: <the gzip stream's SHA-256>
gzip-bytes: <the gzip stream's size>
restore: concatenate the base64 payload lines of parts 1..n in order, base64-decode, gunzip, check both digests
payload:
<the slice, 76 characters per line>
```

To restore a file, follow the header's own instruction:

```text
RESTORE(part 1, …, part n):
  payload ← the empty string
  for i ← 1 to n:
      body ← the text of part i after its first "\npayload:\n"
      payload ← payload followed by every nonempty line of body
  stream ← base64-decode(payload), refusing any character outside the alphabet
  require SHA-256(stream) = gzip-sha256
  file ← gunzip(stream)
  require SHA-256(file) = file-sha256
  return file
```

A bundle restores to a tar whose members are the population's files under `<bundle>/<population>/`. Its
`MANIFEST.tsv` lists every source file with its source and archived digests. The archiver sanitized the paths in text
files: the repository root became `.` and the home directory `~`. In the decision bundle this changed exactly two
files, `children.log` and `metadata.json`. The three summaries hold no path, so their archived digests equal their
source digests.

## 8. The derivation program

The program regenerates the three summaries from the raw child output in the decision bundle and writes
`…-manifest.json`. Following literate programming [7], it is presented here in pieces, each explained before it is
shown. The program is the concatenation, in order, of this document's `python` code blocks, joined by two blank lines.
Section 9 shows how to extract and run it.

**The algorithm in outline.**

```text
DERIVE(repository, logs, work, output, text artifacts):
  require that work and output do not exist
  records ← every record of logs/archive-records
  b ← the one record of the decision bundle; require SHA-256(its tar) = b.sha256
  members ← the tar's members; metadata ← members[metadata.json]
  require SHA-256(each driver source) = metadata.host.source_sha256[that source]
  load the driver module
  for each child block of members[children.log], in order:
      (commit records, failure) ← the driver's classify_child(block)
      require that this outcome is the outcome the block logged
      append the child's rows to verify.tsv, process-summary.tsv and samples.tsv in work
  copy metadata.json and fixtures.tsv into work
  analysis.json ← write_json(analyze_directory(work))
  for each regenerated file f:
      require f = members[f], and SHA-256(f) and size(f) = the collection manifest's entry for f
  for each record r:
      file ← the local copy of r's file, checked against r.sha256 and r.bytes
      parts ← PART-TEXTS(file); require the gzip digest, the restore and any recorded part digests
      entry ← the record's fields and each part's SHA-256
  write the three summaries and …-manifest.json into output
```

**Prelude.** The program uses only the standard library, apart from the driver, which needs numpy. The constants name
the decision bundle, its member directory and the measured commit, and they fix the output names. `REGENERATED` lists
the population files that the program rebuilds: the three summaries and `samples.tsv`, which is rebuilt only to be
checked. `DRIVER_SOURCES` are the three Python files of the analysis, the ones whose digests the collection recorded in
`metadata.json`. `PART_CHARACTERS`, `LINE` and `RESTORE_LINE` reproduce the archivers' part format.

```python
import base64
import gzip
import hashlib
import importlib.util
import json
import re
import sys
import tarfile
from pathlib import Path

TASK = 7929
EXPERIMENT = 446
MEASURED_COMMIT = "941ff07d821f9b7b5e8dc5d0a8e62310c328b32c"
DECISION_BUNDLE = "decision-20261101T101751Z"
POPULATION = f"{DECISION_BUNDLE}/{DECISION_BUNDLE}"
PREFIX = "rspace-fixed-delta-scaling-2026-11-03"
SUMMARIES = ("process-summary.tsv", "verify.tsv", "analysis.json")
REGENERATED = ("verify.tsv", "samples.tsv", "process-summary.tsv", "analysis.json")
DRIVER = "scripts/bench/rspace-fixed-delta-scaling.py"
DRIVER_SOURCES = (DRIVER, "scripts/bench/paired_benchmark_statistics.py", "scripts/bench/benchmark_host.py")
LOGS_PREFIX = "target/perf/logs/7929/"
PART_CHARACTERS = 900_000
LINE = 76
RESTORE_LINE = ("restore: concatenate the base64 payload lines of parts 1..n in order, base64-decode, gunzip, "
                "check both digests")
```

**Digests, refusals and tar members.** `sha256` returns the lowercase hexadecimal digest of some bytes [1]. Every
check goes through `require`, which stops the program with a message that starts `refused:`. A refused run writes no
output directory. `tar_members` reads every regular file of a tar into memory by member name. It never extracts to
disk, so a member name cannot write outside the program's directories.

```python
def sha256(data):
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise SystemExit(f"refused: {message}")


def tar_members(path):
    members = {}
    with tarfile.open(path) as archive:
        for info in archive.getmembers():
            if info.isfile():
                handle = archive.extractfile(info)
                if handle is None:
                    raise SystemExit(f"refused: {path.name}: cannot read {info.name}")
                members[info.name] = handle.read()
    return members
```

**The part format.** `part_texts` is the archivers' encoder: it returns the gzip stream and the exact text of every
part (section 7.2). The text depends only on the file's name and bytes, because the stream depends only on the bytes.
`restore` is the decoder. The program uses it to check that the rebuilt parts give back the file.

```python
def part_texts(name, data):
    packed = gzip.compress(data, compresslevel=9, mtime=0)
    encoded = base64.b64encode(packed).decode("ascii")
    chunks = [encoded[start:start + PART_CHARACTERS] for start in range(0, len(encoded), PART_CHARACTERS)] or [""]
    texts = []
    for index, chunk in enumerate(chunks, start=1):
        header = [
            f"archive-format: gzip+base64, part {index} of {len(chunks)}",
            f"file: {name}",
            f"file-sha256: {sha256(data)}",
            f"file-bytes: {len(data)}",
            f"gzip-sha256: {sha256(packed)}",
            f"gzip-bytes: {len(packed)}",
            RESTORE_LINE,
            "payload:",
        ]
        lines = [chunk[start:start + LINE] for start in range(0, len(chunk), LINE)]
        texts.append("\n".join(header) + "\n" + "\n".join(lines) + "\n")
    return packed, texts


def restore(texts):
    payload = []
    for text in texts:
        body = text.split("\npayload:\n", 1)[1]
        payload.extend(line for line in body.split("\n") if line)
    return gzip.decompress(base64.b64decode("".join(payload), validate=True))
```

**The archive records.** Each line of a record file is one archived file:

- a per-file record carries `path`, `sha256`, `bytes`, `gzip_sha256`, `gzip_bytes` and `artifacts`;
- a bundle record also carries `bundle`, `tar` and `part_content_sha256`, the digest of each part text, computed before
  upload from the exact string that was sent.

Paths were recorded relative to the log directory, relative to the repository or absolute. `logs_relative` reduces each
to the log directory, and `archived_bytes` reads the file and checks it against its record.

One file, `lane.log`, kept growing after it was archived. The archived file is its first 2,922 bytes, and only the
recorded digest can identify that prefix. So when a file is longer than its record and its prefix of the recorded size
has the recorded digest, that prefix is the archived file. Any other mismatch is refused.

```python
def load_records(logs):
    records = []
    for record_file in sorted((logs / "archive-records").glob("*.jsonl")):
        for line in record_file.read_text(encoding="utf-8").splitlines():
            records.append((record_file.stem, json.loads(line)))
    return records


def logs_relative(logs, text):
    path = Path(text)
    if path.is_absolute():
        return path.relative_to(logs).as_posix()
    if text.startswith(LOGS_PREFIX):
        return text[len(LOGS_PREFIX):]
    return text


def archived_bytes(logs, relative, size, digest):
    data = (logs / relative).read_bytes()
    if len(data) > size and sha256(data[:size]) == digest:
        data = data[:size]
    require(len(data) == size and sha256(data) == digest, f"{relative} differs from its archive record")
    return data
```

**The manifest entries.** `archive_entry` rebuilds an archived file's part texts and checks them three ways:

- the gzip stream must have the recorded digest and size;
- the parts must restore the file;
- there must be one part per recorded artifact.

For a bundle, every part text's digest must also equal the digest recorded before upload. This independent check
shows that the rebuilt text is exactly what was sent. The entry then lists each artifact with its content digest: the
SHA-256 of the part text's UTF-8 bytes, which is the value pgmcp stored on receipt.

`text_entry` handles an artifact that holds a text file verbatim. Its argument has the form `ID=FILE`. The uploader
sent the file's text decoded from UTF-8, so the content digest is the file's own SHA-256. The program refuses a file
that is not UTF-8.

```python
def archive_entry(record_set, relative, data, record, details):
    packed, texts = part_texts(Path(relative).name, data)
    require(sha256(packed) == record["gzip_sha256"] and len(packed) == record["gzip_bytes"],
            f"{relative}: the gzip stream differs from its archive record")
    require(restore(texts) == data, f"{relative}: the part texts do not restore the file")
    require(len(texts) == len(record["artifacts"]),
            f"{relative}: {len(texts)} parts for {len(record['artifacts'])} artifacts")
    digests = [sha256(text.encode("utf-8")) for text in texts]
    if "part_content_sha256" in record:
        require(digests == record["part_content_sha256"], f"{relative}: the part digests differ from the recorded ones")
    artifacts = [{"artifact_id": identifier, "part": index, "parts": len(texts), "content_sha256": digest,
                  "content_bytes": len(text.encode("utf-8"))}
                 for index, (identifier, text, digest) in enumerate(zip(record["artifacts"], texts, digests), start=1)]
    return {"record_set": record_set, **details, "path": relative, "format": "gzip+base64",
            "file_sha256": sha256(data), "file_bytes": len(data), "gzip_sha256": sha256(packed),
            "gzip_bytes": len(packed), "artifacts": artifacts}


def text_entry(argument):
    identifier, separator, name = argument.partition("=")
    require(separator == "=" and identifier.isdigit(), f"{argument!r} is not ID=FILE")
    data = Path(name).read_bytes()
    try:
        data.decode("utf-8")
    except UnicodeDecodeError:
        require(False, f"{name} is not UTF-8 text")
    return {"record_set": "text", "path": Path(name).name, "format": "text", "file_sha256": sha256(data),
            "file_bytes": len(data),
            "artifacts": [{"artifact_id": int(identifier), "part": 1, "parts": 1, "content_sha256": sha256(data),
                           "content_bytes": len(data)}]}
```

**Loading the driver.** The summaries come from the driver's own functions, so their meaning is the driver's. Before
importing the driver, the program checks its three Python sources against the digests in the bundle's
`metadata.json`, which the collection recorded at its start. This proves that the regenerating code is the code that
collected and analyzed the population. The driver puts its own directory on the module path to import its two helper
modules, and those are the other two checked files.

```python
def load_driver(repository, metadata):
    recorded = metadata["host"]["source_sha256"]
    for relative in DRIVER_SOURCES:
        require(sha256((repository / relative).read_bytes()) == recorded[relative],
                f"{relative} differs from the source that the collection recorded")
    specification = importlib.util.spec_from_file_location("rspace_fixed_delta_scaling", repository / DRIVER)
    if specification is None or specification.loader is None:
        raise SystemExit(f"refused: {DRIVER} cannot be loaded")
    driver = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(driver)
    return driver
```

**Reading `children.log`.** The driver's `ChildRunner` logged each child as a block:

```text
=== child <sequence:05d>-<mode>-<population>-<index:02d>-<cell id> ===
command: <the command>
environment: <RSPACE_SCALE_CELL, _MODE, _FIXTURE and, except for a build child, _SCRATCH, as NAME=value>
status: <exit status or timeout> duration_ns=<wall time> outcome=<ok, or the failure text>
--- output ---
<the child's complete output, ending with a line feed>
=== end child <the same label> ===
```

`child_blocks` parses the blocks in order. It refuses any line outside a block and any malformed preamble, and it
returns each block's label, status, duration, logged outcome and output. The output is joined from its lines with a
line feed after each, so it equals the logged output. The driver's parser reads only the lines that start with
`SCALE_`, so the line feed that the logger adds to an unterminated output changes nothing.

```python
CHILD_OPEN = re.compile(r"=== child (\S+) ===")
CHILD_STATUS = re.compile(r"status: (\S+) duration_ns=(0|[1-9][0-9]*) outcome=(.*)")


def child_blocks(text):
    lines = text.split("\n")
    require(lines[-1] == "", "children.log does not end with a line break")
    blocks, position = [], 0
    while position < len(lines) - 1:
        opened = CHILD_OPEN.fullmatch(lines[position])
        if opened is None:
            raise SystemExit(f"refused: children.log line {position + 1} does not open a child")
        label = opened.group(1)
        status = CHILD_STATUS.fullmatch(lines[position + 3])
        if status is None or not (lines[position + 1].startswith("command: ")
                                  and lines[position + 2].startswith("environment: ")
                                  and lines[position + 4] == "--- output ---"):
            raise SystemExit(f"refused: child {label}: malformed preamble")
        end = lines.index(f"=== end child {label} ===", position + 5)
        output = "".join(line + "\n" for line in lines[position + 5:end])
        blocks.append((label, status.group(1), int(status.group(2)), status.group(3), output))
        position = end + 1
    return blocks
```

**Regenerating the population files.** `regenerate` replays the collection's bookkeeping over the logged children,
with the driver's functions. For each child, in order:

1. It rebuilds the frozen protocol from the mode and control recorded in `metadata.json`. It takes the child's cell
   from the protocol, and the sequence, mode, population and index from the child's label.
2. It reclassifies the child with `classify_child`, the driver's strict parser of the record contract. The result,
   rendered as the logger rendered it, must equal the logged outcome.
3. It writes the child's rows with the functions that wrote them during the collection: `verify_rows` for a verify
   child, and `process_row` and `sample_rows` for a time child. A build child adds no row to these tables.
   `fixtures.tsv` is not rebuilt, because it records digests of the fixture stores, which are not archived. Its
   archived copy is an analysis input.

The tables go through the driver's `TableWriter` into the work directory. That directory then holds a population
directory's analysis inputs, so `analyze_directory` and `write_json` produce `analysis.json` exactly as the driver's
`--analyze-only` does.

Each regenerated file must equal two things:

- **its archived copy**, which proves that the committed bytes are the archived bytes;
- **its entry in the collection manifest**, which the driver wrote before anything was archived and which proves that
  the archived bytes are the collected ones.

The check of `samples.tsv` proves that the raw samples and the child output agree, even though the samples are not
committed.

```python
def regenerate(driver, members, metadata, work):
    protocol = driver.build_protocol(metadata["protocol"]["mode"], metadata["protocol"]["control"])
    cells = {cell["id"]: cell for cell in protocol["cells"]}
    tables = {name: driver.TableWriter(work / name, driver.TABLE_COLUMNS[name])
              for name in ("verify.tsv", "samples.tsv", "process-summary.tsv")}
    blocks = child_blocks(members[f"{POPULATION}/children.log"].decode("utf-8"))
    require(len(blocks) == metadata["collection"]["children"], "children.log and metadata.json count different children")
    for expected, (label, status, duration, logged, output) in enumerate(blocks):
        sequence, mode, population, index, cell_id = label.split("-", 4)
        require(int(sequence) == expected and cell_id in cells, f"child {label} is out of order or names no cell")
        timed_out = status == "timeout"
        records, failure = driver.classify_child(mode, cells[cell_id], None if timed_out else int(status), timed_out,
                                                 output)
        require((driver.failure_text(failure) or "ok") == logged, f"child {label}: its outcome differs from the log")
        outcome = driver.ChildOutcome(expected, label, status, duration, records, failure)
        if mode == "verify" and records is not None:
            for row in driver.verify_rows(cells[cell_id], int(index), outcome):
                tables["verify.tsv"].write(row)
        elif mode == "time":
            tables["process-summary.tsv"].write(driver.process_row(cells[cell_id], population, int(index), outcome))
            if records is not None:
                for row in driver.sample_rows(cells[cell_id], population, int(index), outcome):
                    tables["samples.tsv"].write(row)
    for table in tables.values():
        table.close()
    for name in ("metadata.json", "fixtures.tsv"):
        (work / name).write_bytes(members[f"{POPULATION}/{name}"])
    driver.write_json(work / "analysis.json", driver.analyze_directory(work))
    listed = json.loads(members[f"{POPULATION}/manifest.json"])["files"]
    derived = {}
    for name in REGENERATED:
        data = (work / name).read_bytes()
        require(data == members[f"{POPULATION}/{name}"], f"the regenerated {name} differs from the archived one")
        require(sha256(data) == listed[name]["sha256"] and len(data) == listed[name]["bytes"],
                f"the regenerated {name} differs from the collection manifest")
        derived[name] = data
    return derived
```

**The main program.** `main` takes four directories and any number of text artifacts:

- the repository root, from which the driver is loaded;
- the log directory, which holds `archive-records/` and the local copies of the archived files;
- a new work directory, which keeps the rebuilt population files for inspection;
- a new output directory for the four derived files.

It first locates the decision bundle through its record and checks the tar's digest. It then regenerates the
summaries and builds one manifest entry per record and per text artifact. It refuses a duplicate artifact id, and only
after every check has passed does it create the output directory and write the files.

```python
def main():
    require(len(sys.argv) >= 5, "usage: derive.py <repository> <logs> <work> <output> [ID=FILE]...")
    repository, logs, work, output = (Path(argument).resolve() for argument in sys.argv[1:5])
    require(not work.exists() and not output.exists(), "the work and output directories must not exist")
    records = load_records(logs)
    decision = [record for _, record in records if record.get("bundle") == DECISION_BUNDLE]
    require(len(decision) == 1, "the archive records name the decision bundle other than once")
    bundle = logs / decision[0]["tar"]
    require(sha256(bundle.read_bytes()) == decision[0]["sha256"], "the decision bundle differs from its archive record")
    members = tar_members(bundle)
    metadata = json.loads(members[f"{POPULATION}/metadata.json"])
    driver = load_driver(repository, metadata)
    work.mkdir(parents=True)
    derived = regenerate(driver, members, metadata, work)
    entries = []
    for record_set, record in records:
        if "bundle" in record:
            relative = record["tar"]
            details = {key: record[key] for key in ("bundle", "description", "source_files", "archived_files",
                                                    "sha256_only_files")}
        else:
            relative = logs_relative(logs, record["path"])
            details = {}
        data = archived_bytes(logs, relative, record["bytes"], record["sha256"])
        entries.append(archive_entry(record_set, relative, data, record, details))
    entries.extend(text_entry(argument) for argument in sys.argv[5:])
    entries.sort(key=lambda entry: entry["artifacts"][0]["artifact_id"])
    identifiers = [artifact["artifact_id"] for entry in entries for artifact in entry["artifacts"]]
    require(len(identifiers) == len(set(identifiers)), "an artifact id appears twice")
    manifest = {
        "task": TASK,
        "experiment": EXPERIMENT,
        "measured_commit": MEASURED_COMMIT,
        "decision_bundle": DECISION_BUNDLE,
        "decision_data_finished_utc": metadata["collection"]["finished_utc"],
        "summaries": [{"file": f"{PREFIX}-{name}", "sha256": sha256(derived[name]), "bytes": len(derived[name]),
                       "member": f"{POPULATION}/{name}"} for name in SUMMARIES],
        "artifact_count": len(identifiers),
        "archives": entries,
    }
    output.mkdir(parents=True)
    for name in SUMMARIES:
        (output / f"{PREFIX}-{name}").write_bytes(derived[name])
    (output / f"{PREFIX}-manifest.json").write_bytes((json.dumps(manifest, indent=2) + "\n").encode("ascii"))
    print(json.dumps({"artifacts": len(identifiers), "files": sorted(path.name for path in output.iterdir())}))


if __name__ == "__main__":
    main()
```

## 9. Reproducing and verifying

**Extracting and running.** Extract the program from this document, then run it from the repository root. The log
directory must hold the archive records and the archived files, restored from the artifacts with section 7.2 where the
local copies are gone. The three text artifacts are passed as files:

```console
$ python3 -I -c 'import re, sys; text = open(sys.argv[1], encoding="utf-8").read(); print("\n\n".join(re.findall(r"^```python\n(.*?)^```$", text, re.M | re.S)), end="")' docs/design/stack-safety/measurements/rspace-fixed-delta-scaling-2026-11-03.md > derive.py
$ python3 -I derive.py . target/perf/logs/7929 <new work directory> <new output directory> \
    1257=<archive-manifest-control-pilots.md> 1635=<manifest-13979.md> 1636=<manifest-13979.py>
```

The run takes about five seconds. `-I` isolates the interpreter from the environment and from the user's site
packages.

**Why the output is byte-identical.** Every input is pinned:

- the decision bundle by its digest;
- the three driver sources by the digests that the collection recorded;
- the archived files by their records.

The computation is deterministic. The tables use integer arithmetic, `statistics.median` and Python's float `repr`.
The bootstraps draw from numpy's `default_rng(7929)`. numpy does not promise the same stream from a `Generator` across
versions, so the byte identity of `…-analysis.json` holds for the versions that the collection recorded in
`metadata.json`: Python 3.14.9 and numpy 2.5.4. Those are also the versions that reproduced it for this commit.

**What was checked for this commit.**

1. The tangled program regenerated the four derived files, and they are byte-identical to the committed files. A
   second run in fresh directories produced the same bytes.
2. Inside the program, every check of section 8 passed:
   - the bundle's digest and the driver sources' digests;
   - the logged outcome of all 777 children;
   - the four regenerated population files against their archived copies and the collection manifest;
   - every part's gzip stream and restore;
   - the 18 bundle parts' digests against those recorded before upload.
3. Independently of the program:
   - the content digests of artifacts 1257, 1635 and 1636 equal the digests that their uploads printed;
   - `…-manifest.json` lists exactly the 114 artifacts 1159 to 1172, 1180 to 1257, 1303 and 1616 to 1636, each once.
4. Negative controls: each of these tamperings is refused for its stated reason:
   - a changed byte in the decision bundle;
   - the same change with the bundle's record updated to match;
   - a changed archived file;
   - a wrong gzip digest;
   - a wrong recorded part digest;
   - a changed driver source.

   A changed byte in a copy of a committed summary is caught by the byte comparison.

**The limit of the digest check.** No pgmcp MCP tool returns an artifact's stored `content_sha256`. pgmcp's REST
experiment detail, `GET /api/experiments/{slug}`, returns it, but that route is gated by the web console's token, and
reading the database needs a database role. This check used neither. The digests in `…-manifest.json` are
therefore the SHA-256 values of the rebuilt part texts. They equal the stored values because of three facts:

- pgmcp stores the SHA-256 of exactly the content it receives;
- the archivers sent the texts that `part_texts` rebuilds, from inputs pinned by the file and gzip digests;
- for the bundles, the rebuilt digests equal the digests computed before upload from the strings that were sent.

A reader with database access can compare each `content_sha256` with the stored column directly.

**Durability.** The local copies under `target/perf/logs/7929` are scratch data and may be deleted. The durable copies
are the pgmcp artifacts listed in `…-manifest.json`, and the committed summaries.

## 10. References

1. National Institute of Standards and Technology. *Secure Hash Standard (SHS)*. FIPS PUB 180-4, 2015.
   [doi:10.6028/NIST.FIPS.180-4](https://doi.org/10.6028/NIST.FIPS.180-4).
2. P. Deutsch. *GZIP file format specification version 4.3*. RFC 1952, 1996.
   [doi:10.17487/RFC1952](https://doi.org/10.17487/RFC1952).
3. P. Deutsch. *DEFLATE Compressed Data Format Specification version 1.3*. RFC 1951, 1996.
   [doi:10.17487/RFC1951](https://doi.org/10.17487/RFC1951).
4. S. Josefsson. *The Base16, Base32, and Base64 Data Encodings*. RFC 4648, 2006.
   [doi:10.17487/RFC4648](https://doi.org/10.17487/RFC4648).
5. B. Efron. Bootstrap methods: another look at the jackknife. *The Annals of Statistics* 7(1):1–26, 1979.
   [doi:10.1214/aos/1176344552](https://doi.org/10.1214/aos/1176344552).
6. B. Efron and R. J. Tibshirani. *An Introduction to the Bootstrap*. Chapman and Hall/CRC, 1993.
   [doi:10.1201/9780429246593](https://doi.org/10.1201/9780429246593).
7. D. E. Knuth. Literate programming. *The Computer Journal* 27(2):97–111, 1984.
   [doi:10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97).
8. The Linux man-pages project. *proc_pid_io(5)* and *proc_pid_status(5)*: the `/proc/<pid>/io` counters and the
   `VmRSS`, `VmHWM` and `VmSwap` fields. <https://man7.org/linux/man-pages/man5/proc_pid_io.5.html>,
   <https://man7.org/linux/man-pages/man5/proc_pid_status.5.html>.

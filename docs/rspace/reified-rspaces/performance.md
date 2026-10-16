# Reified RSpaces performance

This document reports what the space operations of Reified RSpaces cost in time, phlo and host work, and what bounds them.
Phase P10 measures, as step S8 of its design v2 states ([decision 85](decision-register.md#decision-85)).
An optimization needs its own hypothesis and its own decision, so this document adopts none.
Section 8 lists the findings and the candidate optimizations for a decision of the user.

Read the [glossary](glossary.md) first, because this document uses its terms.
The [specification](specification.md) states the behavior that the benchmarks exercise, and the [verification dossier](verification.md#357-step-s8-the-benchmarks) records the step.

## Contents

- [1. Scope](#1-scope)
- [2. Method](#2-method)
- [3. The cost of one space operation](#3-the-cost-of-one-space-operation)
- [4. Results of the operation benchmarks](#4-results-of-the-operation-benchmarks)
- [5. Results of the persistence benchmarks](#5-results-of-the-persistence-benchmarks)
- [6. Before and after](#6-before-and-after)
- [7. The examples from end to end](#7-the-examples-from-end-to-end)
- [8. Findings](#8-findings)
- [9. Reproducing the measurements](#9-reproducing-the-measurements)
- [References](#references)

## 1. Scope

The benchmarks measure the [native play](glossary.md#native-play) of [deploys](glossary.md#deploy) that use spaces, at the host-work limits of protocol version 6.
Every measured deploy runs on the test chain of `rholang/tests/support/native.rs`, as the tests and the examples harness do.
So a measured time includes the parse and the normalization of the deploy, its evaluation, its space transactions, its events and its metering.
A case of the persistence benchmarks also includes the [checkpoint](glossary.md#checkpoint) after the deploy.

The benchmarks do not measure what the campaign did not port.
The legacy hotspot items are the matrix arena, the copy-on-write checkpoint and the BTreeMap against the PathMap crate.
They target structures that the campaign replaced ([I-9](decision-register.md#i-9), [M-2](decision-register.md#m-2), [I-1](decision-register.md#i-1)).
The MeTTa groups are held ([decision 2](decision-register.md#decision-2)), and no benchmark calls the external ChromaDB service.

Figure 1 shows the path of one benchmark case, from its setup to its timed iterations.

![The path of one benchmark case as an activity diagram in three parts. The setup is not timed: a new test chain over an in-memory store, the setup deploys with the factory call of the space, and then fill deploys of ten sends, or of one send after ten did not fit, each followed by a checkpoint. A fill that cannot store one more send records that the collection is full at r records, and the case ends there. The reference play is checked and not timed: it evaluates the measured deploy once with a fresh v6 host-work budget and a phlo limit of ten to the ninth, reads the values on the result channel, the phlo and the host work, and takes the checkpoint. If the result, the phlo, the root and the host work differ from the contract, the run fails. The record line is appended, and a deploy that failed, for example with HostWorkRejected, is not timed. The timed iterations repeat: reset the chain to the pre-state root outside the timer, start the timer, parse and normalize the source text, reduce with each space operation as a space job, make metered view reads that reserve the key and the whole loaded value before the load, match, stage and commit with the events and the observer calls that charge the phlo, take the checkpoint only when the case includes it, and stop the timer. Criterion then reports the median of ten samples of equal iterations and its 95 percent bootstrap interval.](diagrams/56-benchmark-case-path.svg)

*Figure 1. The path of one benchmark case (`diagrams/56-benchmark-case-path.puml`).*

## 2. Method

### 2.1 Hardware and software

The measured runs of 2026-10-13 ran on this host and tree:

- CPU: AMD Ryzen Threadripper PRO 5975WX 32-Cores, a Zen 3 processor with 32 cores, 64 threads and a highest clock of 4,562 MHz.
- Cache: 4 core complexes (CCDs) of 8 cores, each with 32 MiB of L3 cache.
- Memory and system: 125.6 GiB of RAM, Linux 7.2.9.
- Pinned CPUs: 2,3, two cores of one CCD, under the `performance` governor of the `amd-pstate-epp` driver, with boost on.
- The load average at the start of the runs: 9.6 to 11.1 over one minute, from other sessions on other CCDs.
- Compiler: rustc 1.95.0-nightly (6efa357bf 2026-02-08), with the `bench` profile of the workspace.
- Tree: revision `70eede9e6` with the files of step S8.

The file `system.txt` of each run directory records these facts.
The profiles use AMD uProf, the profiler of this AMD processor, with frame-pointer call stacks (Section 3.5).

### 2.2 The test chain and the limits

A benchmark plays each deploy on the native test chain `NativeChain` of the test support.
The chain keeps its history trie in an in-memory store, and a tokio runtime with two worker threads runs its tasks.
Each deploy gets a fresh [host-work](glossary.md#host-work) budget of the limits of protocol version 6, as production gives each play.
Table 1 lists these limits.
In each of the 30 cases that fail with `HostWorkRejected`, VerificationBytes reaches its limit.
No other dimension of these cases uses more than 49 % of its limit.

*Table 1. The host-work limits of one deploy at protocol version 6.*

| Dimension | Limit of one deploy | What the reservations of a space count in it |
|---|---|---|
| VerificationBytes | 268,435,456 | The scanned bytes: each read, hash and copy |
| SearchStateBytes | 134,217,728 | The allocated bytes of the copies |
| VerificationOperations | 1,073,741,824 | The operations of the reservations |
| AuthorityDepth | 1,048,576 | The depth of the cost-authority chains |
| The 12 other dimensions | 268,435,456 each | Reductions, substitutions, primitive calls, searches, witnesses and structures |

Every benchmark deploy has a phlo limit of 1,000,000,000.
No measured deploy reaches it, so the host-work limits bound each deploy, the host program included.
The test suites use a phlo limit of 1,000,000, and a series of host sends reaches that limit at 1,503 sends.

### 2.3 One case

A benchmark case has a setup, a reference play and timed iterations, as Figure 1 shows.
The setup builds the pre-state of the case with setup deploys, and it is not timed.
A fill step stores records with deploys of ten sends, and with one send in each deploy after ten sends do not fit.
When one send no longer fits, the collection is full, and the case records where its fill stopped.

The reference play evaluates the measured deploy once.
It reads the values on the result channel, the phlo, the root after the checkpoint, and the host work of each dimension.
The run then checks these four results against the contract of Section 2.4.
Each timed iteration resets the chain to the pre-state outside the timer, and it times the deploy and, when the case says so, the checkpoint.

### 2.4 The contract

The file `test-vectors/reified-bench-contract-v1.json` holds the result, the phlo, the root and the host work of each case.
It holds 188 cases, and every run checks each case against it, a run in test mode included.
So a change of the result, the cost or the state of a case fails the benchmark before it measures.
A capture with `RRS_BENCH_CAPTURE` checks the cases that a capture file holds, and it adds the cases that the file does not hold yet.

### 2.5 The series

A one-deploy series measures one deploy with 1 operation, 10 operations, the largest count that fits the limits, and the next count.
The next count fails, and the series stops there.
A calibration found the largest count with a search that doubles its bound and then halves the interval.
The contract records the counts, and the check of the largest count and of the next count verifies the boundary in every run.

A read series measures one operation on a collection of 10, 100, 1,000 and 10,000 records.
It fills the collection in setup deploys, so a series stops when its collection is full.
The checkpoint series of Section 5.2 spreads its records over channels of 100 records, so that no collection becomes full.

### 2.6 Statistics and experiments

Criterion ([CRIT](#ref-crit)) takes ten samples of each case after one second of warm-up, in about five seconds of measurement.
Each sample has the same number of iterations (flat sampling), so the mean time of each sample is one replicate.
The tables report the median of the samples and its 95 % bootstrap interval ([Efr79](#ref-efr79)).
A comparison of two cases is a pgmcp experiment with its hypothesis, 60 replicates of each case, and Welch's t-test ([Wel47](#ref-wel47)).
Section 6.4 reports the experiments.

### 2.7 Profiles

A profiled run samples the bench process with AMD uProf ([UPROF](#ref-uprof)) on the pinned CPUs, with call stacks (Section 3.5).
The report of one bench runs on other CPUs while the next bench runs, so the analysis takes no time from the measurement.
The runs pin their CPUs with `taskset`, because the placement of a process can bias a measurement ([MDH09](#ref-mdh09)).

## 3. The cost of one space operation

### 3.1 One operation

Table 2 lists the phlo and the VerificationBytes of one send, and of one send-receive pair, for each family and for the host program.
The host program sends on ordinary channels, with no space.

*Table 2. The cost of one operation in a deploy of one operation.*

| Family | Phlo of one send | VerificationBytes of one send | Phlo of one send-receive pair | VerificationBytes of one pair |
|---|---|---|---|---|
| bag | 4,374 | 1,989,604 | 6,512 | 4,581,934 |
| set | 4,374 | 2,002,296 | 6,512 | 4,610,846 |
| queue | 7,878 | 4,074,773 | 12,917 | 8,611,752 |
| stack | 7,878 | 4,080,874 | 12,917 | 8,624,086 |
| priority queue | 7,936 | 4,170,901 | 12,976 | 8,773,523 |
| VectorDB | 4,453 | 2,860,916 | 6,755 | 8,482,995 |
| cell | 8,351 | 4,111,381 | not measured | not measured |
| PathMap bag | 9,245 | 5,024,750 | not measured | not measured |
| Array bag | 7,107 | 5,609,903 | not measured | not measured |
| Vector bag | 7,107 | 5,204,770 | not measured | not measured |
| HashSet bag | 7,491 | 3,501,259 | not measured | not measured |
| host program | 664 | 54,361 | 2,532 | 343,578 |

### 3.2 Operations in one deploy

The VerificationBytes bound how many space operations fit in one deploy.
Table 3 lists the most sends that one deploy holds for each family, and Table 4 lists the most send-receive pairs.
A deploy of the host program holds 3,224 sends, and a deploy of a bag space holds 59.
The HashSet bag holds 170 units, because each unit goes to its own channel.

*Table 3. The most sends that one deploy holds.*

| Family | Most sends in one deploy | VerificationBytes of that deploy, of the limit | Next count and its outcome |
|---|---|---|---|
| bag | 59 | 97.6 % | 60: HostWorkRejected |
| set | 59 | 97.9 % | 60: HostWorkRejected |
| queue | 46 | 98.2 % | 47: HostWorkRejected |
| stack | 46 | 98.4 % | 47: HostWorkRejected |
| priority queue | 40 | 97.5 % | 41: HostWorkRejected |
| VectorDB | 37 | 97.3 % | 38: HostWorkRejected |
| cell | 72 | 99.2 % | 73: HostWorkRejected |
| PathMap bag | 20 | 95.0 % | 21: HostWorkRejected |
| Array bag | 57 | 97.8 % | 58: HostWorkRejected |
| Vector bag | 59 | 98.9 % | 60: HostWorkRejected |
| HashSet bag | 170 | 99.9 % | 171: HostWorkRejected |
| host program | 3,224 | 100.0 % | 3,225: HostWorkRejected |

*Table 4. The most send-receive pairs that one deploy holds.*

| Family | Most send-receive pairs in one deploy | VerificationBytes of that deploy, of the limit | Next count and its outcome |
|---|---|---|---|
| bag | 30 | 95.5 % | 31: HostWorkRejected |
| set | 28 | 98.1 % | 29: HostWorkRejected |
| queue | 22 | 94.5 % | 23: HostWorkRejected |
| stack | 22 | 94.7 % | 23: HostWorkRejected |
| priority queue | 21 | 96.9 % | 22: HostWorkRejected |
| VectorDB | 17 | 94.5 % | 18: HostWorkRejected |
| host program | 682 | 99.8 % | 683: HostWorkRejected |

### 3.3 Host work grows with the collection

The VerificationBytes of one space operation grow in proportion to the records of its collection.
Tables 5 and 6 fit the least-squares line $`v(n) = a + b \cdot n`$ to the measured VerificationBytes $`v`$ of one operation on a collection of $`n`$ records.
Here $`a`$ is the cost of the operation on an empty collection, and $`b`$ is the cost of each stored record.
The 9 lines with three measured points have a coefficient of determination $`R^2`$ of at least 0.9999986.
A line through two points fits them exactly, so the tables give no $`R^2`$ for it.

*Table 5. The VerificationBytes of one receive from a collection of n records.*

| Collection | n = 10 | n = 100 | n = 1,000 | Intercept a | Slope b per record | R² | Full at records |
|---|---|---|---|---|---|---|---|
| bag | 5,290,131 | 23,866,747 | 212,535,867 | 3,064,787 | 209,458 | 0.9999986 | 2,667 |
| set | 6,053,253 | 32,385,448 | HostWorkRejected | 3,127,454 | 292,580 | two points | 2,667 |
| queue | 7,495,268 | 28,343,039 | 236,816,983 | 5,179,058 | 231,638 | 1.0000000 | 2,648 |
| stack | 7,501,336 | 28,349,173 | 236,823,051 | 5,185,159 | 231,638 | 1.0000000 | 2,648 |
| priority queue | 7,862,222 | 31,721,033 | HostWorkRejected | 5,211,243 | 265,098 | two points | 1,741 |
| VectorDB | 12,002,112 | 66,553,531 | HostWorkRejected | 5,940,843 | 606,127 | two points | 1,030 |
| host program | 360,523 | 360,523 | 360,523 | 360,523 | 0 | constant | not full at 10,000 |

*Table 6. The VerificationBytes of one send to a collection of n records.*

| Collection | n = 10 | n = 100 | n = 1,000 | Intercept a | Slope b per record | R² | Full at records |
|---|---|---|---|---|---|---|---|
| bag | 2,989,112 | 11,980,844 | 101,896,116 | 1,990,144 | 99,906 | 1.0000000 | 2,667 |
| set | 3,001,771 | 11,993,536 | 101,908,775 | 2,002,820 | 99,906 | 1.0000000 | 2,667 |
| queue | 4,911,995 | 13,903,727 | 103,819,065 | 3,913,024 | 99,906 | 1.0000000 | 2,648 |
| stack | 4,926,548 | 13,918,313 | 103,833,585 | 3,927,595 | 99,906 | 1.0000000 | 2,648 |
| priority queue | 5,528,807 | 19,203,533 | 155,949,471 | 4,009,466 | 151,940 | 1.0000000 | 1,741 |
| VectorDB | 5,442,815 | 28,666,123 | 260,907,395 | 2,861,992 | 258,045 | 1.0000000 | 1,030 |
| host program | 54,361 | 54,361 | 54,361 | 54,361 | 0 | constant | not full at 10,000 |

A receive from the bag reserves 212,535,867 VerificationBytes at 1,000 records.
A receive of the host program reserves 360,523 at every size of its channel.
A collection is full when one send no longer fits the limits.
The bag is full at 2,667 records, the queue at 2,648, the priority queue at 1,741 and VectorDB at 1,030.
A receive fails before that size, so the set, priority-queue and VectorDB reads of 1,000 records fail with `HostWorkRejected`.
Figure 2 draws the receive lines against the limit.

![A line chart of the VerificationBytes of one receive, in millions, against the records of its collection, from 0 to 1,400 records. A horizontal line marks the limit of 268.4 million VerificationBytes. Each discipline has the least-squares line of Table 5, drawn from 0 records to the size where it reaches the limit: VectorDB at 433 records, the set at 907 records, the priority queue at 993 records, the queue and stack at 1,136 records, and the bag at 1,267 records. Black dots mark the measured receives of 10, 100 and 1,000 records that fit the limit, and they lie on the lines. The host program reserves 0.36 million at every size, so the chart does not draw it.](diagrams/57-receive-host-work.svg)

*Figure 2. The VerificationBytes of one receive against the records of its collection (`diagrams/57-receive-host-work.puml`).*

### 3.4 The cause

A space transaction reads its [staged view](glossary.md#staged-view) through [metered view reads](glossary.md#metered-view-read).
The first read of a channel in a transaction loads every record of the channel into the view (`TxnView::load_data`, `rspace++/src/rspace/space_store_handle.rs:1706-1721`).
The read reserves the copy of the whole list of records, and then it copies the list (`NativeViewReads::data`, `rspace++/src/rspace/space_store_handle.rs:1018-1031`).
A send, a receive and a fire each read the list of their channel (`TxnCore::read_data` at `rspace++/src/rspace/space_store_handle.rs:2222-2228` and `TxnCore::load_group` at `rspace++/src/rspace/space_store_handle.rs:2349-2358`).
So each operation reserves work in proportion to the records of its collection, because [decision 40](decision-register.md#decision-40) reserves every read and every copy.

Native play of the host program copies the data of its channel too (`fetch_channel_to_index_data`, `rspace++/src/rspace/rspace/ops_consume.rs:165-177`).
It reserves no host work for that copy, so its VerificationBytes come from the observer calls only, and they do not grow with the channel.
Its time grows: a host receive takes 0.45 ms at 10 records and 5.79 ms at 1,000 records.
A bag receive takes 1.72 ms and 25.74 ms at the same sizes.
So the copy costs time on both paths, and only the space path reserves it as host work.

### 3.5 Where the time goes

Three profiled runs show where the time of a deploy goes.
Each run samples the bench process with AMD uProf ([UPROF](#ref-uprof)), the profiler of the AMD processor of the host.
It uses the hotspots configuration, which samples the CPU time with call stacks, while Criterion runs the case for ten seconds without its analysis.

The DWARF unwinders of uProf and of `perf` give no call stacks on these binaries.
So the profiled build keeps frame pointers, in a target directory of its own.
That build has the flags of the measured build and `-C force-frame-pointers=yes`.
A profile covers the whole bench process, so the profile of a read series also holds the fill deploys of its setup.

Each table gives the share of the CPU-time samples in a function with its callees, and in the function alone.

*Table 7. Where the time goes: one receive from a bag of 1,000 records, with the setup that fills the bag (`receive_from_n/bag/1000`).*

| Frame | With callees | Alone |
|---|---|---|
| Every space job (`ReifiedRuntime::run_op`) | 67.4 % | 0.0 % |
| The metered view reads (`NativeViewReads`) | 62.7 % | 0.0 % |
| The loads of the staged view (`TxnView`) | 54.4 % | 0.0 % |
| The decode of records from the history (`NativeHistoryReader::with_records`) | 28.5 % | 0.0 % |
| The walk that meters each copy (`clone_backing::Walker::drain`) | 27.8 % | 0.9 % |
| The sends of the fill deploys (`FamilyAgent::prepare_produce`) | 32.6 % | 0.0 % |
| The measured receives (`FamilyAgent::prepare_consume`) | 23.8 % | 0.0 % |

*Table 8. Where the time goes: one deploy of 10 sends to a bag space (`send/bag/10`).*

| Frame | With callees | Alone |
|---|---|---|
| Every space job (`submit_space`) | 55.3 % | 0.0 % |
| The sends (`FamilyAgent::prepare_produce`) | 45.6 % | 0.0 % |
| The start of a space transaction (`begin_collection`) | 24.5 % | 0.0 % |
| The read of the config record (`read_space_config`) | 17.1 % | 0.0 % |
| The metered decode of the config (`SpaceConfig::from_par_metered`) | 11.9 % | 0.0 % |
| The metered view reads (`NativeViewReads`) | 10.6 % | 0.0 % |
| The walk that meters each copy (`clone_backing::Walker::drain`) | 20.8 % | 1.1 % |
| The evaluation of the reducer (`generated_message_eval`) | 15.3 % | 0.2 % |

*Table 9. Where the time goes: one deploy of 10 sends of the host program (`send/host/10`).*

| Frame | With callees | Alone |
|---|---|---|
| The host operations with their charges (`ChargingRSpace`) | 30.1 % | 0.0 % |
| The host RSpace (`RSpace`) | 30.1 % | 0.0 % |
| The evaluation of the reducer (`generated_message_eval`) | 32.2 % | 0.2 % |
| The deterministic scheduler (`ReductionSession::drive`) | 17.8 % | 0.1 % |

In a collection of 1,000 records, the metered view reads take most of the time.
They decode the record list from the history, copy it into the staged view, and walk the copy to meter it.
In a small collection, the read and the metered decode of the config record of the space are a large share of each send.
Section 8 names a candidate optimization for each of the two costs.

## 4. Results of the operation benchmarks

Each table of this section lists the cases of one group of `rholang/benches/reified_spaces.rs`.
The outcome is "fits" when the deploy succeeds, and otherwise it is the error of the deploy or the size where the fill stopped.
A case that does not fit is recorded and not timed.
The phlo is that of the native contract with the weights of the test chain.

### 4.1 The factory

*Table 10. One factory call that creates a space.*

| Case | Outcome | Phlo | VerificationBytes | Median time in ms (95 % interval) |
|---|---|---|---|---|
| `factory/hashmap_bag` | fits | 7,144 | 2,039,308 | 1.05 (1.05 to 1.05) |
| `factory/hashmap_vectordb` | fits | 7,352 | 2,532,744 | 1.37 (1.35 to 1.38) |

### 4.2 Sends

The send cases store records in one collection of each family, and in one channel of the host program.
The cell series sends to n channels, because a cell holds one record.

*Table 11. Sends in one deploy.*

| Case | Outcome | Phlo | VerificationBytes | Median time in ms (95 % interval) |
|---|---|---|---|---|
| `send/array_bag/1` | fits | 7,107 | 5,609,903 | 1.47 (1.46 to 1.47) |
| `send/array_bag/10` | fits | 15,276 | 25,728,154 | 4.48 (4.46 to 4.50) |
| `send/array_bag/57` | fits | 57,905 | 262,618,219 | 28.20 (27.96 to 28.28) |
| `send/array_bag/58` | HostWorkRejected | none | 268,434,638 | not timed |
| `send/bag/1` | fits | 4,374 | 1,989,604 | 0.98 (0.97 to 0.98) |
| `send/bag/10` | fits | 12,082 | 20,243,769 | 3.70 (3.67 to 3.71) |
| `send/bag/59` | fits | 54,026 | 261,934,176 | 27.10 (27.02 to 27.24) |
| `send/bag/60` | HostWorkRejected | none | 268,434,630 | not timed |
| `send/cell/1` | fits | 8,351 | 4,111,381 | 1.41 (1.41 to 1.41) |
| `send/cell/10` | fits | 51,852 | 37,010,815 | 7.32 (7.28 to 7.38) |
| `send/cell/72` | fits | 351,555 | 266,338,920 | 50.93 (50.68 to 51.48) |
| `send/cell/73` | HostWorkRejected | none | 268,435,357 | not timed |
| `send/hashset_bag/1` | fits | 7,491 | 3,501,259 | 1.47 (1.46 to 1.47) |
| `send/hashset_bag/10` | fits | 17,278 | 17,338,716 | 4.71 (4.70 to 4.72) |
| `send/hashset_bag/170` | fits | 191,304 | 268,236,963 | 70.17 (69.72 to 71.09) |
| `send/hashset_bag/171` | HostWorkRejected | none | 268,433,508 | not timed |
| `send/host/1` | fits | 664 | 54,361 | 0.18 (0.17 to 0.18) |
| `send/host/10` | fits | 6,640 | 633,303 | 0.64 (0.64 to 0.64) |
| `send/host/3224` | fits | 2,143,896 | 268,434,862 | 2,587.01 (2,556.19 to 2,608.77) |
| `send/host/3225` | HostWorkRejected | none | 268,434,757 | not timed |
| `send/pathmap_bag/1` | fits | 9,245 | 5,024,750 | 1.61 (1.60 to 1.62) |
| `send/pathmap_bag/10` | fits | 60,792 | 84,866,740 | 10.93 (10.83 to 11.24) |
| `send/pathmap_bag/20` | fits | 118,062 | 255,040,777 | 25.63 (25.38 to 25.90) |
| `send/pathmap_bag/21` | HostWorkRejected | none | 268,434,448 | not timed |
| `send/priorityqueue/1` | fits | 7,936 | 4,170,901 | 1.49 (1.48 to 1.50) |
| `send/priorityqueue/10` | fits | 42,265 | 42,951,136 | 7.60 (7.58 to 7.64) |
| `send/priorityqueue/40` | fits | 156,685 | 261,616,173 | 32.96 (32.80 to 33.20) |
| `send/priorityqueue/41` | HostWorkRejected | none | 268,435,064 | not timed |
| `send/queue/1` | fits | 7,878 | 4,074,773 | 1.34 (1.34 to 1.35) |
| `send/queue/10` | fits | 41,695 | 39,629,144 | 6.04 (6.02 to 6.08) |
| `send/queue/46` | fits | 176,947 | 263,476,819 | 30.34 (30.04 to 30.90) |
| `send/queue/47` | HostWorkRejected | none | 268,425,221 | not timed |
| `send/set/1` | fits | 4,374 | 2,002,296 | 0.98 (0.98 to 0.99) |
| `send/set/10` | fits | 12,082 | 20,370,227 | 3.76 (3.75 to 3.78) |
| `send/set/59` | fits | 54,026 | 262,681,552 | 27.79 (27.61 to 28.11) |
| `send/set/60` | HostWorkRejected | none | 268,435,138 | not timed |
| `send/stack/1` | fits | 7,878 | 4,080,874 | 1.35 (1.34 to 1.35) |
| `send/stack/10` | fits | 41,695 | 39,766,354 | 6.38 (6.36 to 6.41) |
| `send/stack/46` | fits | 176,947 | 264,138,498 | 32.43 (32.21 to 32.71) |
| `send/stack/47` | HostWorkRejected | none | 268,435,081 | not timed |
| `send/vector_bag/1` | fits | 7,107 | 5,204,770 | 1.44 (1.42 to 1.45) |
| `send/vector_bag/10` | fits | 15,276 | 23,499,972 | 4.27 (4.27 to 4.29) |
| `send/vector_bag/59` | fits | 59,719 | 265,464,110 | 28.01 (27.83 to 28.17) |
| `send/vector_bag/60` | HostWorkRejected | none | 268,433,882 | not timed |
| `send/vectordb/1` | fits | 4,453 | 2,860,916 | 1.15 (1.13 to 1.15) |
| `send/vectordb/10` | fits | 12,872 | 36,073,239 | 5.48 (5.45 to 5.51) |
| `send/vectordb/37` | fits | 38,117 | 261,310,401 | 25.16 (25.07 to 25.50) |
| `send/vectordb/38` | HostWorkRejected | none | 268,435,397 | not timed |

### 4.3 Send-receive pairs

*Table 12. Send-receive pairs in one deploy.*

| Case | Outcome | Phlo | VerificationBytes | Median time in ms (95 % interval) |
|---|---|---|---|---|
| `send_receive/bag/1` | fits | 6,512 | 4,581,934 | 1.43 (1.42 to 1.44) |
| `send_receive/bag/10` | fits | 33,462 | 55,199,751 | 8.50 (8.46 to 8.54) |
| `send_receive/bag/30` | fits | 93,342 | 256,312,748 | 30.64 (30.57 to 30.81) |
| `send_receive/bag/31` | HostWorkRejected | none | 268,433,946 | not timed |
| `send_receive/host/1` | fits | 2,532 | 343,578 | 0.37 (0.37 to 0.38) |
| `send_receive/host/10` | fits | 25,320 | 3,574,437 | 2.17 (2.15 to 2.18) |
| `send_receive/host/682` | fits | 1,728,678 | 267,898,911 | 1,479.57 (1,472.20 to 1,484.57) |
| `send_receive/host/683` | HostWorkRejected | none | 268,435,148 | not timed |
| `send_receive/priorityqueue/1` | fits | 12,976 | 8,773,523 | 2.22 (2.21 to 2.22) |
| `send_receive/priorityqueue/10` | fits | 92,655 | 101,110,763 | 15.47 (15.38 to 15.57) |
| `send_receive/priorityqueue/21` | fits | 190,038 | 260,076,714 | 35.03 (34.84 to 35.26) |
| `send_receive/priorityqueue/22` | HostWorkRejected | none | 268,435,436 | not timed |
| `send_receive/queue/1` | fits | 12,917 | 8,611,752 | 2.08 (2.06 to 2.11) |
| `send_receive/queue/10` | fits | 92,085 | 95,625,138 | 13.54 (13.46 to 13.68) |
| `send_receive/queue/22` | fits | 197,637 | 253,689,042 | 31.85 (31.70 to 32.04) |
| `send_receive/queue/23` | HostWorkRejected | none | 268,435,248 | not timed |
| `send_receive/set/1` | fits | 6,512 | 4,610,846 | 1.45 (1.44 to 1.46) |
| `send_receive/set/10` | fits | 33,462 | 59,214,996 | 9.13 (9.10 to 9.30) |
| `send_receive/set/28` | fits | 87,354 | 263,213,842 | 32.28 (32.21 to 32.37) |
| `send_receive/set/29` | HostWorkRejected | none | 268,434,647 | not timed |
| `send_receive/stack/1` | fits | 12,917 | 8,624,086 | 2.09 (2.08 to 2.11) |
| `send_receive/stack/10` | fits | 92,085 | 95,823,688 | 13.95 (13.93 to 14.08) |
| `send_receive/stack/22` | fits | 197,637 | 254,136,232 | 32.94 (32.72 to 33.07) |
| `send_receive/stack/23` | HostWorkRejected | none | 268,435,423 | not timed |
| `send_receive/vectordb/1` | fits | 6,755 | 8,482,995 | 2.08 (2.07 to 2.10) |
| `send_receive/vectordb/10` | fits | 35,852 | 119,171,080 | 16.13 (16.09 to 16.27) |
| `send_receive/vectordb/17` | fits | 58,483 | 253,569,548 | 31.08 (30.74 to 31.31) |
| `send_receive/vectordb/18` | HostWorkRejected | none | 268,434,411 | not timed |

### 4.4 A receive from n records

*Table 13. One receive from a collection of n records.*

| Case | Outcome | Phlo | VerificationBytes | Median time in ms (95 % interval) |
|---|---|---|---|---|
| `receive_from_n/bag/10` | fits | 8,379 | 5,290,131 | 1.72 (1.71 to 1.72) |
| `receive_from_n/bag/100` | fits | 8,385 | 23,866,747 | 3.62 (3.61 to 3.70) |
| `receive_from_n/bag/1000` | fits | 8,385 | 212,535,867 | 25.74 (25.67 to 26.06) |
| `receive_from_n/bag/10000` | the collection is full at 2667 records | none | none | not timed |
| `receive_from_n/host/10` | fits | 2,585 | 360,523 | 0.45 (0.44 to 0.45) |
| `receive_from_n/host/100` | fits | 2,588 | 360,523 | 0.92 (0.92 to 0.92) |
| `receive_from_n/host/1000` | fits | 2,588 | 360,523 | 5.79 (5.75 to 5.88) |
| `receive_from_n/host/10000` | fits | 2,588 | 360,523 | 72.08 (71.93 to 72.92) |
| `receive_from_n/priorityqueue/10` | fits | 11,280 | 7,862,222 | 2.06 (2.06 to 2.07) |
| `receive_from_n/priorityqueue/100` | fits | 11,285 | 31,721,033 | 4.28 (4.20 to 4.45) |
| `receive_from_n/priorityqueue/1000` | HostWorkRejected | none | 268,435,420 | not timed |
| `receive_from_n/priorityqueue/10000` | the collection is full at 1741 records | none | none | not timed |
| `receive_from_n/queue/10` | fits | 11,280 | 7,495,268 | 2.05 (2.04 to 2.07) |
| `receive_from_n/queue/100` | fits | 11,285 | 28,343,039 | 4.11 (4.06 to 4.25) |
| `receive_from_n/queue/1000` | fits | 11,285 | 236,816,983 | 27.12 (26.81 to 27.95) |
| `receive_from_n/queue/10000` | the collection is full at 2648 records | none | none | not timed |
| `receive_from_n/set/10` | fits | 8,379 | 6,053,253 | 1.84 (1.83 to 1.85) |
| `receive_from_n/set/100` | fits | 8,379 | 32,385,448 | 4.62 (4.52 to 4.73) |
| `receive_from_n/set/1000` | HostWorkRejected | none | 268,434,027 | not timed |
| `receive_from_n/set/10000` | the collection is full at 2667 records | none | none | not timed |
| `receive_from_n/stack/10` | fits | 11,280 | 7,501,336 | 2.04 (2.03 to 2.04) |
| `receive_from_n/stack/100` | fits | 11,291 | 28,349,173 | 4.10 (4.07 to 4.16) |
| `receive_from_n/stack/1000` | fits | 11,291 | 236,823,051 | 27.48 (27.19 to 27.95) |
| `receive_from_n/stack/10000` | the collection is full at 2648 records | none | none | not timed |
| `receive_from_n/vectordb/10` | fits | 8,638 | 12,002,112 | 2.51 (2.49 to 2.58) |
| `receive_from_n/vectordb/100` | fits | 8,638 | 66,553,531 | 9.29 (9.19 to 9.60) |
| `receive_from_n/vectordb/1000` | HostWorkRejected | none | 268,435,069 | not timed |
| `receive_from_n/vectordb/10000` | the collection is full at 1030 records | none | none | not timed |

### 4.5 A send to n records

*Table 14. One send to a collection of n records.*

| Case | Outcome | Phlo | VerificationBytes | Median time in ms (95 % interval) |
|---|---|---|---|---|
| `send_to_n/bag/10` | fits | 4,374 | 2,989,112 | 1.15 (1.14 to 1.15) |
| `send_to_n/bag/100` | fits | 4,376 | 11,980,844 | 2.56 (2.55 to 2.57) |
| `send_to_n/bag/1000` | fits | 4,376 | 101,896,116 | 17.44 (17.37 to 17.48) |
| `send_to_n/bag/10000` | the collection is full at 2667 records | none | none | not timed |
| `send_to_n/host/10` | fits | 664 | 54,361 | 0.22 (0.22 to 0.22) |
| `send_to_n/host/100` | fits | 665 | 54,361 | 0.56 (0.56 to 0.56) |
| `send_to_n/host/1000` | fits | 665 | 54,361 | 3.93 (3.90 to 3.94) |
| `send_to_n/host/10000` | fits | 666 | 54,361 | 48.35 (47.87 to 49.05) |
| `send_to_n/priorityqueue/10` | fits | 7,333 | 5,528,807 | 1.64 (1.64 to 1.65) |
| `send_to_n/priorityqueue/100` | fits | 7,340 | 19,203,533 | 3.18 (3.16 to 3.31) |
| `send_to_n/priorityqueue/1000` | fits | 7,340 | 155,949,471 | 19.67 (19.48 to 20.05) |
| `send_to_n/priorityqueue/10000` | the collection is full at 1741 records | none | none | not timed |
| `send_to_n/queue/10` | fits | 7,275 | 4,911,995 | 1.42 (1.42 to 1.42) |
| `send_to_n/queue/100` | fits | 7,282 | 13,903,727 | 2.80 (2.79 to 2.81) |
| `send_to_n/queue/1000` | fits | 7,282 | 103,819,065 | 17.44 (17.27 to 17.69) |
| `send_to_n/queue/10000` | the collection is full at 2648 records | none | none | not timed |
| `send_to_n/set/10` | fits | 4,374 | 3,001,771 | 1.15 (1.15 to 1.16) |
| `send_to_n/set/100` | fits | 4,376 | 11,993,536 | 2.59 (2.59 to 2.61) |
| `send_to_n/set/1000` | fits | 4,376 | 101,908,775 | 17.38 (17.35 to 17.59) |
| `send_to_n/set/10000` | the collection is full at 2667 records | none | none | not timed |
| `send_to_n/stack/10` | fits | 7,275 | 4,926,548 | 1.45 (1.44 to 1.47) |
| `send_to_n/stack/100` | fits | 7,282 | 13,918,313 | 2.84 (2.82 to 2.84) |
| `send_to_n/stack/1000` | fits | 7,282 | 103,833,585 | 17.75 (17.68 to 17.80) |
| `send_to_n/stack/10000` | the collection is full at 2648 records | none | none | not timed |
| `send_to_n/vectordb/10` | fits | 4,453 | 5,442,815 | 1.44 (1.43 to 1.44) |
| `send_to_n/vectordb/100` | fits | 4,453 | 28,666,123 | 4.02 (3.99 to 4.07) |
| `send_to_n/vectordb/1000` | fits | 4,453 | 260,907,395 | 34.27 (34.01 to 34.69) |
| `send_to_n/vectordb/10000` | the collection is full at 1030 records | none | none | not timed |

### 4.6 Matching

The `bag_scan_miss` case uses a pattern that no record matches, so the bag inspects every record, and the receive then waits.
The `bag_first` case matches the first record of the canonical order, and the `queue_head` case inspects only the head.
The `join_two` case joins two channels of a bag that each hold n records, so the receive reserves one record on each channel.
The `send_fires_waiter` case parks n receives on one channel, and the measured send fires one of them: the produce side of a COMM.
In this series, the size where the fill stopped counts the parked receives.

*Table 15. Matching over a collection of n records.*

| Case | Outcome | Phlo | VerificationBytes | Median time in ms (95 % interval) |
|---|---|---|---|---|
| `matcher/bag_first/10` | fits | 8,379 | 5,289,635 | 1.76 (1.75 to 1.76) |
| `matcher/bag_first/100` | fits | 8,385 | 23,866,251 | 3.76 (3.69 to 3.80) |
| `matcher/bag_first/1000` | fits | 8,385 | 212,533,883 | 25.88 (25.76 to 26.83) |
| `matcher/bag_first/10000` | the collection is full at 2667 records | none | none | not timed |
| `matcher/bag_scan_miss/10` | fits | 4,571 | 4,330,802 | 1.37 (1.37 to 1.38) |
| `matcher/bag_scan_miss/100` | fits | 4,571 | 24,533,474 | 3.65 (3.60 to 3.70) |
| `matcher/bag_scan_miss/1000` | fits | 4,571 | 229,487,985 | 27.03 (26.80 to 27.90) |
| `matcher/bag_scan_miss/10000` | the collection is full at 2667 records | none | none | not timed |
| `matcher/join_two/10` | fits | 11,149 | 8,621,329 | 2.30 (2.27 to 2.38) |
| `matcher/join_two/100` | fits | 11,149 | 45,761,539 | 6.01 (5.95 to 6.08) |
| `matcher/join_two/1000` | HostWorkRejected | none | 268,434,908 | not timed |
| `matcher/join_two/10000` | the collection is full at 5334 records | none | none | not timed |
| `matcher/queue_head/10` | fits | 11,280 | 7,495,202 | 2.09 (2.05 to 2.12) |
| `matcher/queue_head/100` | fits | 11,285 | 28,343,006 | 4.09 (4.05 to 4.20) |
| `matcher/queue_head/1000` | fits | 11,285 | 236,816,983 | 27.13 (26.94 to 27.52) |
| `matcher/queue_head/10000` | the collection is full at 2648 records | none | none | not timed |
| `matcher/send_fires_waiter/10` | fits | 5,564 | 6,097,880 | 1.53 (1.52 to 1.54) |
| `matcher/send_fires_waiter/100` | fits | 5,564 | 35,864,408 | 4.70 (4.68 to 4.88) |
| `matcher/send_fires_waiter/1000` | HostWorkRejected | none | 268,435,422 | not timed |
| `matcher/send_fires_waiter/10000` | the collection is full at 1726 records | none | none | not timed |

### 4.7 VectorDB queries

The queries select by a threshold, the best record, the best eight records, or every record of the collection.

*Table 16. VectorDB queries over a collection of n records.*

| Case | Outcome | Phlo | VerificationBytes | Median time in ms (95 % interval) |
|---|---|---|---|---|
| `vectordb_query/best/10` | fits | 8,638 | 12,003,441 | 2.50 (2.50 to 2.53) |
| `vectordb_query/best/100` | fits | 8,638 | 66,562,892 | 8.87 (8.73 to 9.09) |
| `vectordb_query/best/1000` | HostWorkRejected | none | 268,435,102 | not timed |
| `vectordb_query/best/10000` | the collection is full at 1030 records | none | none | not timed |
| `vectordb_query/k8/10` | fits | 50,839 | 29,431,200 | 4.47 (4.44 to 4.58) |
| `vectordb_query/k8/100` | fits | 50,839 | 83,992,649 | 11.11 (10.95 to 11.44) |
| `vectordb_query/k8/1000` | HostWorkRejected | none | 268,435,246 | not timed |
| `vectordb_query/k8/10000` | the collection is full at 1030 records | none | none | not timed |
| `vectordb_query/list_all/10` | fits | 74,730 | 32,811,622 | 4.61 (4.58 to 4.70) |
| `vectordb_query/list_all/100` | HostWorkRejected | none | 268,435,311 | not timed |
| `vectordb_query/list_all/1000` | HostWorkRejected | none | 268,435,238 | not timed |
| `vectordb_query/list_all/10000` | the collection is full at 1030 records | none | none | not timed |
| `vectordb_query/threshold/10` | fits | 8,700 | 12,266,850 | 2.70 (2.66 to 2.71) |
| `vectordb_query/threshold/100` | fits | 8,700 | 66,841,146 | 9.38 (9.25 to 9.58) |
| `vectordb_query/threshold/1000` | HostWorkRejected | none | 268,435,453 | not timed |
| `vectordb_query/threshold/10000` | the collection is full at 1030 records | none | none | not timed |

### 4.8 Concurrent operations

Each case runs eight operations in parallel in one deploy.
They use one channel, eight channels of one space, four send-receive pairs on one channel, or eight spaces.

*Table 17. Eight parallel operations in one deploy.*

| Case | Outcome | Phlo | VerificationBytes | Median time in ms (95 % interval) |
|---|---|---|---|---|
| `concurrency/many_channels/8` | fits | 12,314 | 12,908,742 | 3.42 (3.40 to 3.43) |
| `concurrency/many_spaces/8` | fits | 29,288 | 15,962,322 | 5.39 (5.37 to 5.43) |
| `concurrency/mixed_one_channel/8` | fits | 15,498 | 18,718,661 | 3.62 (3.61 to 3.66) |
| `concurrency/one_channel/8` | fits | 10,369 | 15,492,909 | 3.09 (3.08 to 3.11) |

### 4.9 Queue order

Each deploy sends n records to one queue and drains them with one persistent receive, one record in each round.

*Table 18. Sends and a FIFO drain in one deploy.*

| Case | Outcome | Phlo | VerificationBytes | Median time in ms (95 % interval) |
|---|---|---|---|---|
| `queue_ordering/send_and_drain/1` | fits | 12,919 | 8,908,585 | 2.18 (2.17 to 2.20) |
| `queue_ordering/send_and_drain/10` | fits | 57,446 | 62,257,327 | 8.76 (8.71 to 9.00) |
| `queue_ordering/send_and_drain/31` | fits | 161,333 | 259,997,336 | 28.92 (28.70 to 29.11) |
| `queue_ordering/send_and_drain/32` | HostWorkRejected | none | 268,435,273 | not timed |

## 5. Results of the persistence benchmarks

Each case of `rholang/benches/reified_persistence.rs` measures work that a block adds to its evaluation.
The cases of Sections 5.1 and 5.2 time the deploy together with its checkpoint, and the other cases time the work that their sections name.
The cases of Sections 5.1 and 5.2 use the in-memory store of the test chain.
The cases of Sections 5.3 to 5.6 use LMDB stores.

### 5.1 Commit

Each case sends 10 records to one collection, and it times the deploy with its checkpoint.

*Table 19. One deploy of 10 sends with its checkpoint.*

| Case | Outcome | Phlo | VerificationBytes | Median time in ms (95 % interval) |
|---|---|---|---|---|
| `commit/bag/10` | fits | 14,512 | 20,500,537 | 5.28 (5.25 to 5.30) |
| `commit/priorityqueue/10` | fits | 49,065 | 43,342,685 | 9.42 (9.38 to 9.46) |
| `commit/queue/10` | fits | 48,495 | 40,002,917 | 7.87 (7.81 to 8.03) |
| `commit/set/10` | fits | 14,512 | 20,627,061 | 5.38 (5.34 to 5.40) |
| `commit/stack/10` | fits | 48,495 | 40,164,112 | 8.48 (8.41 to 8.60) |
| `commit/vectordb/10` | fits | 15,302 | 36,337,039 | 7.06 (7.04 to 7.09) |

### 5.2 The checkpoint against the size of the space

Each case sends one record to a new channel of a space that already holds S records, and it times the deploy with its checkpoint.
The S records lie on channels of 100 records, and the fill of each channel is its own setup step.
The measured send goes to a new channel, so the records of the other channels change only the size of the history trie.

### 5.3 Restart and attach

The `bag/100` case receives from a bag of 100 records on an LMDB store after a reset to the root.
The runtime keeps its caches through the reset.
The `bag/100/cold` case closes the store and opens it again at that root with a new runtime.
It then times the first receive, as a node does after a restart.
The open and the close are not timed.

*Table 20. The first receive after a reset and after a restart.*

| Case | Outcome | Phlo | VerificationBytes | Median time in ms (95 % interval) |
|---|---|---|---|---|
| `restart_attach/bag/100` | fits | 8,379 | 23,862,746 | 3.89 (3.87 to 3.95) |
| `restart_attach/bag/100/cold` | fits | as above | as above | 3.99 (3.95 to 4.04) |

The contract checks the cached case only. The cold case plays the same deploy on the same root, so its result, phlo and root are those of the cached case.

### 5.4 Merge

Two branches over one base each store 10 records in one bag space on an LMDB store.
The case times the merge of both branches.
A bag has no counter, so the merger accepts both branches, and the contract holds the merged root.

*Table 21. The merge of two branches that both write one bag.*

| Case | Merged root | Median time in ms (95 % interval) |
|---|---|---|
| `merge/bag_two_branches/10` | `f74cb026c832dcaf…` | 0.83 (0.83 to 0.83) |

### 5.5 Replay

The case plays two blocks on an LMDB store: the creation of a queue space, and 10 sends to it.
It then times the native replay of both blocks, which certifies each play against its journal, as a validator does.

*Table 22. The native replay of two blocks.*

| Case | Phlo of block 2 | Median time in ms (95 % interval) |
|---|---|---|
| `replay/queue_two_blocks/10` | 41,695 | 16.12 (16.06 to 16.29) |

### 5.6 The bytes of a block on LMDB

The `lmdb_commit` case opens an LMDB store, creates a bag space, and plays 10 blocks of 10 sends with a checkpoint after each block.
It records the length and the allocated bytes of each data file after each block.
It then times one block of 10 sends with its checkpoint.

The store has two LMDB environments: the history trie and the cold store of the record lists.
Table 23 sums the data files of both, and it gives the bytes that each environment adds.

*Table 23. The bytes that each block of 10 sends adds to the LMDB store.*

| Block | Length of the data files | Added to the length | Added to the cold store | Added to the history |
|---|---|---|---|---|
| 1 | 118,784 | 40,960 | 40,960 | 0 |
| 2 | 163,840 | 45,056 | 45,056 | 0 |
| 3 | 225,280 | 61,440 | 61,440 | 0 |
| 4 | 299,008 | 73,728 | 65,536 | 8,192 |
| 5 | 380,928 | 81,920 | 73,728 | 8,192 |
| 6 | 475,136 | 94,208 | 86,016 | 8,192 |
| 7 | 585,728 | 110,592 | 106,496 | 4,096 |
| 8 | 696,320 | 110,592 | 110,592 | 0 |
| 9 | 827,392 | 131,072 | 126,976 | 4,096 |
| 10 | 966,656 | 139,264 | 139,264 | 0 |

*Table 24. One block of 10 sends with its checkpoint on LMDB.*

| Case | Outcome | Phlo | VerificationBytes | Median time in ms (95 % interval) |
|---|---|---|---|---|
| `lmdb_commit/bag/10` | fits | 12,082 | 20,243,703 | 5.03 (5.01 to 5.05) |

## 6. Before and after

### 6.1 The legacy benchmark

The legacy branch benchmarked its spaces with `rholang/benches/spaces_benchmark.rs` at revision `ff479042`.
Its 92 cases time in-memory structures alone.
They are the collections of the legacy spaces, a test space with the wildcard matcher, a shared map and atomic counters.
No legacy case plays a deploy, writes an event, meters work or writes the history trie.
The restored archive `test-vectors/legacy-spaces-benchmark-ff479042.tar.gz` holds its raw samples, and `scripts/bench/check-p0-archives.py` checks the digests of the restored archives.

### 6.2 The comparable cases

Table 25 gives each legacy case for one element, and each counterpart for one operation of a deploy.
The time of one more operation is the difference of the medians of the deploys of 1 and of 10 operations, divided by 9.
The ratio is large in every row, because each operation now runs the deploy machinery that the legacy cases did not have.
The last column explains each ratio.

*Table 25. The legacy cases and their counterparts.*

| Legacy case | Before: ns for one element | Counterpart now | After: ms for one operation | After over before | Why the time differs |
|---|---|---|---|---|---|
| `collection_types/bag_insert_1000` | 1.2 | one more send of a deploy to a bag space | 0.302 | 243,974 | The legacy case pushes an integer into an in-memory vector. A send now runs a space job: routing, locks, a metered copy of the list, events and the observer calls. |
| `collection_types/queue_insert_1000` | 1.3 | one more send of a deploy to a queue space | 0.522 | 394,905 | As for the bag, and each send also bumps the counter of the collection with three events (M-1). |
| `collection_types/stack_insert_1000` | 1.3 | one more send of a deploy to a stack space | 0.559 | 433,603 | As for the queue. |
| `collection_types/set_insert_1000` | 55.2 | one more send of a deploy to a set space | 0.309 | 5,596 | The legacy case inserts into a hash set. A send now stores a record, and a receive later retires its class. |
| `produce_consume_cycle/produce_only/1000` | 19.6 | one more send of a deploy to a bag space | 0.302 | 15,432 | The legacy space had no events, no metering and no history. A send now makes all three. |
| `produce_consume_cycle/consume_immediate_match/1000` | 76.6 | one more send-receive pair of a deploy on a bag space | 0.785 | 10,252 | The legacy consume matched with the wildcard matcher. A receive now matches its pattern, fires a COMM and commits it. |
| `produce_consume_cycle/produce_multi_channel/1000` | 91.3 | one more send of a deploy, each to its own cell | 0.656 | 7,190 | Each send now reads its own collection and bumps its own counter. |
| `collection_retrieval/bag_find_all` | 184.6 | one receive from a bag space of 1,000 records | 25.745 | 139,427 | The legacy case removes 1,000 items from an in-memory vector, so one removal scans part of it. A receive now copies the whole list of 1,000 records into its staged view first (Section 3.4). |
| `collection_retrieval/queue_find_all` | 0.4 | one receive from a queue space of 1,000 records | 27.122 | 73,091,165 | The legacy queue removed its head in place. A receive now copies the whole list, although it inspects only the head. |
| `collection_retrieval/stack_find_all` | 1.3 | one receive from a stack space of 1,000 records | 27.480 | 20,442,133 | As for the queue. |
| `matcher_comparison/exact_match/1000` | 0.4 | one record of a receive that scans 1,000 records and matches none | 0.027 | 63,154 | The legacy case compares two integers. The receive now matches a Rholang pattern against each record with the metered matcher, inside a deploy. |
| `vectordb_top_k/top_10/1000` | 117.9 | one record of a query for the best eight records of a VectorDB space of 100 records | 0.111 | 942 | The legacy case ranks 1,000 vectors of 384 floats in memory. The space query ranks exact numbers inside a deploy, and the collection holds at most a few hundred records under the limits. |
| `lock_contention_same_channel/produce/8` | 90.5 | one of eight parallel sends to one channel of a bag space | 0.386 | 4,264 | The legacy case writes a shared map from eight threads. The eight space jobs share the footprint key of their space ([I-11](decision-register.md#i-11)), so they run one after another. |
| `lock_contention_different_channels/isolated_channels/8` | 81.9 | one of eight parallel sends to eight channels of a bag space | 0.427 | 5,214 | The legacy case writes eight maps from eight threads. The jobs of different channels of one space also share its footprint key, so they also run one after another. |
| `atomic_ordering/seqcst_fetch_add` | 1.8 | one send and one drain round of a queue space, in a deploy of 10 sends | 0.876 | 479,935 | The legacy case increments an atomic counter. The order of a queue record now comes from the counter record of the collection, with its events. |

### 6.3 The legacy groups with no counterpart

*Table 26. The legacy groups that have no counterpart, and why.*

| Legacy groups | Reason |
|---|---|
| `phlogiston` | The legacy gas meter is deleted. One charging path remains ([M-3](decision-register.md#m-3)), and each case records its phlo instead. |
| `registry_lookup`, `registry_scale` | The legacy registry of spaces is deleted. A space is reached through its handle, so the cold attach of Section 5 is the nearest measure. |
| `vectordb_cosine`, `vectordb_batch_cosine`, `matcher_comparison/vectordb_match` | These cases time a similarity kernel of floats alone. The VectorDB space computes exact similarities inside a deploy ([I-9](decision-register.md#i-9)), so the query cases of Table 16 measure it. |
| `matcher_comparison/wildcard_match` | The wildcard matcher of the legacy branch matched every record. The campaign removed it, and every receive matches its pattern. |
| `collection_types/set_insert_1000_duplicates` | No case sends duplicates to a set. A duplicate send stores a record as any other send does. |
| `checkpoint_operations` | The legacy cases time the soft checkpoints of an in-memory space. The checkpoint of the history trie is measured in Section 5, and a soft checkpoint has no counterpart. |

### 6.4 The ratio to the host program

The host program sends and receives on ordinary channels, so it is the nearest comparison on this branch.
Table 27 divides the median of each space case by the median of its host case of the same size.

*Table 27. The space cases against the host program.*

| Space case | Median of the space case, ms | Median of the host program, ms | Space over host |
|---|---|---|---|
| `send/bag/10` | 3.70 | 0.64 | 5.80 |
| `send/set/10` | 3.76 | 0.64 | 5.91 |
| `send/queue/10` | 6.04 | 0.64 | 9.48 |
| `send/stack/10` | 6.38 | 0.64 | 10.01 |
| `send/priorityqueue/10` | 7.60 | 0.64 | 11.93 |
| `send/vectordb/10` | 5.48 | 0.64 | 8.60 |
| `send_receive/bag/10` | 8.50 | 2.17 | 3.92 |
| `send_receive/set/10` | 9.13 | 2.17 | 4.22 |
| `send_receive/queue/10` | 13.54 | 2.17 | 6.25 |
| `send_receive/stack/10` | 13.95 | 2.17 | 6.44 |
| `send_receive/priorityqueue/10` | 15.47 | 2.17 | 7.14 |
| `send_receive/vectordb/10` | 16.13 | 2.17 | 7.45 |
| `receive_from_n/bag/10` | 1.72 | 0.45 | 3.85 |
| `receive_from_n/set/10` | 1.84 | 0.45 | 4.12 |
| `receive_from_n/queue/10` | 2.05 | 0.45 | 4.60 |
| `receive_from_n/stack/10` | 2.04 | 0.45 | 4.56 |
| `receive_from_n/priorityqueue/10` | 2.06 | 0.45 | 4.63 |
| `receive_from_n/vectordb/10` | 2.51 | 0.45 | 5.62 |
| `receive_from_n/bag/100` | 3.62 | 0.92 | 3.93 |
| `receive_from_n/set/100` | 4.62 | 0.92 | 5.01 |
| `receive_from_n/queue/100` | 4.11 | 0.92 | 4.46 |
| `receive_from_n/stack/100` | 4.10 | 0.92 | 4.45 |
| `receive_from_n/priorityqueue/100` | 4.28 | 0.92 | 4.64 |
| `receive_from_n/vectordb/100` | 9.29 | 0.92 | 10.09 |
| `receive_from_n/bag/1000` | 25.74 | 5.79 | 4.45 |
| `receive_from_n/queue/1000` | 27.12 | 5.79 | 4.68 |
| `receive_from_n/stack/1000` | 27.48 | 5.79 | 4.75 |

The experiments of Table 28 test the differences that matter with 60 replicates of each arm.
Each experiment is open in pgmcp with its criterion fixed before the measurement.

*Table 28. The experiments of step S8.*

| Experiment | Title | Hypothesis | Arms | Verdict | Result of the test |
|---|---|---|---|---|---|
| 421 | A space send against a host send | A deploy of 10 sends to a bag space takes more time than 10 sends of the host program | `send/host/10` (control), `send/bag/10` (treatment), 60 replicates each | accepted | Welch's t-test: $`t = 115.90`$, $`p < 10^{-6}`$, $`d = 21.16`$, and the difference is 3.09 to 3.19 ms |
| 422 | A receive against the size of its collection | A receive from a bag space of 1,000 records takes more time than one from 10 records | `receive_from_n/bag/10` (control), `receive_from_n/bag/1000` (treatment), 60 replicates each | accepted | Welch's t-test: $`t = 332.56`$, $`p < 10^{-6}`$, $`d = 60.72`$, and the difference is 24.57 to 24.87 ms |
| 423 | A cold attach after a restart against a reset | The first receive from a bag space of 100 records takes more time after a restart of its LMDB store than after a reset | `restart_attach/bag/100` (control), `restart_attach/bag/100/cold` (treatment), 60 replicates each | accepted | Welch's t-test: $`t = 11.55`$, $`p < 10^{-6}`$, $`d = 2.11`$, and the difference is 0.14 to 0.20 ms |
| 424 | The examples from end to end | At least 95 % of the end-to-end runs of the examples finish in less than 1,000 ms | every run of the 70 example commands (one arm), 10 runs each | accepted | The 95th percentile of the 700 runs is 53.5 ms |

### 6.5 The restored archives

Step S8 restored the benchmark archives of phase P0 from the backup of 2026-10-04.
Their logs held the workspace path of the capture host, so the restore replaced that path.
The file `test-vectors/benchmarks/p0-archives-provenance.json` maps each original digest to the committed digest.
Besides the legacy benchmark, the archives hold the cost-base captures of revision `068d51c6` and a workspace run at revision `e748426`.
The cost-base captures time the bincode codec, LMDB keys and values, and RSpace sources.
These captures measure internals of the base, not space operations, so this document does not compare them.

## 7. The examples from end to end

Each example of `examples/reified_rspaces/` runs as one command.
The command is the release binary of the examples harness with `--exact example_NAME`.
One run plays every deploy of the example natively at the limits of protocol version 6.
It checks the values and certifies each deploy by native replay.
The tool hyperfine ([HYP](#ref-hyp)) runs each command once to warm up and then 10 times, on the pinned CPUs.
The 70 medians lie between 15.6 ms and 69.8 ms.

*Table 29. The examples from end to end.*

| Example | Mean ± standard deviation, ms | Median, ms | Minimum, ms | Maximum, ms | Peak resident memory, MiB |
|---|---|---|---|---|---|
| `example_agent_capability_rejects_a_persistent_call` | 19.1 ± 0.5 | 19.2 | 18.1 | 19.6 | 31.7 |
| `example_agent_hashmap_bag` | 70.0 ± 0.6 | 69.8 | 69.3 | 71.1 | 37.0 |
| `example_agent_space_rejects_a_deep_pattern` | 20.7 ± 0.2 | 20.8 | 20.3 | 21.0 | 32.5 |
| `example_agent_space_rejects_a_persistent_send` | 15.7 ± 0.7 | 16.0 | 14.9 | 16.5 | 30.2 |
| `example_agent_space_rejects_a_query` | 16.7 ± 0.5 | 16.8 | 15.6 | 17.3 | 30.3 |
| `example_agent_space_rejects_a_spanning_guard` | 16.2 ± 0.6 | 16.0 | 15.6 | 17.5 | 30.5 |
| `example_agent_space_rejects_params` | 16.5 ± 0.7 | 16.4 | 15.6 | 17.6 | 30.1 |
| `example_agent_wrapper_forwards_to_native` | 38.5 ± 0.7 | 38.7 | 37.2 | 39.6 | 33.6 |
| `example_array_indexed_space` | 26.3 ± 0.8 | 26.4 | 25.2 | 27.8 | 33.1 |
| `example_array_out_of_names` | 20.6 ± 0.7 | 20.4 | 19.9 | 21.9 | 32.1 |
| `example_bag_basics` | 27.9 ± 0.9 | 27.8 | 26.4 | 29.4 | 33.0 |
| `example_bag_peeks_and_persistence` | 43.8 ± 1.0 | 43.4 | 42.9 | 46.2 | 33.6 |
| `example_bag_rejects_a_query` | 21.5 ± 1.3 | 21.3 | 20.2 | 24.8 | 32.6 |
| `example_cell_exactly_once` | 37.0 ± 1.1 | 37.1 | 35.2 | 38.8 | 33.5 |
| `example_cell_occupied` | 26.6 ± 0.4 | 26.7 | 25.8 | 27.2 | 32.8 |
| `example_cell_peeks_and_persistence` | 44.5 ± 0.6 | 44.5 | 43.6 | 45.5 | 34.1 |
| `example_cross_space_join` | 25.1 ± 0.7 | 24.9 | 24.0 | 26.5 | 32.7 |
| `example_determinism_demo` | 32.4 ± 0.8 | 32.4 | 30.6 | 33.4 | 33.0 |
| `example_factory_rejects_a_bad_argument` | 17.7 ± 0.4 | 17.7 | 17.1 | 18.3 | 30.9 |
| `example_factory_rejects_a_forged_params_tuple` | 19.0 ± 1.1 | 19.0 | 17.9 | 21.3 | 31.3 |
| `example_factory_rejects_a_missing_param` | 17.3 ± 0.5 | 17.2 | 16.5 | 18.3 | 30.9 |
| `example_factory_rejects_a_persistent_call` | 15.4 ± 0.5 | 15.6 | 14.9 | 16.4 | 29.5 |
| `example_factory_rejects_a_positional_param` | 16.7 ± 0.8 | 16.5 | 15.8 | 18.3 | 30.8 |
| `example_factory_rejects_an_invalid_param` | 17.5 ± 0.7 | 17.4 | 16.5 | 18.8 | 31.0 |
| `example_factory_rejects_an_unknown_param` | 17.6 ± 0.6 | 17.6 | 16.8 | 18.6 | 31.2 |
| `example_factory_rejects_an_unknown_theory` | 17.6 ± 0.4 | 17.5 | 17.2 | 18.2 | 31.3 |
| `example_fips_example` | 46.9 ± 0.8 | 47.1 | 45.9 | 48.0 | 35.7 |
| `example_get_space_agent` | 26.9 ± 0.5 | 27.1 | 26.2 | 27.5 | 32.5 |
| `example_handle_methods` | 35.9 ± 0.9 | 36.0 | 34.5 | 37.6 | 33.2 |
| `example_handle_rejects_a_persistent_call` | 20.3 ± 0.7 | 20.4 | 19.0 | 21.0 | 31.7 |
| `example_hashset_dedup_spaces` | 49.1 ± 0.6 | 49.0 | 48.1 | 50.0 | 34.2 |
| `example_hashset_rejects_a_pattern` | 20.0 ± 0.6 | 20.0 | 19.3 | 21.1 | 32.2 |
| `example_hashset_rejects_a_payload` | 19.6 ± 0.5 | 19.5 | 18.8 | 20.2 | 32.2 |
| `example_multi_space_isolation` | 29.2 ± 1.4 | 29.0 | 27.7 | 31.8 | 33.5 |
| `example_pathmap_hierarchical_spaces` | 49.3 ± 0.8 | 49.2 | 48.5 | 51.2 | 34.7 |
| `example_pathmap_prefix_aggregation` | 49.2 ± 3.3 | 48.2 | 46.8 | 58.1 | 35.0 |
| `example_priority_queue` | 50.4 ± 0.8 | 50.7 | 48.6 | 51.7 | 34.3 |
| `example_priority_queue_peeks_and_persistence` | 47.2 ± 1.0 | 46.9 | 45.9 | 48.8 | 34.1 |
| `example_priority_queue_rejects_a_level` | 20.8 ± 0.5 | 20.8 | 20.0 | 21.7 | 32.5 |
| `example_queue_fifo` | 39.4 ± 1.2 | 39.2 | 37.9 | 41.9 | 33.5 |
| `example_queue_peeks_and_persistence` | 44.4 ± 1.1 | 44.5 | 42.8 | 45.9 | 34.1 |
| `example_queue_rejects_a_send_param` | 21.2 ± 0.9 | 21.3 | 20.0 | 23.1 | 32.3 |
| `example_seq_rejects_concurrent_uses` | 24.3 ± 0.8 | 24.2 | 23.2 | 25.7 | 32.1 |
| `example_set_idempotent` | 36.8 ± 0.8 | 37.1 | 35.3 | 37.7 | 33.3 |
| `example_set_peeks_and_persistence` | 41.7 ± 1.1 | 41.7 | 39.9 | 43.4 | 33.9 |
| `example_set_rejects_a_send_param` | 20.7 ± 0.7 | 20.6 | 19.6 | 21.6 | 32.2 |
| `example_space_annotations` | 27.8 ± 0.7 | 27.9 | 26.6 | 28.6 | 33.7 |
| `example_space_qualifier_default` | 33.1 ± 1.0 | 33.0 | 31.4 | 34.7 | 33.3 |
| `example_space_qualifier_seq` | 30.7 ± 1.0 | 30.7 | 29.4 | 32.5 | 33.0 |
| `example_space_qualifier_temp` | 27.0 ± 1.0 | 27.5 | 25.6 | 28.0 | 32.6 |
| `example_space_qualifiers_seq_sent` | 22.4 ± 0.6 | 22.2 | 21.5 | 23.4 | 32.3 |
| `example_speculative_execution` | 41.4 ± 0.8 | 41.2 | 40.3 | 43.1 | 34.2 |
| `example_stack_lifo` | 39.7 ± 1.1 | 39.8 | 37.7 | 41.6 | 33.5 |
| `example_stack_peeks_and_persistence` | 43.6 ± 0.9 | 43.4 | 42.0 | 45.2 | 33.7 |
| `example_stack_rejects_a_query` | 22.5 ± 0.7 | 22.5 | 21.0 | 23.5 | 32.4 |
| `example_system_op_rejects_a_persistent_call` | 15.9 ± 0.9 | 15.9 | 14.4 | 17.3 | 29.8 |
| `example_system_op_rejects_params` | 16.1 ± 0.6 | 16.1 | 15.4 | 17.3 | 30.2 |
| `example_temp_space_ends_with_its_deploy` | 29.4 ± 0.9 | 29.2 | 28.1 | 30.7 | 33.0 |
| `example_theory_refuses_a_value` | 20.6 ± 0.7 | 20.7 | 19.7 | 21.7 | 32.4 |
| `example_theory_validated_space` | 25.1 ± 1.1 | 25.2 | 23.9 | 27.6 | 32.9 |
| `example_vector_indexed_space` | 32.3 ± 1.2 | 32.0 | 31.2 | 34.8 | 33.8 |
| `example_vector_ops` | 31.4 ± 0.9 | 31.5 | 30.0 | 33.4 | 32.6 |
| `example_vectordb_capacity_exceeded` | 28.4 ± 0.8 | 28.3 | 27.1 | 29.4 | 32.8 |
| `example_vectordb_dimension_mismatch` | 21.6 ± 0.8 | 21.7 | 20.1 | 23.0 | 32.3 |
| `example_vectordb_embed` | 19.1 ± 0.7 | 19.2 | 18.0 | 20.2 | 31.4 |
| `example_vectordb_list_scores` | 35.8 ± 0.7 | 35.8 | 34.6 | 37.1 | 33.6 |
| `example_vectordb_mirror` | 44.4 ± 0.8 | 44.4 | 43.4 | 45.4 | 34.6 |
| `example_vectordb_multi_topic` | 54.8 ± 1.4 | 54.5 | 53.5 | 58.3 | 34.5 |
| `example_vectordb_peeks_and_persistence` | 55.0 ± 0.7 | 55.2 | 53.6 | 55.9 | 34.7 |
| `example_vectordb_queries` | 53.6 ± 1.0 | 53.5 | 51.8 | 55.2 | 35.0 |

Experiment 424 checks that 95 % of these runs finish in less than one second.
Its verdict is accepted. The 95th percentile of the 700 runs is 53.5 ms.

## 8. Findings

### 8.1 F1: the host work of one operation grows with its collection

Each space operation reserves the copy of the whole record list of its channel (Section 3.4).
A receive that fires reserves that copy twice: once for its staged view and once for the check of its commit (`committed_candidates`, `rspace++/src/rspace/space_store_handle.rs:2875-2911`).
So a receive reserves 209,458 more VerificationBytes for each record of a bag.
It reserves 606,127 more for each record of a VectorDB collection.

A collection is full at 2,667 records for the bag and at 1,030 records for VectorDB, because one more send exceeds the limit.
A receive fails before that size.
The set, priority-queue and VectorDB receives fail at 1,000 records, and a VectorDB list read fails at 100.
Between these sizes, a send still fits and every receive fails.
Each operation that removes a record loads the whole list first, so no operation can remove a record of such a collection.
These limits are deterministic, so every node rejects the same deploy.

### 8.2 F2: a deploy holds fewer space operations than host operations

A deploy holds 59 sends to a bag space, 37 to a VectorDB space and 20 to a PathMap bag space (Table 3).
A deploy of the host program holds 3,224 sends.
The difference comes from F1 and from the work of each space transaction.
That work is the space job, the read of the config record, the metered view reads and the events (Section 3.5).

### 8.3 F3: a space deploy takes more time than the host program

A deploy of 10 sends to a bag space takes 3.70 ms, and the host program takes 0.64 ms: 5.8 times as long.
Experiment 421 tests this difference with 60 replicates of each arm.
Its verdict is accepted (Welch's t-test: $`t = 115.90`$, $`p < 10^{-6}`$, $`d = 21.16`$, and the difference is 3.09 to 3.19 ms).

A receive from a bag of 1,000 records takes 25.74 ms, against 1.72 ms from 10 records.
Experiment 422 tests this growth.
Its verdict is accepted (Welch's t-test: $`t = 332.56`$, $`p < 10^{-6}`$, $`d = 60.72`$, and the difference is 24.57 to 24.87 ms).

### 8.4 F4: the persistence costs

After a restart of its LMDB store, the first receive from a bag of 100 records takes 3.99 ms, against 3.89 ms after a reset.
Experiment 423 tests this difference.
Its verdict is accepted (Welch's t-test: $`t = 11.55`$, $`p < 10^{-6}`$, $`d = 2.11`$, and the difference is 0.14 to 0.20 ms).
The first block of 10 sends to one bag adds 40,960 bytes to the LMDB store.
Block 10 adds 139,264 bytes (Table 23).
The history stores the record list of a channel as one value, so each block that changes the channel stores the whole list again (`rspace++/src/rspace/history/history_repository_impl.rs:174-185`).

### 8.5 Candidate optimizations

This document adopts no optimization ([decision 85](decision-register.md#decision-85)).
Each candidate below needs its own hypothesis, its own design pass and a decision of the user.

*Table 30. Candidate optimizations for the findings.*

| Candidate | Change | Hypothesis to test | Impact |
|---|---|---|---|
| O1: a copy-free staged view | The staged view keeps the snapshot of the hot store (`native_data_view`, `rspace++/src/rspace/hot_store/native.rs:1129-1154`) and the changes of its own transaction. The check of the commit reads a snapshot too. Parked receives stay shared pointers, as the hot store holds them. | A send to a bag, a set, a queue or a stack no longer reserves work for each stored record. A receive keeps the work of its discipline for each record, such as the head scan of a queue. | It changes the charges of space operations and the rule of [decision 47](decision-register.md#decision-47) for a view load, with the formal models of that rule. Space charges are not released yet. |
| O2: one channel for each record of an ordered collection | A queue, a stack or a priority queue stores each record on its own channel, keyed by its order, so an operation loads only the head. The plan names this layout as a later optimization of its risk table. | The cost of a queue operation does not grow with the queue. | It changes the stored layout of ordered collections and the counter rules of [M-1](decision-register.md#m-1), with their models. |
| O3: one decode of the config record in each deploy | A space job decodes the config record of its space once in each deploy and keeps the decoded value. Each later read still reserves the work of the read, as [decision 47](decision-register.md#decision-47) requires for a hit and a miss alike. | The time of a send to a small collection falls by the share of the config decode (Section 3.5), and no charge changes. | No change of the charges, the events or the state. |

## 9. Reproducing the measurements

The script `scripts/bench/reified-benchmark-with-perf.sh` runs every measurement of this document.
It builds each bench under the caps of `systemd-run`, pins the runs with `taskset`, and writes one run directory under `target/verification/reified-rspaces/benches/`.
The run directory holds `system.txt`, the Criterion output in `criterion/`, one record file for each bench, and the logs.

1. Set the `performance` governor on the pinned CPUs before a measured run.
2. Run `scripts/bench/reified-benchmark-with-perf.sh` for the measured runs of both benches.
3. Run `scripts/bench/reified-benchmark-with-perf.sh --examples` to time the examples with hyperfine.
4. Run `scripts/bench/reified-benchmark-with-perf.sh --profile --filter PATTERN` for a uProf profile of the matching cases.
5. Set `RRS_BENCH_SAMPLES=60` and give the filter of both arms for the run of an experiment.
6. Run `cargo bench -p rholang --bench reified_spaces -- --test` with `RRS_BENCH_MAX_SIZE=1000` for the check that the aggregate gate runs.

A profiled run builds the bench with frame pointers in `target/fp`, with the rustflags of `.cargo/config.toml` for the host target.

A filter is a regular expression over the case keys, as in Criterion, so `^send/(bag|host)/10$` selects the two arms of Section 6.4.
A capture runs a bench with `RRS_BENCH_CAPTURE=FILE` on a copy of the contract, and a reviewer then copies the file to `test-vectors/`.
The check of the contract fails a run whose result, phlo, root or host work differs from the contract.

## References

<a id="ref-efr79"></a>[Efr79] B. Efron, "Bootstrap methods: another look at the jackknife," *The Annals of Statistics*, vol. 7, no. 1, pp. 1–26, 1979. [doi:10.1214/aos/1176344552](https://doi.org/10.1214/aos/1176344552)

<a id="ref-mdh09"></a>[MDH09] T. Mytkowicz, A. Diwan, M. Hauswirth, and P. F. Sweeney, "Producing wrong data without doing anything obviously wrong!," in *Proceedings of the 14th International Conference on Architectural Support for Programming Languages and Operating Systems (ASPLOS XIV)*, ACM, 2009, pp. 265–276. [doi:10.1145/1508244.1508275](https://doi.org/10.1145/1508244.1508275)

<a id="ref-wel47"></a>[Wel47] B. L. Welch, "The generalization of 'Student's' problem when several different population variances are involved," *Biometrika*, vol. 34, no. 1–2, pp. 28–35, 1947. [doi:10.1093/biomet/34.1-2.28](https://doi.org/10.1093/biomet/34.1-2.28)

<a id="ref-crit"></a>[CRIT] B. Heisler and the Criterion.rs authors, "Criterion.rs," version 0.5.1, a statistics-driven benchmarking library for Rust. [docs.rs/criterion/0.5.1](https://docs.rs/criterion/0.5.1/criterion/)

<a id="ref-hyp"></a>[HYP] D. Peter, "hyperfine," version 2.0.0, a command-line benchmarking tool. [github.com/sharkdp/hyperfine](https://github.com/sharkdp/hyperfine)

<a id="ref-uprof"></a>[UPROF] Advanced Micro Devices, "AMD uProf," version 5.3.521.0, a performance analysis tool for AMD processors. [amd.com/en/developer/uprof.html](https://www.amd.com/en/developer/uprof.html)

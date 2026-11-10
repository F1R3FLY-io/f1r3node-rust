# Reified RSpaces performance

This document reports what the space operations of Reified RSpaces cost in time and phlo, and what bounds them.
Phase P10 measures, as step S8 of its design v2 states ([decision 85](decision-register.md#decision-85)).
An optimization needs its own hypothesis and its own decision, so this document adopts none.
Section 8 lists the findings and the candidate optimizations for a decision of the user.

Read the [glossary](glossary.md) first, because this document uses its terms.
The [specification](specification.md) states the behavior that the benchmarks exercise, and the [verification dossier](verification.md#35-phase-p10-the-formal-verification-closure-the-documents-the-examples-and-the-benchmarks) records the step.

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

The benchmarks measure the [native play](glossary.md#native-play) of [deploys](glossary.md#deploy) that use spaces.
Every measured deploy runs on the test chain of `rholang/tests/support/native.rs`, as the tests and the examples harness do.
So a measured time includes the parse and the normalization of the deploy, its evaluation, its space transactions, its events and its metering.
A case of the persistence benchmarks also includes the [checkpoint](glossary.md#checkpoint) after the deploy.

The benchmarks do not measure what the campaign did not port.
The legacy hotspot items are the matrix arena, the copy-on-write checkpoint and the BTreeMap against the PathMap crate.
They target structures that the campaign replaced ([I-9](decision-register.md#i-9), [M-2](decision-register.md#m-2), [I-1](decision-register.md#i-1)).
The MeTTa groups are held ([decision 2](decision-register.md#decision-2)), and no benchmark calls the external ChromaDB service.

Figure 1 shows the path of one benchmark case, from its setup to its timed iterations.

![The path of one benchmark case as an activity diagram in three parts. The setup is not timed: a new test chain over an in-memory store, the setup deploys with the factory call of the space, and then fill deploys of ten sends, or of one send on an ordered family, each followed by a checkpoint. The reference play is checked and not timed: it evaluates the measured deploy once with a phlo limit of ten to the ninth, reads the values on the result channel and the phlo, and takes the checkpoint. If a field of the class of the case differs from the contract, the run fails. The record line is appended, and a deploy that failed is not timed. The timed iterations repeat: reset the chain to the pre-state root outside the timer, start the timer, parse and normalize the source text, reduce with each space operation as a space job, make view reads that load the records of the channel into the staged view, match, stage and commit with the events and the observer calls that charge the phlo, take the checkpoint only when the case includes it, and stop the timer. Criterion then reports the median of ten samples of equal iterations and its 95 percent bootstrap interval.](diagrams/56-benchmark-case-path.svg)

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

The timed run of 2026-11-10 used the same CPU, memory, pinned CPUs, governor, driver, boost and compiler, with these changes:

- System: Linux 7.2.14.
- The load average at the start of the run: 26.54 over one minute, from other processes on other CPUs.
- Tree: revision `dde077f81`, with no change to a tracked file.

Before the run, a gate waited until CPUs 2 to 5 and their SMT siblings were at least 90 % idle for 60 s.
During the run, the SMT siblings of CPUs 2 and 3 stayed at least 91 % idle in each minute.
The run folder records these facts in `system.txt`, `gate.json` and `mpstat-during.txt`.

### 2.2 The test chain and the limits

A benchmark plays each deploy on the native test chain `NativeChain` of the test support.
The chain keeps its history trie in an in-memory store, and a tokio runtime with two worker threads runs its tasks.

Every benchmark deploy has a phlo limit of 1,000,000,000.
No measured deploy reaches it.
The test suites use a phlo limit of 1,000,000.
In the measurement before [decision 89](decision-register.md#decision-89), a series of host sends reached that limit at 1,503 sends.

### 2.3 One case

A benchmark case has a setup, a reference play and timed iterations, as Figure 1 shows.
The setup builds the pre-state of the case with setup deploys, and it is not timed.
A fill step stores records with deploys of ten sends, or one send for each deploy on an ordered family.

The reference play evaluates the measured deploy once.
It reads the values on the result channel, the phlo and the root after the checkpoint.
The run then checks the fields of the class of the case against the contract of Section 2.4.
Each timed iteration resets the chain to the pre-state outside the timer, and it times the deploy and, when the case says so, the checkpoint.

### 2.4 The contract

The file `test-vectors/reified-bench-contract-v2.json` holds the result, the phlo and the root of each case.
Every run checks each case against the file, a run in test mode included.

Each case belongs to a class that names the fields a run compares (`compare_of` in `rholang/benches/common/mod.rs`).
Class `All` compares the result, the phlo and the root.
Class `NoRoot` compares the result and the phlo.
Class `Pick` compares the form of the result and the phlo.

A capture with `RRS_BENCH_CAPTURE` checks the cases that a capture file holds, and it adds the cases that the file does not hold yet.

### 2.5 The series

A one-deploy series measures one deploy with 1, 10 and 100 operations.
The series `queue_ordering/send_and_drain` runs the counts 1, 10 and 31 (the constant `QUEUE_ORDERING_COUNTS` in `rholang/benches/reified_spaces.rs`).

A read series measures one operation on a collection of 10, 100, 1,000 and 10,000 records.
It fills the collection in setup deploys.
The checkpoint series of Section 5.2 spreads its records over channels of 100 records.
The timed run of 2026-11-10 sets `RRS_BENCH_MAX_SIZE` to 1,000, so it measures no collection of more than 1,000 records.

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

Table 2 and Tables 10 to 24 come from the timed run of 2026-11-10 (Section 2.1).
The after column of Table 25, Table 27 and the medians in Sections 3.4, 8.3 and 8.4 also come from that run.
Tables 3 to 9 and Figure 2 keep the measurement of step S8, before [decision 89](decision-register.md#decision-89).
The timed run measures no host work and runs no profile.

*Changed by [decision 89](decision-register.md#decision-89): the tables of the timed run have no column of VerificationBytes, because a deploy meters no host work. Before, the last row of each series in Tables 11 and 12 was the most operations that fit the host-work limits.*

### 3.1 One operation

Table 2 lists the phlo of one send, and of one send-receive pair, for each family and for the host program.
The host program sends on ordinary channels, with no space.
The id of a data channel or a collection counter holds the [collection key](glossary.md#collection-key) of the channel.
So each `Par` of the inner channel adds 8 bytes to the id ([decision 112](decision-register.md#decision-112)).
The channel bytes of a produce or a consume pay for the id at the byte rate.

*Table 2. The cost of one operation in a deploy of one operation.*

| Family | Phlo of one send | Phlo of one send-receive pair |
|---|---|---|
| bag | 4,382 | 6,525 |
| set | 4,382 | 6,525 |
| queue | 6,644 | 10,462 |
| stack | 6,644 | 10,462 |
| priority queue | 6,668 | 10,487 |
| VectorDB | 4,457 | 6,725 |
| cell | 7,145 | not measured |
| PathMap bag | 8,005 | not measured |
| Array bag | 5,869 | not measured |
| Vector bag | 5,869 | not measured |
| HashSet bag | 7,505 | not measured |
| host program | 666 | 2,540 |

### 3.2 Operations in one deploy

Table 3 lists the most sends that one deploy held for each family in the measurement before [decision 89](decision-register.md#decision-89).
Table 4 lists the most send-receive pairs.

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

### 3.3 Host work grew with the collection

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

Figure 2 draws the receive lines against the limit.

![A line chart of the VerificationBytes of one receive, in millions, against the records of its collection, from 0 to 1,400 records. The chart keeps the measurement before decision 89. A horizontal line marks the retired limit of 268.4 million VerificationBytes. Each discipline has the least-squares line of Table 5, drawn from 0 records to the size where it reaches the limit: VectorDB at 433 records, the set at 907 records, the priority queue at 993 records, the queue and stack at 1,136 records, and the bag at 1,267 records. Black dots mark the measured receives of 10, 100 and 1,000 records that fit the limit, and they lie on the lines. The host program reserved 0.36 million at every size, so the chart does not draw it.](diagrams/57-receive-host-work.svg)

*Figure 2. The VerificationBytes of one receive against the records of its collection (`diagrams/57-receive-host-work.puml`).*

### 3.4 The cause

A space transaction reads the channels of its operations through its [staged view](glossary.md#staged-view).
The first read of a channel in a transaction loads every record of the channel into the view (`TxnView::load_data`, `rspace++/src/rspace/space_store_handle.rs:1909-1923`).
A send, a receive and a fire each read the list of their channel (`TxnCore::read_data` at `rspace++/src/rspace/space_store_handle.rs:3273-3278` and `TxnCore::load_group` at `rspace++/src/rspace/space_store_handle.rs:3529-3537`).

Native play of the host program copies the data of its channel too (`fetch_channel_to_index_data`, `rspace++/src/rspace/rspace/ops_consume.rs:313-325`).
Its time grows: a host receive takes 0.32 ms at 10 records and 5.68 ms at 1,000 records.
A bag receive takes 1.02 ms and 12.91 ms at the same sizes.
So the copy costs time on both paths.

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

In a collection of 1,000 records, the reads of the staged view took most of the time.
They decoded the record list from the history and copied it into the staged view.
In a small collection, the read and the decode of the config record of the space were a large share of each send.
Section 8 names a candidate optimization for each of the two costs.

## 4. Results of the operation benchmarks

Each table of this section lists the cases of one group of `rholang/benches/reified_spaces.rs`.
The outcome is "fits" when the deploy succeeds, and otherwise it is the error of the deploy.
A case that does not fit is recorded and not timed.
The phlo is that of the native contract with the weights of the test chain.

### 4.1 The factory

*Table 10. One factory call that creates a space.*

| Case | Outcome | Phlo | Median time in ms (95 % interval) |
|---|---|---|---|
| `factory/hashmap_bag` | fits | 7,150 | 0.69 (0.68 to 0.71) |
| `factory/hashmap_vectordb` | fits | 7,294 | 0.80 (0.79 to 0.82) |

### 4.2 Sends

The send cases store records in one collection of each family, and in one channel of the host program.
The cell series sends to n channels, because a cell holds one record.

*Table 11. Sends in one deploy.*

| Case | Outcome | Phlo | Median time in ms (95 % interval) |
|---|---|---|---|
| `send/array_bag/1` | fits | 5,869 | 0.93 (0.89 to 0.96) |
| `send/array_bag/10` | fits | 14,127 | 2.50 (2.48 to 2.54) |
| `send/array_bag/100` | fits | 96,729 | 26.46 (26.22 to 26.76) |
| `send/bag/1` | fits | 4,382 | 0.59 (0.58 to 0.60) |
| `send/bag/10` | fits | 12,215 | 2.00 (1.99 to 2.01) |
| `send/bag/100` | fits | 90,515 | 24.21 (24.10 to 24.33) |
| `send/cell/1` | fits | 7,145 | 0.83 (0.81 to 0.84) |
| `send/cell/10` | fits | 39,845 | 3.81 (3.72 to 3.88) |
| `send/cell/100` | fits | 366,996 | 39.56 (38.62 to 40.22) |
| `send/hashset_bag/1` | fits | 7,505 | 0.99 (0.99 to 1.01) |
| `send/hashset_bag/10` | fits | 17,525 | 2.81 (2.80 to 2.83) |
| `send/hashset_bag/100` | fits | 117,731 | 24.75 (24.53 to 25.24) |
| `send/host/1` | fits | 666 | 0.10 (0.10 to 0.10) |
| `send/host/10` | fits | 6,660 | 0.43 (0.43 to 0.43) |
| `send/host/100` | fits | 66,636 | 5.25 (5.12 to 5.29) |
| `send/pathmap_bag/1` | fits | 8,005 | 0.88 (0.87 to 0.90) |
| `send/pathmap_bag/10` | fits | 48,445 | 4.74 (4.68 to 4.79) |
| `send/pathmap_bag/100` | fits | 452,960 | 81.39 (79.94 to 82.50) |
| `send/priorityqueue/1` | fits | 6,668 | 0.78 (0.76 to 0.78) |
| `send/priorityqueue/10` | fits | 29,792 | 3.24 (3.22 to 3.28) |
| `send/priorityqueue/100` | fits | 261,111 | 41.33 (40.65 to 41.95) |
| `send/queue/1` | fits | 6,644 | 0.72 (0.71 to 0.74) |
| `send/queue/10` | fits | 29,552 | 2.95 (2.93 to 2.97) |
| `send/queue/100` | fits | 258,711 | 34.81 (34.49 to 34.99) |
| `send/set/1` | fits | 4,382 | 0.60 (0.59 to 0.61) |
| `send/set/10` | fits | 12,215 | 1.99 (1.98 to 1.99) |
| `send/set/100` | fits | 90,515 | 24.22 (23.96 to 24.45) |
| `send/stack/1` | fits | 6,644 | 0.74 (0.73 to 0.76) |
| `send/stack/10` | fits | 29,552 | 3.01 (3.00 to 3.03) |
| `send/stack/100` | fits | 258,711 | 37.23 (36.63 to 37.67) |
| `send/vector_bag/1` | fits | 5,869 | 0.84 (0.82 to 0.86) |
| `send/vector_bag/10` | fits | 14,127 | 2.28 (2.27 to 2.30) |
| `send/vector_bag/100` | fits | 96,729 | 24.80 (24.70 to 25.01) |
| `send/vectordb/1` | fits | 4,457 | 0.73 (0.73 to 0.75) |
| `send/vectordb/10` | fits | 12,965 | 3.06 (3.03 to 3.10) |
| `send/vectordb/100` | fits | 98,015 | 46.65 (46.09 to 47.55) |

### 4.3 Send-receive pairs

*Table 12. Send-receive pairs in one deploy.*

| Case | Outcome | Phlo | Median time in ms (95 % interval) |
|---|---|---|---|
| `send_receive/bag/1` | fits | 6,525 | 0.84 (0.83 to 0.84) |
| `send_receive/bag/10` | fits | 33,645 | 5.69 (5.48 to 5.85) |
| `send_receive/bag/100` | fits | 304,815 | 86.32 (82.93 to 95.26) |
| `send_receive/host/1` | fits | 2,540 | 0.23 (0.22 to 0.23) |
| `send_receive/host/10` | fits | 25,400 | 1.89 (1.88 to 1.90) |
| `send_receive/host/100` | fits | 254,108 | 41.91 (39.81 to 46.45) |
| `send_receive/priorityqueue/1` | fits | 10,487 | 1.15 (1.14 to 1.16) |
| `send_receive/priorityqueue/10` | fits | 67,972 | 8.22 (8.07 to 8.30) |
| `send_receive/priorityqueue/100` | fits | 643,211 | 118.52 (116.15 to 120.42) |
| `send_receive/queue/1` | fits | 10,462 | 1.11 (1.09 to 1.12) |
| `send_receive/queue/10` | fits | 67,732 | 7.88 (7.75 to 7.94) |
| `send_receive/queue/100` | fits | 640,811 | 104.44 (103.40 to 105.33) |
| `send_receive/set/1` | fits | 6,525 | 0.89 (0.88 to 0.90) |
| `send_receive/set/10` | fits | 33,645 | 6.02 (5.91 to 6.17) |
| `send_receive/set/100` | fits | 304,815 | 112.77 (111.19 to 120.48) |
| `send_receive/stack/1` | fits | 10,462 | 1.12 (1.11 to 1.14) |
| `send_receive/stack/10` | fits | 67,732 | 8.04 (7.87 to 8.08) |
| `send_receive/stack/100` | fits | 640,811 | 104.93 (103.65 to 106.05) |
| `send_receive/vectordb/1` | fits | 6,725 | 1.17 (1.17 to 1.18) |
| `send_receive/vectordb/10` | fits | 35,635 | 9.81 (9.74 to 9.92) |
| `send_receive/vectordb/100` | fits | 324,715 | 266.00 (261.42 to 269.20) |

### 4.4 A receive from n records

*Table 13. One receive from a collection of n records.*

| Case | Outcome | Phlo | Median time in ms (95 % interval) |
|---|---|---|---|
| `receive_from_n/bag/10` | fits | 8,386 | 1.02 (1.01 to 1.03) |
| `receive_from_n/bag/100` | fits | 8,386 | 1.98 (1.95 to 2.00) |
| `receive_from_n/bag/1000` | fits | 8,386 | 12.91 (12.86 to 13.09) |
| `receive_from_n/host/10` | fits | 2,599 | 0.32 (0.32 to 0.32) |
| `receive_from_n/host/100` | fits | 2,599 | 0.80 (0.79 to 0.80) |
| `receive_from_n/host/1000` | fits | 2,599 | 5.68 (5.59 to 5.74) |
| `receive_from_n/priorityqueue/10` | fits | 10,061 | 1.16 (1.15 to 1.16) |
| `receive_from_n/priorityqueue/100` | fits | 10,064 | 2.15 (2.13 to 2.17) |
| `receive_from_n/priorityqueue/1000` | fits | 10,064 | 13.68 (13.64 to 13.77) |
| `receive_from_n/queue/10` | fits | 10,061 | 1.15 (1.14 to 1.15) |
| `receive_from_n/queue/100` | fits | 10,064 | 2.09 (2.09 to 2.12) |
| `receive_from_n/queue/1000` | fits | 10,064 | 12.94 (12.87 to 13.08) |
| `receive_from_n/set/10` | fits | 8,386 | 1.12 (1.11 to 1.12) |
| `receive_from_n/set/100` | fits | 8,386 | 2.48 (2.46 to 2.52) |
| `receive_from_n/set/1000` | fits | 8,386 | 18.50 (18.15 to 18.75) |
| `receive_from_n/stack/10` | fits | 10,061 | 1.15 (1.15 to 1.17) |
| `receive_from_n/stack/100` | fits | 10,064 | 2.12 (2.10 to 2.15) |
| `receive_from_n/stack/1000` | fits | 10,064 | 13.01 (12.92 to 13.13) |
| `receive_from_n/vectordb/10` | fits | 8,605 | 1.47 (1.46 to 1.49) |
| `receive_from_n/vectordb/100` | fits | 8,605 | 5.32 (5.23 to 5.37) |
| `receive_from_n/vectordb/1000` | fits | 8,605 | 57.73 (57.50 to 57.82) |

### 4.5 A send to n records

*Table 14. One send to a collection of n records.*

| Case | Outcome | Phlo | Median time in ms (95 % interval) |
|---|---|---|---|
| `send_to_n/bag/10` | fits | 4,382 | 0.67 (0.67 to 0.68) |
| `send_to_n/bag/100` | fits | 4,382 | 1.46 (1.45 to 1.46) |
| `send_to_n/bag/1000` | fits | 4,382 | 10.29 (10.20 to 10.36) |
| `send_to_n/host/10` | fits | 666 | 0.14 (0.14 to 0.14) |
| `send_to_n/host/100` | fits | 667 | 0.50 (0.50 to 0.50) |
| `send_to_n/host/1000` | fits | 667 | 4.06 (4.06 to 4.08) |
| `send_to_n/priorityqueue/10` | fits | 6,081 | 0.79 (0.79 to 0.80) |
| `send_to_n/priorityqueue/100` | fits | 6,084 | 1.70 (1.69 to 1.73) |
| `send_to_n/priorityqueue/1000` | fits | 6,084 | 11.39 (11.18 to 11.49) |
| `send_to_n/queue/10` | fits | 6,057 | 0.76 (0.75 to 0.76) |
| `send_to_n/queue/100` | fits | 6,060 | 1.65 (1.62 to 1.66) |
| `send_to_n/queue/1000` | fits | 6,060 | 11.49 (11.35 to 11.70) |
| `send_to_n/set/10` | fits | 4,382 | 0.67 (0.66 to 0.68) |
| `send_to_n/set/100` | fits | 4,382 | 1.49 (1.49 to 1.49) |
| `send_to_n/set/1000` | fits | 4,382 | 10.91 (10.77 to 11.10) |
| `send_to_n/stack/10` | fits | 6,057 | 0.77 (0.77 to 0.78) |
| `send_to_n/stack/100` | fits | 6,060 | 1.65 (1.63 to 1.66) |
| `send_to_n/stack/1000` | fits | 6,060 | 11.08 (10.95 to 11.16) |
| `send_to_n/vectordb/10` | fits | 4,457 | 0.85 (0.85 to 0.86) |
| `send_to_n/vectordb/100` | fits | 4,457 | 2.44 (2.43 to 2.48) |
| `send_to_n/vectordb/1000` | fits | 4,457 | 22.39 (22.05 to 22.69) |

### 4.6 Matching

The `bag_scan_miss` case uses a pattern that no record matches, so the bag inspects every record, and the receive then waits.
The `bag_first` case matches a record that play picks at random, and the `queue_head` case inspects only the head.
The `join_two` case joins two channels of a bag that each hold n records, so the receive reserves one record on each channel.
The `send_fires_waiter` case parks n receives on one channel, and the measured send fires one of them: the produce side of a COMM.

*Table 15. Matching over a collection of n records.*

| Case | Outcome | Phlo | Median time in ms (95 % interval) |
|---|---|---|---|
| `matcher/bag_first/10` | fits | 8,386 | 1.06 (1.05 to 1.07) |
| `matcher/bag_first/100` | fits | 8,386 | 2.02 (2.00 to 2.03) |
| `matcher/bag_first/1000` | fits | 8,386 | 13.49 (13.46 to 13.65) |
| `matcher/bag_scan_miss/10` | fits | 4,561 | 0.81 (0.81 to 0.82) |
| `matcher/bag_scan_miss/100` | fits | 4,561 | 1.77 (1.76 to 1.79) |
| `matcher/bag_scan_miss/1000` | fits | 4,561 | 11.90 (11.67 to 12.01) |
| `matcher/join_two/10` | fits | 11,190 | 1.34 (1.33 to 1.37) |
| `matcher/join_two/100` | fits | 11,190 | 3.65 (3.62 to 3.67) |
| `matcher/join_two/1000` | fits | 11,190 | 29.09 (28.91 to 29.17) |
| `matcher/queue_head/10` | fits | 10,061 | 1.15 (1.14 to 1.15) |
| `matcher/queue_head/100` | fits | 10,064 | 2.11 (2.10 to 2.15) |
| `matcher/queue_head/1000` | fits | 10,064 | 13.77 (13.64 to 13.88) |
| `matcher/send_fires_waiter/10` | fits | 5,572 | 0.90 (0.89 to 0.90) |
| `matcher/send_fires_waiter/100` | fits | 5,572 | 2.05 (2.05 to 2.06) |
| `matcher/send_fires_waiter/1000` | fits | 5,572 | 15.57 (15.25 to 15.80) |

### 4.7 VectorDB queries

The queries select by a threshold, the best record, the best eight records, or every record of the collection.

*Table 16. VectorDB queries over a collection of n records.*

| Case | Outcome | Phlo | Median time in ms (95 % interval) |
|---|---|---|---|
| `vectordb_query/best/10` | fits | 8,605 | 1.53 (1.51 to 1.56) |
| `vectordb_query/best/100` | fits | 8,605 | 5.60 (5.36 to 5.68) |
| `vectordb_query/best/1000` | fits | 8,605 | 62.14 (60.90 to 63.04) |
| `vectordb_query/k8/10` | fits | 19,480 | 2.85 (2.81 to 2.87) |
| `vectordb_query/k8/100` | fits | 19,480 | 8.84 (8.74 to 8.95) |
| `vectordb_query/k8/1000` | fits | 19,480 | 85.72 (83.23 to 91.24) |
| `vectordb_query/list_all/10` | fits | 22,703 | 2.94 (2.91 to 2.96) |
| `vectordb_query/list_all/100` | fits | 170,213 | 32.50 (32.03 to 32.87) |
| `vectordb_query/list_all/1000` | fits | 1,645,328 | 1,694.76 (1,622.20 to 1,769.00) |
| `vectordb_query/threshold/10` | fits | 8,663 | 1.60 (1.59 to 1.61) |
| `vectordb_query/threshold/100` | fits | 8,663 | 5.84 (5.80 to 5.86) |
| `vectordb_query/threshold/1000` | fits | 8,663 | 63.78 (62.94 to 64.46) |

### 4.8 Concurrent operations

Each case runs eight operations in parallel in one deploy.
They use one channel, eight channels of one space, four send-receive pairs on one channel, or eight spaces.

*Table 17. Eight parallel operations in one deploy.*

| Case | Outcome | Phlo | Median time in ms (95 % interval) |
|---|---|---|---|
| `concurrency/many_channels/8` | fits | 12,515 | 1.92 (1.90 to 1.94) |
| `concurrency/many_spaces/8` | fits | 29,304 | 2.92 (2.89 to 2.96) |
| `concurrency/mixed_one_channel/8` | fits | 15,535 | 2.26 (2.21 to 2.36) |
| `concurrency/one_channel/8` | fits | 10,442 | 1.63 (1.62 to 1.64) |

### 4.9 Queue order

Each deploy sends n records to one queue and drains them with one persistent receive, one record in each round.
The series runs the counts 1, 10 and 31.

*Table 18. Sends and a FIFO drain in one deploy.*

| Case | Outcome | Phlo | Median time in ms (95 % interval) |
|---|---|---|---|
| `queue_ordering/send_and_drain/1` | fits | 10,456 | 1.16 (1.14 to 1.17) |
| `queue_ordering/send_and_drain/10` | fits | 44,074 | 4.10 (4.09 to 4.15) |
| `queue_ordering/send_and_drain/31` | fits | 122,509 | 11.04 (10.96 to 11.22) |

## 5. Results of the persistence benchmarks

Each case of `rholang/benches/reified_persistence.rs` measures work that a block adds to its evaluation.
The cases of Sections 5.1 and 5.2 time the deploy together with its checkpoint, and the other cases time the work that their sections name.
The cases of Sections 5.1 and 5.2 use the in-memory store of the test chain.
The cases of Sections 5.3 to 5.6 use LMDB stores.

### 5.1 Commit

Each case sends 10 records to one collection, and it times the deploy with its checkpoint.

*Table 19. One deploy of 10 sends with its checkpoint.*

| Case | Outcome | Phlo | Median time in ms (95 % interval) |
|---|---|---|---|
| `commit/bag/10` | fits | 14,765 | 3.71 (3.64 to 3.76) |
| `commit/priorityqueue/10` | fits | 37,016 | 5.03 (5.00 to 5.06) |
| `commit/queue/10` | fits | 36,776 | 4.67 (4.66 to 4.69) |
| `commit/set/10` | fits | 14,765 | 3.75 (3.70 to 3.78) |
| `commit/stack/10` | fits | 36,776 | 4.87 (4.85 to 4.91) |
| `commit/vectordb/10` | fits | 15,555 | 4.92 (4.85 to 4.95) |

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

| Case | Outcome | Phlo | Median time in ms (95 % interval) |
|---|---|---|---|
| `restart_attach/bag/100` | fits | 8,386 | 2.58 (2.56 to 2.60) |
| `restart_attach/bag/100/cold` | fits | as above | 2.74 (2.71 to 2.77) |

The contract checks the cached case only. The cold case plays the same deploy on the same root.
It makes its own picks, so its result and root can differ from those of the cached case.

### 5.4 Merge

Two branches over one base each store 10 records in one bag space on an LMDB store.
The case times the merge of both branches.
A bag has no counter, so the merger accepts both branches, and the contract holds the merged root.

*Table 21. The merge of two branches that both write one bag.*

| Case | Merged root | Median time in ms (95 % interval) |
|---|---|---|
| `merge/bag_two_branches/10` | `baa4d476f675852a…` | 0.76 (0.75 to 0.77) |

### 5.5 Replay

The case plays two blocks on an LMDB store: the creation of a queue space, and 10 sends to it.
It then times the native replay of both blocks, which certifies each play against its journal, as a validator does.

*Table 22. The native replay of two blocks.*

| Case | Phlo of block 2 | Median time in ms (95 % interval) |
|---|---|---|
| `replay/queue_two_blocks/10` | 29,512 | 8.48 (8.39 to 8.55) |

### 5.6 The bytes of a block on LMDB

The `lmdb_commit` case opens an LMDB store, creates a bag space, and plays 10 blocks of 10 sends with a checkpoint after each block.
It records the length and the allocated bytes of each data file after each block.
It then times one block of 10 sends with its checkpoint.

The store has two LMDB environments: the history trie and the cold store of the record lists.
Table 23 sums the data files of both, and it gives the bytes that each environment adds.

*Table 23. The bytes that each block of 10 sends adds to the LMDB store.*

| Block | Length of the data files | Added to the length | Added to the cold store | Added to the history |
|---|---|---|---|---|
| 1 | 122,880 | 49,152 | 40,960 | 8,192 |
| 2 | 172,032 | 49,152 | 45,056 | 4,096 |
| 3 | 237,568 | 65,536 | 65,536 | 0 |
| 4 | 303,104 | 65,536 | 57,344 | 8,192 |
| 5 | 389,120 | 86,016 | 77,824 | 8,192 |
| 6 | 491,520 | 102,400 | 94,208 | 8,192 |
| 7 | 610,304 | 118,784 | 110,592 | 8,192 |
| 8 | 733,184 | 122,880 | 118,784 | 4,096 |
| 9 | 880,640 | 147,456 | 143,360 | 4,096 |
| 10 | 1,028,096 | 147,456 | 147,456 | 0 |

*Table 24. One block of 10 sends with its checkpoint on LMDB.*

| Case | Outcome | Phlo | Median time in ms (95 % interval) |
|---|---|---|---|
| `lmdb_commit/bag/10` | fits | 12,175 | 3.65 (3.54 to 3.70) |

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
| `collection_types/bag_insert_1000` | 1.2 | one more send of a deploy to a bag space | 0.156 | 126,118 | The legacy case pushes an integer into an in-memory vector. A send now runs a space job: routing, locks, a copy of the list, events and the observer calls. |
| `collection_types/queue_insert_1000` | 1.3 | one more send of a deploy to a queue space | 0.247 | 186,987 | As for the bag, and each send also bumps the counter of the collection with three events (M-1). |
| `collection_types/stack_insert_1000` | 1.3 | one more send of a deploy to a stack space | 0.252 | 195,826 | As for the queue. |
| `collection_types/set_insert_1000` | 55.2 | one more send of a deploy to a set space | 0.154 | 2,792 | The legacy case inserts into a hash set. A send now stores a record, and a receive later retires its class. |
| `produce_consume_cycle/produce_only/1000` | 19.6 | one more send of a deploy to a bag space | 0.156 | 7,977 | The legacy space had no events, no metering and no history. A send now makes all three. |
| `produce_consume_cycle/consume_immediate_match/1000` | 76.6 | one more send-receive pair of a deploy on a bag space | 0.539 | 7,038 | The legacy consume matched with the wildcard matcher. A receive now matches its pattern, fires a COMM and commits it. |
| `produce_consume_cycle/produce_multi_channel/1000` | 91.3 | one more send of a deploy, each to its own cell | 0.331 | 3,629 | Each send now reads its own collection and bumps its own counter. |
| `collection_retrieval/bag_find_all` | 184.6 | one receive from a bag space of 1,000 records | 12.910 | 69,920 | The legacy case removes 1,000 items from an in-memory vector, so one removal scans part of it. A receive now copies the whole list of 1,000 records into its staged view first (Section 3.4). |
| `collection_retrieval/queue_find_all` | 0.4 | one receive from a queue space of 1,000 records | 12.940 | 34,873,271 | The legacy queue removed its head in place. A receive now copies the whole list, although it inspects only the head. |
| `collection_retrieval/stack_find_all` | 1.3 | one receive from a stack space of 1,000 records | 13.010 | 9,678,546 | As for the queue. |
| `matcher_comparison/exact_match/1000` | 0.4 | one record of a receive that scans 1,000 records and matches none | 0.012 | 27,811 | The legacy case compares two integers. The receive now matches a Rholang pattern against each record with the matcher, inside a deploy. |
| `vectordb_top_k/top_10/1000` | 117.9 | one record of a query for the best eight records of a VectorDB space of 100 records | 0.088 | 749 | The legacy case ranks 1,000 vectors of 384 floats in memory. The space query ranks exact numbers inside a deploy. |
| `lock_contention_same_channel/produce/8` | 90.5 | one of eight parallel sends to one channel of a bag space | 0.204 | 2,252 | The legacy case writes a shared map from eight threads. The eight space jobs share the footprint key of their space ([I-11](decision-register.md#i-11)), so they run one after another. |
| `lock_contention_different_channels/isolated_channels/8` | 81.9 | one of eight parallel sends to eight channels of a bag space | 0.240 | 2,925 | The legacy case writes eight maps from eight threads. The jobs of different channels of one space also share its footprint key, so they also run one after another. |
| `atomic_ordering/seqcst_fetch_add` | 1.8 | one send and one drain round of a queue space, in a deploy of 10 sends | 0.410 | 224,788 | The legacy case increments an atomic counter. The order of a queue record now comes from the counter record of the collection, with its events. |

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
| `send/bag/10` | 2.00 | 0.43 | 4.61 |
| `send/set/10` | 1.99 | 0.43 | 4.59 |
| `send/queue/10` | 2.95 | 0.43 | 6.80 |
| `send/stack/10` | 3.01 | 0.43 | 6.96 |
| `send/priorityqueue/10` | 3.24 | 0.43 | 7.48 |
| `send/vectordb/10` | 3.06 | 0.43 | 7.07 |
| `send_receive/bag/10` | 5.69 | 1.89 | 3.01 |
| `send_receive/set/10` | 6.02 | 1.89 | 3.18 |
| `send_receive/queue/10` | 7.88 | 1.89 | 4.17 |
| `send_receive/stack/10` | 8.04 | 1.89 | 4.25 |
| `send_receive/priorityqueue/10` | 8.22 | 1.89 | 4.35 |
| `send_receive/vectordb/10` | 9.81 | 1.89 | 5.19 |
| `receive_from_n/bag/10` | 1.02 | 0.32 | 3.17 |
| `receive_from_n/set/10` | 1.12 | 0.32 | 3.47 |
| `receive_from_n/queue/10` | 1.15 | 0.32 | 3.56 |
| `receive_from_n/stack/10` | 1.15 | 0.32 | 3.58 |
| `receive_from_n/priorityqueue/10` | 1.16 | 0.32 | 3.58 |
| `receive_from_n/vectordb/10` | 1.47 | 0.32 | 4.56 |
| `receive_from_n/bag/100` | 1.98 | 0.80 | 2.48 |
| `receive_from_n/set/100` | 2.48 | 0.80 | 3.12 |
| `receive_from_n/queue/100` | 2.09 | 0.80 | 2.63 |
| `receive_from_n/stack/100` | 2.12 | 0.80 | 2.66 |
| `receive_from_n/priorityqueue/100` | 2.15 | 0.80 | 2.70 |
| `receive_from_n/vectordb/100` | 5.32 | 0.80 | 6.69 |
| `receive_from_n/bag/1000` | 12.91 | 5.68 | 2.27 |
| `receive_from_n/set/1000` | 18.50 | 5.68 | 3.26 |
| `receive_from_n/queue/1000` | 12.94 | 5.68 | 2.28 |
| `receive_from_n/stack/1000` | 13.01 | 5.68 | 2.29 |
| `receive_from_n/priorityqueue/1000` | 13.68 | 5.68 | 2.41 |
| `receive_from_n/vectordb/1000` | 57.73 | 5.68 | 10.17 |

The experiments of Table 28 tested the differences that matter in step S8, with 60 replicates of each arm.
Each experiment was opened in pgmcp with its criterion fixed before the measurement.

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

This section keeps the measurement of step S8, because the timed run of 2026-11-10 did not time the examples.

Each example of `examples/reified_rspaces/` runs as one command.
The command is the release binary of the examples harness with `--exact example_NAME`.
One run plays every deploy of the example natively.
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

### 8.1 F1: each space operation loads the whole record list of its channel

Each space operation loads the whole record list of its channel into its staged view (Section 3.4).
A receive that fires reads that list again for the check of its commit (`committed_candidates`, `rspace++/src/rspace/space_store_handle.rs:2875-2911`).

### 8.2 F2: a deploy held fewer space operations than host operations

[Decision 89](decision-register.md#decision-89) retires the host-work limits that bound the operations of one deploy.
Tables 3 and 4 record this finding for the measurement before that decision.

### 8.3 F3: a space deploy takes more time than the host program

A deploy of 10 sends to a bag space takes 2.00 ms, and the host program takes 0.43 ms: 4.6 times as long.
Experiment 421 tested this difference in step S8, with 60 replicates of each arm.
Its verdict was accepted (Welch's t-test: $`t = 115.90`$, $`p < 10^{-6}`$, $`d = 21.16`$, and the difference was 3.09 to 3.19 ms).

A receive from a bag of 1,000 records takes 12.91 ms, against 1.02 ms from 10 records.
Experiment 422 tested this growth in step S8.
Its verdict was accepted (Welch's t-test: $`t = 332.56`$, $`p < 10^{-6}`$, $`d = 60.72`$, and the difference was 24.57 to 24.87 ms).

### 8.4 F4: the persistence costs

After a restart of its LMDB store, the first receive from a bag of 100 records takes 2.74 ms, against 2.58 ms after a reset.
Experiment 423 tested this difference in step S8.
Its verdict was accepted (Welch's t-test: $`t = 11.55`$, $`p < 10^{-6}`$, $`d = 2.11`$, and the difference was 0.14 to 0.20 ms).
The first block of 10 sends to one bag adds 49,152 bytes to the LMDB store.
Block 10 adds 147,456 bytes (Table 23).
The history stores the record list of a channel as one value, so each block that changes the channel stores the whole list again (`rspace++/src/rspace/history/history_repository_impl.rs:267-278`).

### 8.5 Candidate optimizations

This document adopts no optimization ([decision 85](decision-register.md#decision-85)).
Each candidate below needs its own hypothesis, its own design pass and a decision of the user.

*Table 30. Candidate optimizations for the findings.*

| Candidate | Change | Hypothesis to test | Impact |
|---|---|---|---|
| O1: a copy-free staged view | The staged view keeps the snapshot of the hot store (`data_view` of the native store, `rspace++/src/rspace/hot_store/native_store.rs:337-366`) and the changes of its own transaction. The check of the commit reads a snapshot too. Parked receives stay shared pointers, as the hot store holds them. | A send to a bag, a set, a queue or a stack no longer copies the records of its channel. A receive keeps the work of its discipline for each record, such as the head scan of a queue. | It changes no charge, because [decision 89](decision-register.md#decision-89) meters no read or copy. |
| O2: one channel for each record of an ordered collection | A queue, a stack or a priority queue stores each record on its own channel, keyed by its order, so an operation loads only the head. The plan names this layout as a later optimization of its risk table. | The cost of a queue operation does not grow with the queue. | It changes the stored layout of ordered collections and the counter rules of [M-1](decision-register.md#m-1), with their models. |
| O3: one decode of the config record in each deploy | A space job decodes the config record of its space once in each deploy and keeps the decoded value. | The time of a send to a small collection falls by the share of the config decode (Section 3.5), and no charge changes. | No change of the charges, the events or the state. |

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
The check of the contract fails a run when a field of the class of the case differs from the contract.

## References

<a id="ref-efr79"></a>[Efr79] B. Efron, "Bootstrap methods: another look at the jackknife," *The Annals of Statistics*, vol. 7, no. 1, pp. 1–26, 1979. [doi:10.1214/aos/1176344552](https://doi.org/10.1214/aos/1176344552)

<a id="ref-mdh09"></a>[MDH09] T. Mytkowicz, A. Diwan, M. Hauswirth, and P. F. Sweeney, "Producing wrong data without doing anything obviously wrong!," in *Proceedings of the 14th International Conference on Architectural Support for Programming Languages and Operating Systems (ASPLOS XIV)*, ACM, 2009, pp. 265–276. [doi:10.1145/1508244.1508275](https://doi.org/10.1145/1508244.1508275)

<a id="ref-wel47"></a>[Wel47] B. L. Welch, "The generalization of 'Student's' problem when several different population variances are involved," *Biometrika*, vol. 34, no. 1–2, pp. 28–35, 1947. [doi:10.1093/biomet/34.1-2.28](https://doi.org/10.1093/biomet/34.1-2.28)

<a id="ref-crit"></a>[CRIT] B. Heisler and the Criterion.rs authors, "Criterion.rs," version 0.5.1, a statistics-driven benchmarking library for Rust. [docs.rs/criterion/0.5.1](https://docs.rs/criterion/0.5.1/criterion/)

<a id="ref-hyp"></a>[HYP] D. Peter, "hyperfine," version 2.0.0, a command-line benchmarking tool. [github.com/sharkdp/hyperfine](https://github.com/sharkdp/hyperfine)

<a id="ref-uprof"></a>[UPROF] Advanced Micro Devices, "AMD uProf," version 5.3.521.0, a performance analysis tool for AMD processors. [amd.com/en/developer/uprof.html](https://www.amd.com/en/developer/uprof.html)

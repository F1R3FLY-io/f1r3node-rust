# Equal-contract LMDB versus durable DAG comparison protocol

This is the preregistration, the measurement and analysis plan fixed before any
data are collected, written by leaf `rspace-dag-required-performance-control`
(7949) of pgmcp, the campaign's tracker and experiment registry; a leaf is a
tracker task without subtasks. Its original text, revision 0, was amended once,
before any sample, by leaf
`rspace-lmdb-dag-protocol-observational-witness` (14312; see the
[amendment record](#amendment-record)). Task 7950 runs the comparison; task
7951 interprets it.

The comparison measures **performance only**. It compares two backends of
RSpace, the tuple space in which the node stores data and waiting receives per
channel. The control is the real `RSpace` over its current backend: LMDB
(Lightning Memory-Mapped Database, a transactional key-value store) holding a
RadixHistory (RSpace's radix tree of recorded states). The treatment is the
real `RSpace` with the durable PathMap backend selected: a content-addressed
DAG (directed acyclic graph) of immutable state objects, each named by the
digest of its bytes, whose live state is held in PathMap, the byte-trie library
the backend is built on. LMDB is the control for performance, not a semantic
reference. Correctness is judged against the Rho calculus, a reflective
higher-order process calculus in which processes communicate over names
([Meredith and Radestock 2005](#references)), as the interpreter of Rholang,
the node's contract language, realises it; nondeterminism is part of that
semantics. Both arms, the control and the treatment, run the same operation
list, the seeded sequence of operations that the harness (the benchmark program
driving both arms) issues, under the same durability, retention, cache and
concurrency contract. Optional features are outside this comparison: ACT (Arena
Compact Tree, PathMap's compact trie snapshot format) snapshots, cache overlays
(caches layered over the stored trie), interning (storing each distinct value
once and referring to it by an identifier), and alternative branch geometries
and alternate tries (other trie node layouts and trie implementations). pgmcp
experiment 377 locks the primary observational hypothesis before any 7950
samples. Its engine does not require samples for an observational verdict; the
51 primary pairs (a pair is one control run and one treatment run of the same
workload), the secondary pairs, the raw artifacts and the witnesses below
remain mandatory for the campaign verdict.

## Terms and notation

Every term below is used in the rest of this document with exactly this
meaning. Mathematical symbols are defined where they are introduced. The tables
are read as one glossary: a row may use a term that another row of any of these
tables defines.

**Comparison terms**

| Term | Meaning |
| --- | --- |
| arm | One of the two backends in a paired measurement, always driven through the real `RSpace`: the **control arm** (LMDB backend) or the **treatment arm** (durable PathMap backend selected). |
| owner, owner entrypoint | The *owner* of an arm is the code that owns its persistence: it constructs `RSpace` over a backend and drives its checkpoints. The *owner entrypoint* is the function a run calls to construct it. The *campaign owner*, by contrast, is the person who directs the campaign and rules on its open questions. |
| coordinator | The agent that assigns the campaign's leaves and decides their technical design within the campaign owner's rulings. Its decisions are not rulings of the campaign owner. |
| real-owner scope, component-only scope | A *real-owner* cell drives an arm's owner end to end; a *component-only* cell exercises one storage component in isolation. |
| pair | One control run and one treatment run of the same cell, executed back to back in a randomized order, AB (control first) or BA (treatment first). |
| lane, axis, level | A *lane* is a family of cells; an *axis* is a factor the lane varies and a *level* is one value of an axis. Lanes vary one factor at a time around the primary cell ([Required matrix and measured quantities](#required-matrix-and-measured-quantities)). |
| cell | One workload configuration that is measured: a lane, one level of each varied axis, and a per-cell manifest frozen before timing (seed, operation list, settings). |
| primary cell | The single cell whose throughput ratio decides the improvement claim (`owner-primary-65536`). |
| timed cell | A cell that is sampled in pairs and timed; its ratios enter the decision rule. |
| secondary cell | Every timed cell other than the primary cell; sampled with 30 pairs. |
| untimed cell | A cell that each arm runs once to check witnesses; it is never timed and never enters a ratio. |
| cold run | A run started in a fresh process on fresh directories, with no warmed cache. |
| critical sentinel | A cell whose throughput, latency and footprint ratios must not regress materially. The six are listed, under the labels revision 0 gave them, in [Paired sampling and decision rule](#paired-sampling-and-decision-rule), which also fixes what the first label denotes. |
| ineligible cell | A cell that cannot be measured under the equal contract, or whose run instance fails preflight. It is reported as ineligible, never as a zero-duration observation and never dropped from a reported denominator. |
| checkpoint, reset | A *checkpoint* is the RSpace operation that records the current contents as a state and returns its root; a *reset* makes the contents those of an earlier recorded state. |
| operation list | The seeded sequence the harness feeds an arm: produce and consume operations with their payloads, checkpoints, and resets. A reset names a checkpoint position, never a root value. |
| checkpoint position | The index of a checkpoint operation in the operation list. |
| root | The hash an arm assigns to the state it records at a checkpoint. Each backend computes roots from its own encoding, so a root is meaningful only inside the arm that produced it. |
| retained root, retained state | A recorded root that the retention policy keeps readable, and the state it names. |
| keep-all retention | A retention policy that never drops a recorded state, so every stored object stays reachable from a retained root. |
| reader pin | A reader holding an old root open so that it stays readable while the reader uses it. |
| acknowledged durable batch | One successful checkpoint whose mutations survive a process restart ([Equal contract and observations](#equal-contract-and-observations)). |
| kill/reopen durability witness | The arm's process is killed after an acknowledged checkpoint and the store is reopened; the arm must recover its own last acknowledged root and the contents recorded there. |
| witness | A check that a pair or a cell must pass before its timings count: the work-equivalence witnesses W1, W2 and W3 and each arm's kill/reopen durability witness. |
| W1, W2, W3 | The work-equivalence witnesses: W1, both arms ran the same operation list; W2, each arm's replay and export/import reproduce its own results, contents and root; W3, in timed cells, the arms' contents digests are equal at every checkpoint ([Pair validity](#pair-validity-the-work-equivalence-witnesses)). |
| run instance | The completed copy of the `run_instance_template` of the [control manifest](lmdb-dag-equal-contract-control-manifest.json) for one immutable checkout; the preflight mode of the [manifest checker](../../../scripts/bench/check-lmdb-dag-control-manifest.py) checks it before any pair is collected ([Machine-readable control freeze](#machine-readable-control-freeze)). |
| receipt | The SHA-256 of a witness's retained report, recorded in a run instance. |
| state sync | Transfer of a recorded state to another store by exporting its stored objects in pages and importing them. |
| bisimilarity | The largest relation between the states of two transition systems under which, whenever two states are related, every step of either can be matched by a step of the other with the same label, ending again in related states ([Sangiorgi 2009](#references)). |

**Tuple-space terms**

| Term | Meaning |
| --- | --- |
| channel | A name on which data are sent and receives wait. A receive on several channels at once (a join) waits on an **ordered channel list**. |
| datum | A sent value with its persistence flag (a persistent datum is not removed when it is consumed). |
| waiting continuation | A resting receive: its patterns (which data it accepts), its continuation (the code to run), its persistence flag and its peek positions (peeked data are read without being removed). |
| produce, consume | The RSpace operations that send a datum on a channel and that install a receive on an ordered channel list. |
| multiset, singleton | A *multiset* is a collection in which order does not matter and an element may occur more than once: formally, a finite map from its elements to their multiplicities, which are positive whole numbers. The *multiset sum* of two multisets adds the multiplicities. A *singleton* is a set with exactly one member. The critical-sentinel label singleton/4,096 is a name, not this notion ([Paired sampling and decision rule](#paired-sampling-and-decision-rule)). |
| contents | The data per channel and the waiting continuations per ordered channel list that a tuple space holds ([Contents and the contents digest](#contents-and-the-contents-digest)). |
| COMM | One communication step of the Rho calculus: a send meets a matching receive. In RSpace, a produce or consume that completes a match. |
| enabled set | For given contents and an arriving produce or consume, the set of COMMs that operation can complete. |
| outcome set | The set of contents that an operation list can reach under every permitted choice. |
| choice-free, choice-bearing | A cell is *choice-free* when no step enables more than one COMM, so that its outcome set has one member ([Choice-free timed cells](#choice-free-timed-cells)), and *choice-bearing* when some step enables more than one. |
| worker, schedule | A *worker* is one thread of the harness that applies its own operation list to an arm. The *schedule* is the order in which the operations of different workers reach the space; it is itself nondeterministic. |
| barrier, segment | A *barrier* is a point at which all workers are *joined*: each has applied a prescribed prefix of its own list and none has gone further. In a cell with several workers, checkpoints and resets are applied only at barriers. A worker's *segment* is the portion of its list between two consecutive barriers. |
| part, channel-disjoint workers | A worker's *part* is the set of channels its operations may name. Workers are *channel-disjoint* when their parts are pairwise disjoint and every receive waits on a channel list inside one part ([Choice-free timed cells](#choice-free-timed-cells), Lemma 2). |
| contents digest | A canonical digest of the contents, computed by the harness outside both backends ([Contents and the contents digest](#contents-and-the-contents-digest)). |
| event log, replay | The *event log* is RSpace's record of the produce, consume and COMM events of a run; a *replay* re-executes an event log so that each recorded COMM is performed again. |
| hot store | RSpace's in-memory layer holding the changes made since the last checkpoint. |
| history, cold objects | In the LMDB arm, the *history* is the radix tree of recorded states, keyed per channel and per channel list; the *cold objects* are the persisted data, continuation and join lists that its leaves name. |
| root membership, current root | The set of roots the LMDB arm's authority knows, and the one it treats as current. |
| executable reference | The small reference tuple space delivered by leaf `rspace-executable-reference-tuplespace` (14306). It computes enabled sets by exhaustive enumeration. It does not exist at this amendment. |

**Storage terms**

| Term | Meaning |
| --- | --- |
| map size, `max_dbs`, readers | LMDB settings: the size of the environment's memory map, fixed when a path is first opened; the maximum number of named databases; the maximum number of concurrent read transactions. |
| relaxed sync mode | An LMDB environment flag that skips or weakens the flush to stable storage at commit (`MDB_NOSYNC`, `MDB_NOMETASYNC` or `MDB_MAPASYNC`). |
| canonical object, canonical bytes | A state object of the durable backend in its canonical encoding; the digest of these bytes names the object. |
| closure | The set of objects reachable from a root. |
| pack | An append-only log file of the durable backend. Each transaction is a prepare record, holding the mutation and each new canonical object once, followed by a separately synced commit record ([`wal/mod.rs`](../../../rspace++/src/rspace/persistence/wal/mod.rs)). |
| WAL | Write-ahead log: the prepare and commit records written to packs before a state is published. |
| index | A rebuildable map from object digest to location in the packs: a physical accelerator, never semantic authority ([`index.rs`](../../../rspace++/src/rspace/persistence/durable/index.rs)). |
| catalog | The durable record that names the current verified root and where its objects are stored; publication updates one of its two slots by compare-and-swap ([`publication.rs`](../../../rspace++/src/rspace/persistence/durable/publication.rs)). |
| generation, publication | A *generation* is one self-contained durable state, with its packs and index. *Publication* makes a successor generation durable and then switches the catalog to it. |
| incremental successor | A successor generation published from its predecessor by adding only the objects that the checkpoint creates. |
| admission | Accepting new objects, or recovered state, into the store after validating them. |
| compaction | Rewriting the retained roots' shared closure into a new pack generation without changing their roots ([`retention.rs`](../../../rspace++/src/rspace/persistence/retention.rs)). |
| reference activation, deactivation | Counting an object into, or out of, the retained set's reference ledger when a root that reaches it is added or dropped. |
| old-closure work, metadata work | Work spent on objects of the predecessor's closure, and on retention metadata, while publishing a checkpoint. |
| pack or WAL rotation | Closing the current pack or log file and continuing in a new one. |
| clean restart, torn restart | A restart after an orderly shutdown; a restart after a crash in the middle of a write, which leaves a partially written record. |
| index repair | Rebuilding a missing or corrupt index from the committed records in the packs. |
| recovery point, replay tail | The last acknowledged state that recovery restores; the portion of the log after the last published generation that recovery must apply again. |
| delta, sharing | The number of paths a checkpoint changes; the percentage of objects that the retained roots have in common. |
| dedup | Storing an object once although several states contain it. |
| derived cache | A cache of values computed from the stored state, such as a match accelerator, which can be rebuilt from that state. |
| candidate visits, node visits | The number of stored data or continuations a match examines, and the number of trie nodes it visits. |
| write amplification | Total physical bytes written divided by newly admitted canonical bytes. |
| logical length, allocated bytes, inode | A file's reported size; the disk blocks actually allocated to it; the file-system record of one file or directory. |
| sidecar | The fixed-width payload file that the historical harness writes beside each ACT image. |
| GC | Garbage collection: reclaiming stored objects that no retained state reaches. |

**Acronyms, tools and system terms**

| Term | Meaning |
| --- | --- |
| CCD | Core complex die: one chiplet of the processor, with its own cores and level-3 cache. |
| CPU, CPI, IPC | Central processing unit; cycles per instruction; instructions per cycle. |
| Criterion | The Rust benchmarking library used by the historical harness. |
| ext4, tmpfs | The Linux fourth extended file system; a memory-backed file system. |
| FFI | Foreign function interface: the C-callable entrypoints of the RSpace library (for example `space_new`). |
| fsync | The system call that forces a file's written data to stable storage. |
| I/O, syscall | Input/output; system call. |
| major fault, minor fault | A page fault that needs disk input, and one that does not. |
| p50, p95, p99 | The 50th, 95th and 99th percentiles of a latency distribution. |
| RSS | Resident set size: the physical memory a process occupies. |
| SHA-256 | The 256-bit Secure Hash Algorithm ([FIPS 180-4](#references)). |
| UUID | Universally unique identifier. |
| sealed digest | The digest that the pgmcp registry records when it seals an experiment's data against later change. |
| governor, scaling driver, boost state | The Linux CPU-frequency policy (`performance` holds the maximum frequency), the kernel driver that applies it, and whether the processor's opportunistic frequency boost is enabled. |
| `MemorySwapMax=0` | A `systemd-run` property that forbids the run to use swap. |
| `perf_event_paranoid` | The Linux setting that limits unprivileged access to performance counters. |
| `heaptrack`, AMD uProf, `perf stat`, `mpstat`, `systemd-run` | A heap-allocation profiler; AMD's sampling profiler for its processors, whose command-line collector is `AMDuProfCLI` and whose system-wide counter monitor is `AMDuProfPcm`; the Linux hardware-counter tool; a per-core utilisation reporter; a launcher that runs a command under resource limits. |
| bootstrap, pair-preserving | Resampling the observations with replacement to estimate the distribution of a statistic; a pair-preserving bootstrap draws whole pairs, so each ratio keeps its own control and treatment. |
| two-sided 99% interval | A confidence interval at level 99% that bounds the estimate from both sides; the decision rule uses its lower and upper bounds. |

## Amendment record

### Amendment 1, 2026-11-11: performance only, work-equivalence witnesses

**When.** Made by leaf `rspace-lmdb-dag-protocol-observational-witness` (14312)
under task 7950 **before any sample was taken**. Evidence: the record of
experiment 377 exported from the pgmcp registry for this amendment has empty
`measurement_runs`, `paired_binary` and `decisions` lists, status `open` with
decision validity `pending`, and a context stating that no samples have been
taken; the [control manifest](lmdb-dag-equal-contract-control-manifest.json)
is still in state `preregistered_without_samples`. The registry protocol of
experiment 377 is amended with the same content through pgmcp's
`experiment_amend_protocol`.

**Why.** The campaign owner ruled on 2026-11-11 that the PathMap backend
replaces the LMDB backend of `RSpace` rather than re-implementing LMDB; that
what must be bisimilar is the node and its Rholang interpreter running on the
new backend, judged against the Rho calculus, not PathMap against LMDB; and that
bisimilarity must take nondeterminism into account. Revision 0 of this
protocol required the two arms to produce equal roots, equal orders and
byte-equal state-sync pages, which makes LMDB a semantic reference. Under the
ruling LMDB is only the control arm of a performance comparison, so the
pair-validity rule is replaced, and the cells that have no meaning under the
ruling or under keep-all retention in both arms are marked not applicable
(Table 5). The method for timed cells with eight workers, the exclusion of two
orders of one channel set from untimed cells as well, the denotation of the
sentinel label singleton/4,096 and the rule for the historical harness pin are
technical decisions of the coordinator within these rulings.

**What changed.** Thresholds, the primary cell and the pair counts are unchanged.

| Item | Revision 0 | Amendment 1 |
| --- | --- | --- |
| Role of LMDB | Control arm and semantic yardstick | Control arm for performance only |
| Pair validity | Equal canonical roots (or a documented mapping from legacy roots); equal contents and order of the DataBag, ContinuationMap and JoinBag (data per channel, continuations per channel list, joins per channel); equal duplicate and persistent-item behaviour; equal produce/consume/COMM results and diagnostics; equal known and current roots, reset/replay, cold export/import and state-sync bytes, and restart selection (the root a restarted store selects) | Same work modulo nondeterminism: witnesses W1, W2 and W3. No rule compares roots, orders, chosen COMMs or state-sync bytes across arms |
| Contract items | Identical command trace; identical canonical object stream and batch size | Replaced by W1 (same operation list, whose checkpoint positions fix the batch sizes); the arms' stored objects differ by design |
| Timed cells | Any trace of the fixed corpus | Choice-free only, verified before sampling by the executable reference; traces with two orders of one channel set excluded |
| Timed cells with eight workers | One versus eight writer/reader workers, with no rule for the nondeterministic schedule | Channel-disjoint parts checked outside both backends, one reference run per worker, and checkpoints and resets only at joined barriers; together they give one outcome at every checkpoint (Lemma 2) |
| Untimed cells | None | One choice-bearing cell per lane, checking W1, W2 and COMM membership; traces with two orders of one channel set excluded, as from timed cells |
| Treatment entrypoint | The non-default real owner after task 7756's cutover, constructed at the time only through `DurableAuthorityViewV1::into_rspace_authority` | The real `RSpace` with the durable backend selected |
| Component lane | Run when byte-identical | Not applicable |
| Retained-root GC cells, GC outputs and GC sentinel; retention-eviction cells | Measured | Not applicable: keep-all retention in both arms, and LMDB reclaims nothing |
| Lanes | Six | Seven: a history-scaling lane is added |
| Publication and lifecycle lane | No reset cell | A reset-then-checkpoint cell is added |
| Run-instance receipts | `semantic_witness_sha256` | `work_equivalence_witness_sha256` and `choice_freedom_receipt_sha256` |
| Critical sentinel singleton/4,096 | Named, with the word "singleton" defined nowhere | The owner-primary cell at 4,096 objects, the smallest state of that lane, fixed before any sample; the other five sentinels are unchanged |
| Historical harness pin (`harness_last_edit`) | Had to equal the last commit that touched the harness at the checked-out revision | The harness version that produced the retained transcript: an ancestor of the checked-out revision and the last commit that touched the harness as of itself, so the planned repair of the harness by leaf `rspace-act-lmdb-harness-equal-durable-work` (14340) leaves it valid |

## Machine-readable control freeze

The [control manifest](lmdb-dag-equal-contract-control-manifest.json) freezes the
historical evidence role, LMDB owner source pin and settings, treatment entrypoint,
the work-equivalence and durability witnesses, the timed-cell policy, the
not-applicable markings, the paired decision rule, the primary cell, and seven
matrix lanes with their untimed choice-bearing cells. It is a preregistration,
not a result. Its `run_instance_template` contains deliberately unfilled fields;
task 7950 must copy and complete those fields for a clean immutable checkout
before collecting a pair. The historical ACT/LMDB component transcript has role
`historical_component_context_only` and can never fill a paired real-owner
control observation.

The standard-library-only [manifest checker](../../../scripts/bench/check-lmdb-dag-control-manifest.py)
has a static mode and a run-instance preflight. Run `python3
scripts/bench/check-lmdb-dag-control-manifest.py preregistration` and `python3
scripts/bench/check-lmdb-dag-control-manifest.py self-test` from the root of a
git checkout of the repository to check the frozen artifact hashes, owner pin,
exact primary and secondary sampling contract, lane coverage, the
work-equivalence witnesses, including the prohibition of cross-arm comparisons
of roots, orders, chosen COMMs and state-sync bytes, the choice-free marking of
every timed cell, the method of the eight-worker timed cells and their joined
barriers, the per-lane choice-bearing cells and their exclusion of two orders of
one channel set, the not-applicable markings and the
[negative fixtures](lmdb-dag-equal-contract-negative-fixtures.json). The static
mode asks git two questions about the historical harness pin, the harness
version that produced the retained transcript: whether it is an ancestor of the
checked-out revision, and whether it is the last commit that touched the harness
as of itself. A pin that fails either is refused, and a later edit of the
harness does not invalidate a correct pin. The static mode also resolves the
control pin. These commands do not run a benchmark. A run-instance file copied
from the template is checked
with `python3 scripts/bench/check-lmdb-dag-control-manifest.py preflight
--instance PATH`. Preflight refuses missing owner-adapter and witness receipts,
a missing choice-freedom receipt from the executable reference, any dirty
source tree, a source revision that changes the pinned LMDB owner, or any
historical component data promoted to a real-owner control. A failed preflight
is an ineligible cell, never a zero-duration observation. The manifest hashes
this protocol and the retained historical transcript; this document does not
embed the manifest's hash, so the references are acyclic. A protocol edit
requires an explicit manifest hash refresh before further sampling.

## Existing evidence and source freeze

| Evidence | Frozen coordinate | What it does and does not establish |
| --- | --- | --- |
| [Historical ACT/LMDB harness](../../../rspace++/benches/rspace_act_lmdb.rs) and [Appendix J](../stack-safety/stack-safety-report-2026-07-29.md#appendix-j--bounded-actlmdb-persistence-comparison-2026-09-05) | Historical experiment 327 at `87a9584b3`; the harness version that produced the retained transcript is `0c1d01cebcab832b1ba01191e27f00aa7f268f48`, the last commit that touched the harness as of `87a9584b3` | Component fixture: shared-prefix byte keys, 128-byte payloads, 256/4,096/16,384 entries. ACT dump plus sidecar versus one LMDB batched put; warm ACT lookup versus LMDB batched get. `unique_path` uses `std::env::temp_dir()`, which is a tmpfs on this workstation, so this run cannot qualify ext4 durability or physical block I/O. The ACT sidecar is written without an equal LMDB transaction/sync contract. No durable DAG, retained roots, recovery or RSpace owner. |
| [Retained experiment 327 extract](../stack-safety/measurements/rspace-act-lmdb-experiment-327-transcript-2026-09-29.json) | SHA-256 `555566dc9cfced5b022da475588d970e664449b831406d2163e8478c3531d453` | Eighteen size-labeled summary scalars, two run UUIDs and artifact IDs 409–412. Current pgmcp `experiment_get(327)` returns `experiment not found`; the original Criterion per-cell samples, sealed digest, decision and observed compiler versions are unavailable. The disk-byte submissions also warned of a unit mismatch. Reuse only as historical fixture context, never as a paired control for 7950. |
| [Current LMDB authority](../../../rspace++/src/rspace/state/rspace_state_authority.rs), [environment cache](../../../rspace++/src/rspace/shared/env_cache.rs), [store](../../../shared/src/rust/store/lmdb_key_value_store.rs) and [manager](../../../rspace++/src/rspace/shared/rspace_store_manager.rs) | Initial source pin `5d19c439215ff26bd28c42beac418da56a6b8dd6` (2026-10-01 audit) | `commit_local_checkpoint` writes history, cold objects, root membership and current root in one LMDB write transaction. The environment builder sets map size, `max_dbs` and 2,048 readers without a no-sync flag in this source; the first open fixes map size per path. Ordinary `KeyValueStore::put` commits one transaction per batch. These are source observations, not new performance measurements or a power-loss proof. |

The initial source pin freezes the **control implementation for protocol review**.
It is not an executable baseline measurement. Before 7950, use a clean,
immutable checkout of the treatment revision for both selectable backends;
record its full commit, dependency lockfile, compiler, binary hash, and
backend selection. If that revision changes LMDB semantics, durability,
retention, or the operation list from the initial pin, supersede this pin with
an explicit new control revision **before** samples are taken. Never mix the
historical 327 component summaries with current owner samples. The present
workspace contains other agents' staged changes and is not a valid benchmark
source tree without isolation and source hashing.

The existing [FFI `space_new`](../../../rspace++/libs/rspace_rhotypes/src/lib.rs)
path passes `GB` (1,073,741,824 bytes) to `mk_rspace_store_manager`; that
manager's `LmdbEnvConfig::new` defaults to 20 databases, and the shared
environment cache sets 2,048 readers. An alternate
`get_or_create_rspace_store` path passes 10,000 databases but is
not a substitute for the FFI path in an FFI-owner comparison. The runner must
name the actual owner entrypoint it exercises and freeze these resolved
settings from that entrypoint before the first pair; mixing entrypoints makes
the cell ineligible.
The current FFI constructor reaches `RSpace::create` and
`RSpaceStateAuthority` through that LMDB store manager. The **treatment
entrypoint** is the real `RSpace` with the durable backend selected. At the base
of this amendment no such selection exists: the durable authority is built only
through the crate-private
[`DurableAuthorityViewV1::into_rspace_authority`](../../../rspace++/src/rspace/persistence/durable/mod.rs)
path, which tests call directly and which yields an authority, not an `RSpace`.
Leaf `rspace-select-durable-backend` (12421, under task 7931, which wires the
selected authority through the RSpace operation gate as part of task 7756's
durable cutover) adds the per-store-prefix backend selection; until it lands
the treatment arm cannot run. Task 7950 needs one command adapter, the harness
component that issues the operation list's calls to an arm's `RSpace`, that
drives both arms with the same operation list; its commit is the run instance's
`real_owner_adapter_commit`. A run of the authority-only path, or an FFI LMDB
run paired with an isolated DAG component fixture, is ineligible as an owner
comparison.

## Equal contract and observations

An **acknowledged durable batch** is one successful RSpace checkpoint whose
mutations are recoverable after process restart under the selected backend
policy. Compare LMDB's successful owner transaction commit with the
DAG's successful WAL/pack/catalog publication and required synchronization.
A returned hot-store mutation, buffered write, in-memory PathMap mutation or
component-only ACT dump is not a durable batch. The primary throughput
numerator counts successful checkpoint batches, and elapsed time includes
their mutation preparation and every synchronization. Each primary replicate
runs exactly 4,096 mutations and 64 checkpoints after the same untimed
initial-state construction; report construction time separately. No failed,
refused or unfinished batch enters the numerator.

Freeze one manifest per comparison cell before timing: source commit and
binary hash; filesystem/device and mount options; LMDB map size, database
layout, environment flags, reader limit and transaction boundaries; DAG pack
rotation and checkpoint policy; exact fsync/barrier sequence; retention policy
(keep-all in both arms), retained-state count, root-sharing and reader pins;
hot-store and derived-cache enablement and capacity; worker count and CPU
affinity; payload distribution and deterministic seed; the operation list and
its SHA-256; initial object count, unchanged state, changed paths, new canonical
bytes, and objects whose last reference is removed. Keep the same logical operation
order, checkpoint frequency, retained-root policy, cache state and concurrency
in both arms. Explicitly verify that LMDB uses no relaxed sync mode and that
each arm passes the kill/reopen durability witness on its own backend before
any speed comparison: after a kill and a reopen the arm recovers its own last
acknowledged root and the contents it recorded there. If one arm cannot offer
the same durability/retention outcome, mark that cell **ineligible**, without
assigning it a favorable zero or dropping it from a reported denominator.

### Pair validity: the work-equivalence witnesses

The two arms implement the same tuple space with different storage. Under the
Rho calculus a send or a receive that can meet several partners may meet any
one of them; this nondeterminism is part of the semantics, so two correct
backends may reach different, equally valid contents from the same operations,
and each stores and hashes its contents in its own format. A pair is therefore
**valid when both arms did the same work modulo nondeterminism**, which three
witnesses establish.

**Table 1: the work-equivalence witnesses**

| Witness | What is compared | Scope | Passes when |
| --- | --- | --- | --- |
| W1, same operation list | Operations, payloads and checkpoint positions | Across arms | Both arms executed the same operation list (equal SHA-256 of the executed list); every reset names a checkpoint position, never a root value. |
| W2, own validity | A replay of the arm's own event log on its own backend; an export of the arm's state imported into a fresh store of the same backend | Within each arm | The replay gives the arm's own results, contents and root, and the import gives the arm's own root. |
| W3, choice-free contents | The contents digest at every checkpoint position | Across arms, timed cells only | The two arms' digests are equal at every checkpoint position. |

**Never compared across arms**, and why:

- **Roots.** Each backend hashes its own encoding of a state; two correct
  backends assign different roots to the same contents. A reset therefore names
  a checkpoint position, which each arm maps to its own root.
- **Orders.** Contents are multisets. The order in which a backend stores or
  iterates data, continuations or joins has no meaning in the Rho calculus.
- **Chosen COMMs.** When several COMMs are enabled the calculus may take any one,
  so two correct arms may choose differently. In a choice-free cell the choice
  is forced and W3 already covers its effect.
- **State-sync bytes.** Exported pages are each backend's own storage format.

A mismatch in W1, W2 or W3 is a correctness failure of the pair, not an outlier.
A difference in a quantity that is never compared across arms is not a failure.

**Diagram 1: what each witness compares in one pair**

```text
                   ┌─────────────────────────────────────┐
                   │ operation list, seeded and frozen   │
                   │ before timing; each reset names a   │
                   │ checkpoint position, never a root   │
                   │ value                               │
                   └──────────────────┬──────────────────┘
                 ┌────────────────────┴─────────────────────┐
                 ▼                                          ▼
   ┌───────────────────────────┐              ┌───────────────────────────┐
   │ control arm               │      W1      │ treatment arm             │
   │ RSpace with the LMDB      │◀────────────▶│ RSpace with the durable   │
   │ backend                   │  same list   │ PathMap backend selected  │
   └─────────────┬─────────────┘              └─────────────┬─────────────┘
                 │                                          │
                 ▼                                          ▼
   ┌───────────────────────────┐              ┌───────────────────────────┐
   │ W2 inside the arm: replay │              │ W2 inside the arm: replay │
   │ of its own event log and  │              │ of its own event log and  │
   │ export then import give   │              │ export then import give   │
   │ its own results, contents │              │ its own results, contents │
   │ and its own root          │              │ and its own root          │
   └─────────────┬─────────────┘              └─────────────┬─────────────┘
                 │                                          │
                 ▼                                          ▼
   ┌───────────────────────────┐      W3      ┌───────────────────────────┐
   │ contents digest at every  │◀────────────▶│ contents digest at every  │
   │ checkpoint, computed by   │ equal (timed │ checkpoint, computed by   │
   │ the harness               │ cells only)  │ the harness               │
   └───────────────────────────┘              └───────────────────────────┘

   Legend: ◀──▶ compared across arms.  W2 never leaves its arm.
   Never compared across arms: roots · orders · chosen COMMs · state-sync bytes.
```

**When the witnesses run.** Witness work never runs inside a timed interval.
Each timed run records, per checkpoint position, the root its own arm returned.
After the run's clock stops, the harness resets the arm to each recorded root
(keep-all retention keeps every one readable) and computes the contents digest
there (W3); it replays the arm's own event log and exports and imports the
arm's final state on a fresh store of the same backend (W2); and it compares the
SHA-256 of the operation list each arm executed (W1).

### Contents and the contents digest

Let $`\mathcal{C}`$ be the set of channels and $`\mathcal{C}^{+}`$ the set of
non-empty finite sequences of channels, the ordered channel lists. Let
$`\mathcal{V}`$ be the set of data (a value with its persistence flag) and
$`\mathcal{W}`$ the set of waiting continuations (patterns, continuation,
persistence flag and peek positions). For a set $`X`$, $`\mathcal{M}(X)`$ is the
set of finite multisets over $`X`$. The contents of an arm are a pair

```math
\sigma = (D, K), \qquad D : \mathcal{C} \to \mathcal{M}(\mathcal{V}), \qquad K : \mathcal{C}^{+} \to \mathcal{M}(\mathcal{W}),
```

in which only finitely many channels and channel lists map to a non-empty
multiset. This is the tuple-space portion of the configurations that leaf
`rspace-rho-bisimilarity-statement` (14304) specifies. Joins, the per-channel index
of the channel lists that mention a channel, do not belong to the contents: they
are an index over $`K`$. Provenance that an arm stores with an item, such as the
event that produced a datum, does not belong to the contents either.

The encoding $`\operatorname{enc}`$ maps a channel, a datum or a waiting
continuation to its canonical byte string: the encoder the operation list uses
for payloads, applied to the values an arm returns through `RSpace`'s reading
interface (for example `to_map`). The harness must use an injective encoder:
different values have different encodings. For an ordered channel list
$`(c_1, \ldots, c_r)`$ of $`r`$ channels,
$`\operatorname{enc}(c_1, \ldots, c_r)`$ is the concatenation of
$`\operatorname{enc}(c_1), \ldots, \operatorname{enc}(c_r)`$, each preceded by
its length, so for channels $`a \neq b`$ the lists $`(a, b)`$ and $`(b, a)`$
have different encodings: $`\operatorname{enc}(a, b) \neq \operatorname{enc}(b, a)`$.
The contents digest $`\Delta(\sigma)`$ of contents $`\sigma`$ is computed as
follows. In the pseudocode `←` is assignment, `▸` begins a comment, `‖` is
concatenation of byte strings, and a record is a triple (tag, key, members)
whose key is the encoded channel or channel list.

```text
contents_digest(D, K):
    records ← empty list
    ▸ data: one record per channel that holds data
    for each channel c with D(c) non-empty:
        members ← [ enc(v) for each occurrence v in D(c) ]   ▸ repeated by multiplicity
        sort members bytewise
        append (tag 0, enc(c), members) to records
    ▸ waiting continuations: one record per ordered channel list
    for each ordered channel list l with K(l) non-empty:
        members ← [ enc(w) for each occurrence w in K(l) ]
        sort members bytewise
        append (tag 1, enc(l), members) to records
    ▸ canonical order, then an unambiguous serialization
    sort records by (tag, key bytes)
    bytes ← for each record in order:
                tag byte ‖ length(key) ‖ key ‖ count(members) ‖ for each m: length(m) ‖ m
    return SHA-256(bytes)                 ▸ every length and count is 8 bytes, big-endian
```

Three properties make $`\Delta`$ fit for W3.

1. **Canonical.** $`\Delta(\sigma)`$ depends on $`\sigma`$ only. Every multiset
   is serialized in sorted order and the records are sorted by their keys, so
   neither the order in which items arrived nor the order in which a backend
   iterates them can change the bytes.
2. **Distinguishing.** Every byte string is preceded by its length and every
   list by its count, so the serialization decodes back to the records, and
   because $`\operatorname{enc}`$ is injective the records determine the
   contents. Two different contents therefore give two different
   serializations, and equal digests imply equal contents unless SHA-256 has a
   collision ([FIPS 180-4](#references)). Because $`\operatorname{enc}`$ keeps
   the order of a channel list, the groups waiting on `(a, b)` and on `(b, a)`
   are different records.
3. **Outside both backends.** The harness computes $`\Delta`$ from values read
   through `RSpace`, never from a backend's stored bytes, roots or order.

### Choice-free timed cells

An operation list of one worker is $`\omega = (o_1, \ldots, o_n)`$. Each
$`o_i`$ is a produce, a consume, a checkpoint, or a reset to a checkpoint
position $`k < i`$. The cell's untimed construction builds the initial contents
$`\sigma_0`$; it is itself an operation list and is verified choice-free in the
same way, so it builds one initial contents. For contents $`\sigma`$ and a
produce or consume $`o`$, the enabled set $`E(\sigma, o)`$ is the set of COMMs
that $`o`$ can complete against $`\sigma`$; two COMMs that differ only in which
of several identical occurrences they consume count as one, because they lead
to the same contents.

The **eager reference semantics**, which leaf
`rspace-rho-bisimilarity-statement` (14304, pending at this amendment) specifies
as the reference of the PathMap backend's correctness criterion, is:

- a produce or consume fires any one member of its enabled set, and rests (its
  datum or continuation joins the contents) only when the set is empty;
- the contents after a given COMM are determined by that COMM: it removes the
  resting occurrences it consumes, except persistent items and peeked data, and
  the arriving operation does not rest;
- a checkpoint leaves the contents unchanged;
- a reset to position $`k`$ replaces the contents by those at position $`k`$.

The outcome set $`O_i(\omega)`$ is the set of contents this semantics can reach
after $`o_1, \ldots, o_i`$ over all choices, with $`O_0(\omega) = \{\sigma_0\}`$.
The list is **choice-free** when every reachable produce or consume enables at
most one COMM:

```math
\forall i \in \{1, \ldots, n\} \;\; \forall \sigma \in O_{i-1}(\omega) : \quad o_i \text{ is a produce or a consume} \implies \lvert E(\sigma, o_i) \rvert \le 1 .
```

**Lemma 1 (one worker).** If $`\omega`$ is choice-free, then $`O_i(\omega)`$ has
exactly one member for every $`i \in \{0, \ldots, n\}`$.

*Proof.* By strong induction on $`i`$. For $`i = 0`$,
$`O_0(\omega) = \{\sigma_0\}`$ by definition. Let $`i \ge 1`$ and assume that
$`O_j(\omega)`$ is a singleton $`\{\sigma_j\}`$ for every $`j < i`$. The operation
$`o_i`$ falls into exactly one of four cases.

1. $`o_i`$ is a produce or consume and $`E(\sigma_{i-1}, o_i)`$ is empty. The
   operation rests, so the only successor is $`\sigma_{i-1}`$ with the datum or
   continuation of $`o_i`$ added.
2. $`o_i`$ is a produce or consume and $`E(\sigma_{i-1}, o_i) = \{m\}`$. The
   operation fires $`m`$, and the contents after a given COMM are determined by
   that COMM, so the successor is unique.
3. $`o_i`$ is a checkpoint. The contents are unchanged, so
   $`O_i(\omega) = \{\sigma_{i-1}\}`$.
4. $`o_i`$ is a reset to position $`k < i`$. The contents become those at
   position $`k`$, which is the single member $`\sigma_k`$ of $`O_k(\omega)`$ by
   the induction hypothesis.

Choice-freedom excludes a produce or consume with two or more enabled COMMs, so
no other case exists, and $`O_i(\omega)`$ is a singleton. ∎

**Corollary 1 (one run verifies one worker).** Run the eager reference
semantics once over $`\omega`$ from $`\rho_0 = \sigma_0`$, and let $`\rho_i`$ be
the contents after $`o_i`$ on that run. If every produce or consume $`o_i`$ of
the run satisfies $`\lvert E(\rho_{i-1}, o_i) \rvert \le 1`$, then $`\omega`$ is
choice-free and $`O_i(\omega) = \{\rho_i\}`$ for every $`i`$.

*Proof.* By strong induction on $`i`$, $`O_i(\omega) = \{\rho_i\}`$. For
$`i = 0`$, $`O_0(\omega) = \{\sigma_0\} = \{\rho_0\}`$. Let $`i \ge 1`$ and assume
$`O_j(\omega) = \{\rho_j\}`$ for every $`j < i`$. The only contents reachable
before $`o_i`$ is then $`\rho_{i-1}`$, at which the run's check gives
$`\lvert E(\rho_{i-1}, o_i) \rvert \le 1`$ whenever $`o_i`$ is a produce or a
consume. The four cases of the proof of Lemma 1, with $`\rho`$ in place of
$`\sigma`$, give $`O_i(\omega) = \{\rho_i\}`$; in case 4 the run resets to
$`\rho_k`$, the single member of $`O_k(\omega)`$. Every reachable contents
before a produce or consume is therefore the run's, where the check holds, and
this is the definition of choice-free. ∎

**Consequence for W3.** In a choice-free timed cell each checkpoint position
$`k`$ has exactly one possible contents $`\sigma_k`$, by Lemma 1. Write
$`\sigma^{\mathrm{control}}_k`$ and $`\sigma^{\mathrm{treatment}}_k`$ for the
contents that the control arm and the treatment arm hold at position $`k`$. An
arm that follows the eager reference semantics holds $`\sigma_k`$ there, so if
both arms follow it, their digests are equal at every checkpoint position:

```math
\Delta\bigl(\sigma^{\mathrm{control}}_k\bigr) = \Delta(\sigma_k) = \Delta\bigl(\sigma^{\mathrm{treatment}}_k\bigr), \qquad k \text{ a checkpoint position}.
```

Contrapositively, a digest mismatch at a checkpoint of a timed cell shows that
at least one arm left the reference semantics: a correctness failure, not an
outlier. Choice-freedom also makes the work identical: both arms perform the
same COMMs and hold the same contents, so the throughput ratio compares two
backends doing the same work rather than two different choices. In a
choice-bearing cell the arms may hold different members of the outcome set and
their digests may differ without any failure; this is why W3 is confined to
timed cells and why choice-bearing cells are checked by membership instead.

**Diagram 2: a choice-free step and a choice-bearing step**

```text
  choice-free step                         choice-bearing step
  ────────────────                         ───────────────────
  resting:   x!(1)                         resting:   x!(1)    x!(2)
  arriving:  for (@y <- x) { Nil }         arriving:  for (@y <- x) { Nil }
  enabled set: one COMM (with x!(1))       enabled set: two COMMs (with x!(1)
                                                        or with x!(2))
  outcome set: { nothing left on x }       outcome set: { x!(2) left on x,
                                                          x!(1) left on x }
```

Here `x` is a name bound by an enclosing `new`; for example, the choice-bearing
step is the closed Rholang term `new x in { x!(1) | x!(2) | for (@y <- x) { Nil } }`
with both sends resting before the receive arrives.

**Several workers.** In a cell with $`q > 1`$ workers that change contents,
worker $`j`$ runs its own list $`\omega^{(j)}`$ of produces, consumes and reads of
the current contents. Checkpoints and resets are *barrier operations*: they
belong to no worker's list and are applied only at barriers, with all workers
joined. The barriers are numbered $`1, 2, \ldots`$ in order, barrier $`0`$ being
the end of the untimed construction; a worker's segments are the portions of its
list between consecutive barriers, and at each barrier the cell applies that
barrier's checkpoints and resets in a fixed order. Within a segment the
schedule is nondeterministic. Applying the condition above to each schedule
separately does not make the outcome set a singleton, as Diagram 3 shows: every
step of either schedule enables at most one COMM, yet the cell has two outcomes.

**Diagram 3: two workers, each step choice-free, two outcomes**

```text
  initial contents: the receive  for (@y <- x) { Nil }  rests on x
  worker 1 runs:  x!(1)                    worker 2 runs:  x!(2)

  schedule A                               schedule B
  1. x!(1) arrives: one enabled COMM;      1. x!(2) arrives: one enabled COMM;
     it fires with y = 1                      it fires with y = 2
  2. x!(2) arrives: no enabled COMM;       2. x!(1) arrives: no enabled COMM;
     x!(2) rests                              x!(1) rests
  final data on x: x!(2)                   final data on x: x!(1)
```

For such a cell the outcome set at a checkpoint is the set of contents
reachable there over every schedule and every choice, and W3 requires that it
have exactly one member at every checkpoint. Workers that only read recorded
states, such as a reader holding an old root, do not change contents and do not
count in $`q`$. Diagram 3 shows that a choice-free step is not enough. A cell
with eight workers is therefore verified by the following three conditions,
which this amendment adopts as the method (a decision of the coordinator). They
are stated with the *part* $`P_j`$ of worker $`j`$, a set of channels that the
cell's generator fixes for that worker. For an ordered channel list $`l`$,
$`c \in l`$ means that channel $`c`$ occurs in $`l`$, and $`l \subseteq P_j`$
means that every channel of $`l`$ lies in $`P_j`$.

1. **Channel-disjoint parts.** The parts are pairwise disjoint:
   $`P_j \cap P_{j'} = \emptyset`$ whenever $`j \neq j'`$. Every operation of
   worker $`j`$ names only channels of $`P_j`$: a produce sends on a channel of
   $`P_j`$, a consume waits on a list $`l \subseteq P_j`$, and a read reads
   channels of $`P_j`$. Every produce of the untimed construction sends on a
   channel of some part, and every consume of it waits on a list that lies
   inside one part, so every receive group, arriving or resting, lies inside one
   part. The runner checks this condition on the generated operation lists,
   outside both backends.
2. **Each worker choice-free.** Let $`\hat{\omega}^{(j)}`$ be worker $`j`$'s list
   with the barrier operations inserted: its first segment, the operations of
   barrier 1, its second segment, and so on, each reset naming the same
   checkpoint as in the cell. The executable reference runs once, from empty
   contents, over the untimed construction followed by $`\hat{\omega}^{(j)}`$,
   and checks at every produce or consume that the enabled set has at most one
   member; by Corollary 1, applied to that combined list, this one run verifies
   that the combined list is choice-free. Write $`\tau^{(j)}_k`$ for the
   contents this run holds at the cell's $`k`$-th checkpoint.
3. **Joined barriers.** Every checkpoint and every reset is applied at a
   barrier, with all workers joined, each having completed the prescribed
   prefix of its own list.

Five more notions are used below. The *restriction* $`\sigma\vert_{P_j}`$ of
contents $`\sigma = (D, K)`$ to a part keeps $`D(c)`$ for every channel
$`c \in P_j`$ and $`K(l)`$ for every list $`l \subseteq P_j`$, and maps every
other channel and list to the empty multiset. The *union* of contents
$`\sigma = (D, K)`$ and $`\sigma' = (D', K')`$ adds them channel by channel and
list by list,

```math
\sigma \uplus \sigma' = (D \uplus D', K \uplus K'), \qquad (D \uplus D')(c) = D(c) \uplus D'(c), \qquad (K \uplus K')(l) = K(l) \uplus K'(l),
```

where $`\uplus`$ on two multisets is their multiset sum. Contents are
*partitioned* when every channel $`c`$ with $`D(c)`$ non-empty lies in some part
and every list $`l`$ with $`K(l)`$ non-empty lies inside some part. Partitioned
contents are the union of their restrictions,
$`\sigma = \sigma\vert_{P_1} \uplus \cdots \uplus \sigma\vert_{P_q}`$: because
the parts are disjoint, such a channel lies in exactly one part and such a
list, which is non-empty, inside exactly one part, while empty entries
contribute nothing to either side. A *step* of worker $`j`$
applies one produce or consume of its list under the eager reference semantics:
it fires one member of the enabled set or, when the set is empty, rests. An
*execution* of the cell is the untimed construction followed by the steps of
every segment, in the order the schedule gives them, with the barrier operations
between the segments.

**Lemma 2 (channel-disjoint workers).** If a cell with $`q > 1`$ workers
satisfies conditions 1 to 3, then for every $`k`$ its outcome set at its
$`k`$-th checkpoint, over every schedule and every choice, has exactly one
member:

```math
\sigma_k = \tau^{(1)}_k\vert_{P_1} \uplus \tau^{(2)}_k\vert_{P_2} \uplus \cdots \uplus \tau^{(q)}_k\vert_{P_q} .
```

*Proof.* In five steps.

*Step 1: every reachable contents is partitioned.* By strong induction on the
number of operations applied, in an execution of the cell or of the untimed
construction followed by any $`\hat{\omega}^{(j)}`$. The construction starts
from empty contents, which are partitioned. Every further operation is of one
of six kinds. A produce that rests adds a datum on its channel, which lies in a
part by condition 1. A consume that rests adds a waiting continuation on its
list, which lies inside one part by condition 1. A produce or consume that
fires a COMM only removes resting occurrences, and the arriving operation does
not rest. A read and a checkpoint change nothing. A reset restores contents
reached earlier in the same execution, which are partitioned by the induction
hypothesis.

*Step 2: a step of worker $`j`$ reads and writes only part $`P_j`$.* Let
$`\sigma`$ be partitioned and $`c \in P_j`$. A list $`l`$ with $`c \in l`$ and
$`K(l)`$ non-empty lies inside one part, because $`\sigma`$ is partitioned, and
that part contains $`c`$, so it is $`P_j`$, the only part that contains $`c`$.
Table 2 lists, for the four kinds of step, what the step reads and writes in
each component of the tuple space: the data per channel, the waiting
continuations per ordered channel list, and the joins per channel. Joins do
not belong to the contents, being an index over $`K`$, but an arm that keeps them
keeps them per channel, so the table lists them too.

**Table 2: what one step of worker $`j`$ reads and writes**

| Step | Data, per channel | Waiting continuations, per ordered channel list | Joins, per channel |
| --- | --- | --- | --- |
| A produce of a datum on $`c \in P_j`$ that rests, its enabled set being empty | Reads the data on the channels of every list $`l`$ with $`c \in l`$ and $`K(l)`$ non-empty; adds the datum to $`D(c)`$ | Reads $`K(l)`$ for every list $`l`$ with $`c \in l`$ | Reads the joins of $`c`$ |
| A produce of a datum on $`c \in P_j`$ that fires a COMM with a continuation waiting on a list $`l`$ with $`c \in l`$ | Reads as in the row above; removes the matched data on the channels of $`l`$, except persistent and peeked data | Reads as in the row above; removes the continuation from $`K(l)`$ unless it is persistent | Reads the joins of $`c`$; may remove $`l`$ from the joins of the channels of $`l`$ once $`K(l)`$ is empty |
| A consume waiting on a list $`l \subseteq P_j`$ that rests, its enabled set being empty | Reads $`D(c')`$ for every channel $`c'`$ of $`l`$ | Adds the continuation to $`K(l)`$ | Adds $`l`$ to the joins of each channel of $`l`$ |
| A consume waiting on a list $`l \subseteq P_j`$ that fires a COMM | Reads $`D(c')`$ for every channel $`c'`$ of $`l`$; removes the matched data, except persistent and peeked data | Reads and writes nothing | Reads and writes nothing |

Every channel and list that Table 2 names lies in $`P_j`$: a produce's channel
$`c`$ and a consume's list $`l`$ by condition 1, and a list $`l`$ with
$`c \in l`$ and $`K(l)`$ non-empty, with all its channels, by the observation
above. A list $`l`$ with $`c \in l`$ and $`K(l)`$ empty offers no COMM,
whichever part its channels lie in. Two facts follow for every produce or
consume $`o`$ of worker $`j`$ and every partitioned $`\sigma`$:

- (a) the enabled set depends on part $`P_j`$ alone:
  $`E(\sigma, o) = E(\sigma\vert_{P_j}, o)`$;
- (b) if a step of worker $`j`$ takes $`\sigma`$ to $`\sigma'`$, then
  $`\sigma'\vert_{P_{j'}} = \sigma\vert_{P_{j'}}`$ for every worker
  $`j' \neq j`$, and the same step takes $`\sigma\vert_{P_j}`$ to
  $`\sigma'\vert_{P_j}`$.

A read by worker $`j`$ changes nothing and reads part $`P_j`$ alone.

*Step 3: adjacent steps of different workers commute.* Let $`\sigma`$ be
reachable, hence partitioned (Step 1), as are the contents after any step from
it; let $`s`$ be a step of worker $`j`$ possible at $`\sigma`$, and let
$`t`$ be a step of worker $`j' \neq j`$ possible at the contents that $`s`$
leaves. By Step 2(b), $`s`$ leaves part $`P_{j'}`$ unchanged, so by Step 2(a)
the operation of $`t`$ has the same enabled set before $`s`$ as after it: $`s`$
neither enables nor disables any COMM of $`t`$, and $`t`$, firing the same COMM
or resting on the same empty set, is possible at $`\sigma`$. Symmetrically,
$`t`$ leaves part $`P_j`$ unchanged, so $`s`$ is possible at the contents that
$`t`$ leaves. In both orders part $`P_j`$ ends as $`s`$ leaves it, part
$`P_{j'}`$ as $`t`$ leaves it, and every other part as in $`\sigma`$. Both
results are partitioned (Step 1), so each is the union of these same parts, and
the two orders reach the same contents.

*Step 4: every schedule sorts into the sequential one.* Fix an execution of the
cell and one of its segments. Two steps of the segment form an *inversion* when
a step of worker $`j'`$ comes before a step of worker $`j < j'`$. If the segment
has an inversion, it has one between adjacent steps; otherwise the worker index
would never decrease along the segment and no inversion could exist. Exchanging
two adjacent steps that form an inversion, which belong to different workers,
gives by Step 3 an execution in which every step is still possible, each worker
takes the same steps in the same order, and the contents after the pair, hence
at every later point, are unchanged. The exchange removes exactly one inversion
and creates none, since every other pair of steps keeps its order. After
finitely many exchanges, no more than the initial number of inversions, the
segment is *sequential*: all of worker 1's steps, then all of worker 2's, and
so on to worker $`q`$'s. Doing this in every segment, without moving any barrier
operation, turns the execution into a *sequential execution* that holds the
same contents at every checkpoint.

*Step 5: a sequential execution holds $`\sigma_k`$ at its $`k`$-th checkpoint.*
Fix a sequential execution and a worker $`j`$, and keep only the untimed
construction, worker $`j`$'s steps and the barrier operations. By induction
along the execution, the kept operations form an execution of the construction
followed by $`\hat{\omega}^{(j)}`$, and after every operation the two
executions hold the same part $`P_j`$. The construction is common to both. A
step of another worker leaves part $`P_j`$ unchanged (Step 2(b)) and is not
kept. Before a step of worker $`j`$, the contents of both executions are
partitioned (Step 1) and agree on $`P_j`$ (induction hypothesis), so by
Step 2(a) the step's operation has the same enabled set in both; the step is
therefore possible in both and changes part $`P_j`$ alike (Step 2(b)). A
checkpoint changes neither. A reset gives each execution its own contents at the named
checkpoint, which agree on $`P_j`$ by the induction hypothesis. By condition 2
and Corollary 1, every execution of the construction followed by
$`\hat{\omega}^{(j)}`$ holds $`\tau^{(j)}_k`$ at the cell's $`k`$-th checkpoint,
so the sequential execution holds $`\tau^{(j)}_k\vert_{P_j}`$ in part $`P_j`$
there. This holds for every worker, and partitioned contents are the union of
their parts, so the sequential execution holds $`\sigma_k`$. By Step 4, every
execution of the cell, under any schedule and any choices, holds at its
$`k`$-th checkpoint the contents of a sequential execution, which is
$`\sigma_k`$. ∎

**Diagram 4: sorting one segment of a two-worker cell (Lemma 2)**

```text
  worker 1 owns part P1 = {a1, b1}          worker 2 owns part P2 = {a2, b2}
  its segment: s1, s2, on P1 only           its segment: t1, t2, on P2 only

  ── barrier: all workers joined ───────────────────────────────────────────
     a schedule of the segment:  t1  s1  t2  s2    3 inversions
     exchange t1 and s1:         s1  t1  t2  s2    2 inversions
     exchange t2 and s2:         s1  t1  s2  t2    1 inversion
     exchange t1 and s2:         s1  s2  t1  t2    0: sequential
     each exchange keeps the contents after the pair (Steps 3 and 4)
  ── next barrier: all workers joined; a checkpoint ────────────────────────
     part P1 = worker 1's reference run there, restricted to P1 (Step 5)
     part P2 = worker 2's reference run there, restricted to P2 (Step 5)
     contents = part P1 ⊎ part P2, the same for every schedule
```

Diagram 3 breaks condition 1: both workers send on `x`, so their steps touch
one channel and need not commute. Condition 1 also confines a worker's reads to
its own part, which only its own steps change (Step 2(b)) and which, by the
argument of Step 5, follows the worker's own reference run step by step; so what
a worker reads from the current contents does not depend on the schedule
either. Lemma 2
gives a cell with several workers the consequence for W3 that Lemma 1 gives a
cell with one: each checkpoint has exactly one possible contents, and the
argument of "Consequence for W3" applies unchanged.

**What an eight-worker timed cell measures.** By condition 1 no two workers
name a common channel, so such a cell does not measure contention between
workers on one channel: they never race for one datum or one waiting
continuation, which is the race that Diagram 3 shows to be a choice. Contention
on everything the workers share remains and is measured: the arm's locks, its
store and its publication path, and the joined barrier at every checkpoint,
which both arms take at the same positions of the same operation lists.

**Verification before sampling.** The executable reference is delivered by leaf
`rspace-executable-reference-tuplespace` (14306), which is pending at this
amendment, so no timed cell can be verified yet and no timed cell can be
sampled. As that leaf specifies it, the reference keeps contents as ordered maps
of multisets, computes enabled sets by exhaustive enumeration, depends on no
PathMap type, on no order in which a space examines candidates and on no hash
value, and never sorts a channel list. For each one-worker timed cell the runner
runs the reference once, from empty contents, over the untimed construction
followed by the timed operation list, and checks at every produce or consume
that the enabled set has at most one member; by Corollary 1 this one run
verifies that the cell is choice-free. For each eight-worker timed cell the
runner checks condition 1 on the generated operation lists, outside both
backends, and runs the reference once per worker as condition 2 describes; the
harness applies every checkpoint and reset at a joined barrier (condition 3),
and by Lemma 2 the cell then has one outcome at every checkpoint. For every
cell, timed or untimed, the runner also checks that no two consumes of its
operation lists name different orders of one channel set (see
[Traces excluded from every cell](#traces-excluded-from-every-cell)). The
reference's reports and the partition checks for the timed cells of a run
instance are retained in one file, whose SHA-256 is the run instance's
`choice_freedom_receipt_sha256`; without it preflight refuses the run instance
(`choice_freedom_unverified`).

### The untimed choice-bearing cell of each lane

Restricting timed cells to choice-free traces would hide defects that appear
only when several COMMs are enabled: candidate selection, joins racing for one
datum, persistent and peeked items under contention. Each applicable lane
therefore has exactly one **untimed choice-bearing cell**
([Table 4](#required-matrix-and-measured-quantities)). Its operation list
deliberately enables several COMMs at some steps. Each arm runs it once,
untimed and never counted in a ratio, and it checks:

- **W1**: both arms ran the same operation list;
- **W2**: each arm against itself;
- **membership**: every COMM an arm performs is a member of the reference's
  enabled set for that arm's own contents and the arriving operation, and the
  arm rests only when that set is empty. The reference follows the arm's own
  choices step by step, as the conformance driver of leaf 14306 does.

The two arms may end in different members of the outcome set; that is not a
failure, and W3 is not applied. The cell's configuration is frozen in its
per-cell manifest before sampling, like every cell's, and like every cell it
contains no two orders of one channel set
([Traces excluded from every cell](#traces-excluded-from-every-cell)).

### Traces excluded from every cell

A receive on several channels, such as `for (@u <- a & @v <- b) { Nil }` in
Rholang, waits in a group keyed by its ordered channel list. The Rho calculus
distinguishes the lists `(a, b)` and `(b, a)`, and so does the treatment arm: the
PathMap backend keys a group by its literal ordered list, framing each channel
in list order (`RhoPathV1::continuation_prefix` in
[`rho_path.rs`](../../../rspace++/src/rspace/rho_path.rs)), following the
campaign owner's ruling that "(a,b) and (b,a) should have different prefixes".
The LMDB arm does not. Its history key for a group is computed from the list's channel hashes
after sorting them (`hash_from_vec` in
[`stable_hash_provider.rs`](../../../rspace++/src/rspace/hashing/stable_hash_provider.rs)),
both when a checkpoint writes the group
([`history_repository_impl.rs`](../../../rspace++/src/rspace/history/history_repository_impl.rs))
and when a group is read back
([`rspace_history_reader_impl.rs`](../../../rspace++/src/rspace/history/instances/rspace_history_reader_impl.rs)),
while its hot store keeps one group per literal list
([`hot_store.rs`](../../../rspace++/src/rspace/hot_store.rs)). The two orders
therefore share one history key in that arm: a group read in one order holds
what was stored in the other, so the arm merges the two groups, and its radix
history refuses a checkpoint batch that holds two actions on one key
([`radix_history.rs`](../../../rspace++/src/rspace/history/instances/radix_history.rs),
"Cannot process duplicate actions on one key."). These are statements about the
source, read at the base of this amendment, not results of a run.

**Diagram 5: two orders of one channel set in each arm**

```text
  Rholang:  for (@u <- a & @v <- b) { Nil }   waits on the list (a, b)
            for (@u <- b & @v <- a) { Nil }   waits on the list (b, a)

  treatment arm: PathMap backend             control arm: LMDB backend
  key = the literal ordered list             key = hash of the sorted channel hashes
  ┌──────────────────┐ ┌──────────────────┐  ┌──────────────────────────────────────┐
  │ group (a, b)     │ │ group (b, a)     │  │ one history key for (a, b) and       │
  └──────────────────┘ └──────────────────┘  │ (b, a): a group read in one order    │
  two groups, as the Rho calculus has them   │ holds what was stored in the other   │
                                             │ (the groups merge), and a checkpoint │
                                             │ batch that changes both is refused   │
                                             │ (two actions on one key)             │
                                             └──────────────────────────────────────┘
```

This is an artefact of the control arm, not of the Rho calculus, and **traces
that contain two orders of one channel set are excluded from every cell, timed
and untimed**. In a timed cell such a trace would make W3 fail on the artefact
and make the two arms do different work. In an untimed choice-bearing cell it
would break W1: both arms must be able to run the same operation list, and the
LMDB arm refuses a checkpoint whose batch changes both groups (two actions on
one key, refused in
[`radix_history.rs`](../../../rspace++/src/rspace/history/instances/radix_history.rs)
with "Cannot process duplicate actions on one key."), so it cannot complete a
list that the PathMap arm completes. A cell that only the PathMap arm can run is
not a comparison and does not belong in this protocol.

### Trace content

The fixed trace includes shared-prefix, random and adversarial channels,
repeated keys, duplicate channels, datum-first and receive-first COMM,
no-match and matched operations, checkpoint/reset, and a retained-root
transfer. In every cell the trace contains no two orders of one channel set. In
a timed cell it is also choice-free; the choice-bearing behaviour is exercised
by each lane's untimed choice-bearing cell. Seeds and exact serialized inputs
are retained with raw results.

### Procedure for one cell

**Diagram 6: from freezing a cell to its verdict** (rounded boxes are decisions,
square boxes are steps, ✗ marks an outcome without a timing verdict)

```text
  ┌───────────────────────────────────────────────┐
  │ freeze: source commit, per-cell manifest,     │
  │ seeded operation list, durability contract    │
  └───────────────────────┬───────────────────────┘
                          ▼
  ╭───────────────────────────────────────────────╮   no
  │ same durability, retention, cache and         ├───────▶ ✗ ineligible cell:
  │ concurrency contract in both arms?            │           repair the contract
  ╰───────────────────────┬───────────────────────╯
                          │ yes
                          ▼
  ╭───────────────────────────────────────────────╮   no
  │ timed cell verified by the executable         ├───────▶ ✗ no choice-freedom
  │ reference: choice-free, and with eight        │           receipt: not sampled
  │ workers channel-disjoint with joined          │
  │ barriers (Lemma 2); no list of the cell or    │
  │ of its lane's choice-bearing cell with two    │
  │ orders of one channel set?                    │
  ╰───────────────────────┬───────────────────────╯
                          │ yes
                          ▼
  ┌───────────────────────────────────────────────┐
  │ untimed witness runs: the lane's choice-      │
  │ bearing cell (W1, W2, membership) and each    │
  │ arm's kill/reopen durability witness          │
  └───────────────────────┬───────────────────────┘
                          ▼
  ╭───────────────────────────────────────────────╮   no
  │ every witness passes?                         ├───────▶ ✗ correctness failure:
  │                                               │           no timing verdict
  ╰───────────────────────┬───────────────────────╯
                          │ yes
                          ▼
  ┌───────────────────────────────────────────────┐
  │ randomized paired AB/BA trials; after each    │
  │ timed run: W1, W2 and W3 for that pair        │
  └───────────────────────┬───────────────────────┘
                          ▼
  ╭───────────────────────────────────────────────╮   no
  │ every pair passes W1, W2 and W3?              ├───────▶ ✗ correctness failure,
  │                                               │           not an outlier
  ╰───────────────────────┬───────────────────────╯
                          │ yes
                          ▼
  ┌───────────────────────────────────────────────┐
  │ retain raw timing and resource artefacts;     │
  │ apply the preregistered win and regression    │
  │ gates                                         │
  └───────────────────────────────────────────────┘
```

## Required matrix and measured quantities

The **component** lane is not applicable and is marked so before sampling
(Table 5); it stays listed so that it cannot silently disappear. The **owner**
lanes invoke each arm's real `RSpace`, the treatment with the durable backend
selected, after migration, incremental-successor, GC and state-sync readiness.
No component result may be promoted to an end-to-end claim.

**Table 3: lanes, levels and outputs**

| Lane | Frozen variable and levels | Required outputs |
| --- | --- | --- |
| Component admission and persistence | **Not applicable before sampling** (Table 5). Same canonical object stream and batch size; fresh versus reused digest; 4,096 and 65,536 objects | Admission/copy/commit latency, bytes read/written, allocations, peak resident set size (RSS), file syncs, and installed physical bytes. |
| Owner primary and unchanged-state slope | One worker; 128-byte payload; 64 changed paths per checkpoint; unchanged current objects 4,096/65,536/1,048,576; 16 retained roots with 95% sharing; checkpoint every 64 mutations | Acknowledged durable batches/s, p50/p95/p99 operation and checkpoint latency, the cell's work-equivalence witness results (manifest output `semantic_witness`), work per changed path. The 65,536-object cell is the single primary decision cell. |
| Delta, sharing and retention | At 65,536 current objects vary delta 1/64/4,096; retained roots 1/16/128; sharing 0/50/95%, one factor at a time | Current/retained reference activations and deactivations, new bytes, old-closure work, metadata work, dedup, and latency. Include repeated update, full removal and same-digest reuse; the retention-eviction case is **not applicable** (Table 5). |
| Concurrency and pinned readers | One versus eight writer/reader workers; no pin versus long-lived old-root pin | Committed throughput, p99, lock/contention, WAL/replay-tail growth and available recovery point; GC deferral is **not applicable** (Table 5). |
| Publication and lifecycle | Checkpoint every 1/64/1,024 mutations; pack/WAL rotation on/off where contract permits; clean and torn restart; index repair; GC with and without pins (**not applicable**, Table 5); the added reset-then-checkpoint cell | Checkpoint, reopen/recovery, compaction latency and work, write amplification, retained and garbage bytes, all pack/index/WAL/catalog/cache files and inode count; GC work is **not applicable** (Table 5). For the reset-then-checkpoint cell: reset latency and the latency of the checkpoint that follows. |
| Transfer and COMM | Cold retained-root export/import and state sync; shared/random/adversarial channels; datum-first and receive-first | Transfer bytes/latency, each arm's imported root equal to its own exported root (W2), produce/consume/COMM throughput and p99, candidate and node visits. |
| History scaling (added) | 16, 256 and 4,096 retained states, each built by a choice-free operation list and kept under keep-all retention; other factors at the primary cell's values | Reopen latency, peak RSS and physical bytes. |

The **reset-then-checkpoint** cell runs at the primary cell's configuration: its
operation list resets to an earlier checkpoint position, which each arm maps to
its own recorded root, and then checkpoints. The **history-scaling** lane
measures how reopening a store and its footprint grow with the number of
retained states. The reset-then-checkpoint cell and the three history-scaling
cells are timed secondary cells: choice-free, verified before sampling, and each
sampled with 30 pairs.

The matrix varies axes independently around the primary cell rather than
forming an enormous cross-product. A resource-capped or unsupported large
cell is a recorded failure/limit; it must not silently disappear. For each
phase measure both logical file lengths and physical allocated bytes for the
**whole store tree**, including obsolete but still retained packs, indexes,
WAL, catalog, old generations and any enabled cache/ACT images. Record file
and directory inode counts. Write amplification is total physical write bytes
divided by newly admitted canonical bytes; report the numerator separately
when block-device attribution is unavailable. Record allocation calls/bytes,
peak RSS, major/minor faults, syscalls, instructions/cycles/branches/cache
events, and time spent in read/write/sync/rename. Device counters and process
I/O counters have different scopes and must be labeled accordingly.

**Table 4: the cells of each lane**

| Lane | Timed cells (all choice-free) | Untimed choice-bearing cell | Not applicable before sampling |
| --- | --- | --- | --- |
| Component admission and persistence | None | None | The whole lane |
| Owner primary and unchanged-state slope | Every level, including the primary cell | `owner-primary-unchanged-state-choice-bearing` | Nothing |
| Delta, sharing and retention | Every level except the retention-eviction case | `delta-sharing-retention-choice-bearing` | The retention-eviction case |
| Concurrency and pinned readers | Every level; the eight-worker cells under the three conditions of Lemma 2 (see Several workers in [Choice-free timed cells](#choice-free-timed-cells)) | `concurrency-pinned-readers-choice-bearing` | The GC-deferral output |
| Publication and lifecycle | Every level except GC with and without pins, plus `publication-lifecycle-reset-then-checkpoint` | `publication-lifecycle-choice-bearing` | GC with and without pins; the GC-work output |
| Transfer and COMM | Every level | `transfer-comm-choice-bearing` | Nothing |
| History scaling | 16, 256 and 4,096 retained states | `history-scaling-choice-bearing` | Nothing |

**Table 5: not-applicable markings, made before sampling**

| Item | Marked not applicable | Reason |
| --- | --- | --- |
| Component lane | The lane `component_admission_persistence` | Both arms are the real `RSpace` with one backend selected each. This lane drives neither arm's `RSpace`, and its precondition, identical canonical bytes in both arms, cannot hold because the PathMap backend does not reproduce LMDB's bytes. |
| Paired retained-root GC | The publication lane's GC-with-and-without-pins cells, its GC-work output, the concurrency lane's GC-deferral output, and the retained-root GC critical sentinel | Both arms run keep-all retention and LMDB reclaims nothing: its object store refuses deletion ("content-addressed RSpace objects cannot be deleted" in [`rspace_state_authority.rs`](../../../rspace++/src/rspace/state/rspace_state_authority.rs)). No garbage collection runs in either arm, so a GC measurement has no control value to pair with. |
| Retention eviction | The delta lane's retention-eviction case | Both arms run keep-all retention and LMDB reclaims nothing, so no retained state is evicted in either arm. |

## Paired sampling and decision rule

For the primary cell use 51 complete pairs of separate control and treatment
runs. Randomize AB versus BA order within each pair with a recorded seed,
balance order, and use equivalent fresh directories and process starts for
cold lanes. Do not reuse a warmed process for one arm and a cold process for
the other. Use 30 complete pairs for each predeclared secondary cell. Lock
the trace seed, pair key, arm order, affinity, worker budget and host snapshot
before measurement. Preflight physical-core idleness with `mpstat`/the
existing `scripts/bench/benchmark_host.py`; postpone a pair if selected cores
are busy. Pin each arm to the same single CCD, set only its selected cores to
the `performance` governor, and record the scaling driver, boost state, full
hardware string, CPU model, `uname -a` and exact affinity. Record the command
and output once per pair; never rerun a pair to select a favorable result.
Discard declared warm-up runs before sampling. Run under a hard `systemd-run`
memory ceiling and `MemorySwapMax=0`; the ceiling is **not** observed RSS.
Timed paired runs have no profiler attached.
Use headless `heaptrack` reports and AMD uProf on separate diagnostic repeats,
never a GUI and never a profiled run as a timing replicate.
The measurement host is an AMD Ryzen Threadripper PRO 5975WX. The full uProf
suite is installed on that host, with `AMDuProfCLI` on `PATH`.
Use its command-line collector and reports for sampled AMD counter and CPI
diagnostics, retaining the exact command, configuration and raw output. At the
2026-10-01 preflight, `perf_event_paranoid` was 2 and `AMDuProfPcm` could not
collect its cumulative IPC counters without a lower setting. Keep the host
setting unchanged for this comparison; use per-process `perf stat` for exact
instruction/cycle counts where it is permitted, and label unavailable counters
as unavailable. Run `heaptrack` with `--record-only` and empty `DISPLAY` and
`WAYLAND_DISPLAY`, then use `heaptrack_print` to produce its report.

Let $`T_i`$ and $`C_i`$ be treatment and control durable-batch-throughput
observations in pair $`i`$. The preregistered primary estimate is the median
of the 51 within-pair ratios:

```math
R_{\mathrm{primary}}=\operatorname{median}_{1\le i\le 51}\left(T_i/C_i\right).
```

Use 100,000 pair-preserving bootstrap resamples with recorded seed
`20261001`, reporting a two-sided 99% interval for that median. A throughput
**improvement claim** requires its lower bound to exceed 1.05 in the primary
cell. A neutral or negative result remains valid evidence for task 7951.
For each critical sentinel (singleton/4,096, primary/65,536, largest eligible
unchanged-state cell, eight-worker COMM, cold recovery and retained-root GC),
the lower 99% throughput-ratio bound must be at least 0.95 and the upper 99%
ratio for p99 latency at most 1.05 to exclude a material regression. Revision 0
named the first of these sentinels, singleton/4,096, without defining the word
"singleton". Before any sample, this amendment fixes the label to denote the
owner-primary lane's cell at 4,096 unchanged current objects, the smallest state
of that lane; the label then has the form cell/objects of the next one,
primary/65,536. The other five sentinels are unchanged. Where a metric is not
applicable, mark it as such before sampling: the retained-root GC sentinel is
not applicable (Table 5). The eight-worker COMM sentinel, like every
eight-worker timed cell, is verified by the three conditions of Lemma 2 (see
Several workers in [Choice-free timed cells](#choice-free-timed-cells)). The
work-equivalence witnesses W1, W2 and W3 and each arm's kill/reopen durability
witness must pass exactly; an eligible cell must stay within its
recorded memory/disk cap. Report RSS, physical bytes, write amplification and
inode ratios with 99% paired intervals; a greater-than-5% physical-footprint
or peak-RSS regression in a critical cell is a review blocker, not a result to
hide behind a faster primary point estimate. Any failure of these guards
blocks a clean adoption verdict; it does not justify changing the locked
threshold or removing the adverse cell.

The existing [paired statistics helper](../../../scripts/bench/paired_benchmark_statistics.py)
provides pair-preserving 99% bootstrap views. Retain every raw observation,
host snapshot, contents digest, choice-freedom receipt, failure, run UUID,
profiler report and file hash in pgmcp; register the exact hypotheses and
thresholds **before** 7950 samples. No new benchmark execution or verdict is
claimed by this document.

## References

- National Institute of Standards and Technology. *Secure Hash Standard (SHS)*.
  Federal Information Processing Standards Publication 180-4, August 2015.
  [doi:10.6028/NIST.FIPS.180-4](https://doi.org/10.6028/NIST.FIPS.180-4).
- L. G. Meredith and M. Radestock. "A Reflective Higher-order Calculus".
  *Electronic Notes in Theoretical Computer Science* 141(5):49–67, 2005.
  [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
- D. Sangiorgi. "On the origins of bisimulation and coinduction".
  *ACM Transactions on Programming Languages and Systems* 31(4):1–41, 2009.
  [doi:10.1145/1516507.1516510](https://doi.org/10.1145/1516507.1516510).

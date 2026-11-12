------------------------ MODULE RSpaceGcPackLeaseRace ------------------------
\* The race between generation leases and the reclamation of a pack range, for
\* processes that open one durable directory one after another.
\*
\* What is modelled. A process opens the store by taking the directory's
\* writer lock. While it holds the lock it is the owner: it admits read
\* handles and reclaims superseded pack ranges. A read handle is a generation
\* lease in the process's own lease registry together with open descriptors on
\* the packs of its generation, pinned when the generation was admitted. The
\* registry is process memory: it defers reclamation by the same process and
\* is invisible to every other process. A handle may outlive its owner and the
\* owner's lock, so a later owner in another process may remove pack entries
\* that the handle still reads. There is no durable lease record, no liveness
\* file and no lock on the read path: the handle's reads stay correct because
\* they go through the pinned descriptors.
\*
\* Source citations. A comment line of the form
\*     <path>:<line> | <text>
\* cites one source line. <text> is that line without its indentation, `P/`
\* abbreviates `rspace++/src/rspace/persistence/`, and every other path is
\* relative to the repository root. The line numbers are those of the tree in
\* which the two-process form was introduced; after later edits, locate a
\* line by its text.
\*
\* Modelled facts.
\*   F1  Every opener takes the writer lock and keeps it for the lifetime of
\*       its owner; a second opener is refused (OpenStore, CloseOwner).
\*   F2  A read handle can outlive its owner and the owner's lock
\*       (CloseOwner).
\*   F3  A read handle reads through pack descriptors pinned at admission
\*       (Acquire, HandleRead).
\*   F4  Reclamation removes directory entries and never rewrites a pack in
\*       place (Unlink).
\*   F5  A lease registry is process-local, refuses a lease at or below its
\*       retired frontier, and retires a pack range only when no lease of the
\*       same process overlaps it (Acquire, Release, BlockFirst, RetireFirst,
\*       RetireSecond).
\*   K1  The kernel keeps the contents of an unlinked file while a descriptor
\*       on it is open (HandleRead). POSIX unlink(): "If one or more processes
\*       have the file open when the last link is removed, the link shall be
\*       removed before unlink() returns, but the removal of the file contents
\*       shall be postponed until all references to the file are closed."
\*       https://pubs.opengroup.org/onlinepubs/9699919799/functions/unlink.html
\*   K2  The kernel closes every descriptor of a process that terminates, for
\*       whatever reason (Crash). POSIX _Exit(), Consequences of Process
\*       Termination: "All of the file descriptors, directory streams,
\*       conversion descriptors, and message catalog descriptors open in the
\*       calling process shall be closed."
\*       https://pubs.opengroup.org/onlinepubs/9699919799/functions/_Exit.html
\*
\* Abstractions.
\*   A1  A handle's lease and its pins are taken in one step under the writer
\*       lock. The code can also create a handle later from the published
\*       core; that handle shares the descriptors which the core opened at
\*       admission, so its reads are the reads of a handle taken at admission.
\*         P/durable/rspace_authority.rs:347 | pub(crate) fn read_handle(
\*   A2  Packs 1..2 and 3..4 are two successive superseded ranges; the
\*       publication of their successors is not modelled. Pack 5 is never
\*       reclaimed.
\*   A3  Appends and the cut of an unpublished tail are not modelled: both
\*       touch only bytes beyond every admitted tail. The one truncation of a
\*       pack cuts to the durable tail that the catalog appoints.
\*         P/durable/publication.rs:1001 | let appointed = current.pack().durable_pack_tail();
\*         P/durable/publication.rs:1004 | pack.set_len(appointed)?;
\*   A4  A read is not a step. PinnedReadReturnsAdmittedBytes quantifies over
\*       every read that a live handle could issue in a state.
EXTENDS Naturals, FiniteSets, TLC

\* Mutant switch. TRUE lets reclamation test the exact catalog key instead of
\* the physical pack overlap. That is the test reclamation must not use:
\*   P/retention.rs:650 | Ok(state.leases.values().any(|candidate| *candidate == key))
CONSTANT ExactKeyOnly
ASSUME ExactKeyOnly \in BOOLEAN

\* Mutant switch. TRUE makes a read handle keep no pack descriptor: it opens
\* the pack by its path name for every read. The code reads through the
\* descriptor pinned at admission:
\*   P/durable/admission.rs:939 | pinned.file.read_exact_at(offset, &mut bytes)?;
CONSTANT ReopenByPathName
ASSUME ReopenByPathName \in BOOLEAN

\* Mutant switch. TRUE makes reclamation cut or overwrite the bytes of a pack
\* before it removes the entry. The code removes the entry and nothing else:
\*   P/durable/io.rs:1902 | let result = unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) };
CONSTANT RewriteInPlace
ASSUME RewriteInPlace \in BOOLEAN

\* The operating-system processes that open the durable directory; the cfg
\* files bind two. NoProcess stands for "no process holds the writer lock".
CONSTANT Procs
NoProcess == "no process"
ASSUME IsFiniteSet(Procs) /\ Procs # {} /\ NoProcess \notin Procs

\* A lease names a generation by its key: catalog epoch, first live pack,
\* active pack and index generation.
\*   P/retention.rs:483 | pub struct GenerationLeaseKeyV1 {
FirstKey == [epoch |-> 2, first |-> 1, last |-> 2, index |-> 2]
OlderSameKey == [epoch |-> 1, first |-> 1, last |-> 2, index |-> 2]
PartialKey == [epoch |-> 3, first |-> 2, last |-> 3, index |-> 3]
SecondKey == [epoch |-> 4, first |-> 3, last |-> 4, index |-> 4]
OutsideKey == [epoch |-> 5, first |-> 5, last |-> 5, index |-> 5]
Readers == {"older", "target", "partial", "successor", "outside"}
KeyOf(reader) ==
  CASE reader = "older" -> OlderSameKey
    [] reader = "target" -> FirstKey
    [] reader = "partial" -> PartialKey
    [] reader = "successor" -> SecondKey
    [] reader = "outside" -> OutsideKey
FirstPack(key) == key.first
LastPack(key) == key.last
Packs(key) == FirstPack(key)..LastPack(key)
Overlaps(left, right) ==
  FirstPack(left) <= LastPack(right) /\ FirstPack(right) <= LastPack(left)
FirstPacks == Packs(FirstKey)
SecondPacks == Packs(SecondKey)
Max2(left, right) == IF left >= right THEN left ELSE right

\* State.
\*   lock            the process that holds the writer lock, or NoProcess (F1)
\*   alive[p]        whether process p runs
\*   leases[p]       the read handles of p: the leases in p's registry (F2, F5)
\*   pinned[p]       the read handles of p that hold open pack descriptors (F3)
\*   retiredThrough[p], priorFrontier[p], stage[p]
\*                   p's registry: its retired-pack frontier, the frontier
\*                   before its latest retirement, and the last range it
\*                   retired (0 none, 1 the first range, 2 the second) (F5)
\*   deleted         the packs whose directory entry is removed (F4)
\*   rewritten       the packs whose stored bytes were changed in place; it
\*                   stays empty unless RewriteInPlace
\*   unlinkedBy[p]   the packs whose entry this incarnation of p removed
\*   deferredOlder[p], retiredAfterOlder[p], witnessComplete[p]
\*                   observers of the older-epoch deferral lifecycle in p
\* `lock`, `deleted` and `rewritten` belong to the directory and survive every
\* process; all other variables are memory of one process and end with it.
VARIABLES lock, alive, leases, pinned, retiredThrough, priorFrontier, stage,
          deleted, rewritten, unlinkedBy,
          deferredOlder, retiredAfterOlder, witnessComplete

vars == <<lock, alive, leases, pinned, retiredThrough, priorFrontier, stage,
          deleted, rewritten, unlinkedBy,
          deferredOlder, retiredAfterOlder, witnessComplete>>

\* F5. The leases of process p that defer the retirement of a pack range. The
\* code tests physical overlap, whatever the epoch of the lease:
\*   P/retention.rs:667 | candidate.first_live_pack <= key.active_pack &&
\*   P/retention.rs:668 | key.first_live_pack <= candidate.active_pack
Blocking(p, key) ==
  {reader \in leases[p] :
    IF ExactKeyOnly THEN KeyOf(reader) = key ELSE Overlaps(KeyOf(reader), key)}

\* A range is pending while one of its packs still has its directory entry.
Pending(key) == Packs(key) \ deleted # {}

\* Whether p's own registry retired the first range. A process that opens
\* after another process removed the first range retires only the second one:
\* its frontier then passes the first range without an overlap test on it.
RetiredFirst(p) ==
  stage[p] = 1 \/ (stage[p] = 2 /\ priorFrontier[p] = LastPack(FirstKey))
RetiredPacks(p) ==
  (IF RetiredFirst(p) THEN FirstPacks ELSE {}) \cup
  (IF stage[p] = 2 THEN SecondPacks ELSE {})

\* F4, K1. What a read of a pack returns. The directory entry and every open
\* descriptor of a pack lead to the same file: a rewrite in place reaches
\* every reader, the removal of the entry reaches only a reader that opens the
\* pack by its name.
StoredBytes(pack) == IF pack \in rewritten THEN "other" ELSE "admitted"
ReadByPathName(pack) ==
  IF pack \in deleted THEN "missing" ELSE StoredBytes(pack)

\* F3, K1. A handle that holds descriptors reads through them and therefore
\* does not depend on the directory entry:
\*   P/durable/admission.rs:939 | pinned.file.read_exact_at(offset, &mut bytes)?;
HandleRead(p, reader, pack) ==
  IF reader \in pinned[p] THEN StoredBytes(pack) ELSE ReadByPathName(pack)

Init ==
  /\ lock = NoProcess
  /\ alive = [p \in Procs |-> TRUE]
  /\ leases = [p \in Procs |-> {}]
  /\ pinned = [p \in Procs |-> {}]
  /\ retiredThrough = [p \in Procs |-> 0]
  /\ priorFrontier = [p \in Procs |-> 0]
  /\ stage = [p \in Procs |-> 0]
  /\ deleted = {}
  /\ rewritten = {}
  /\ unlinkedBy = [p \in Procs |-> {}]
  /\ deferredOlder = [p \in Procs |-> FALSE]
  /\ retiredAfterOlder = [p \in Procs |-> FALSE]
  /\ witnessComplete = [p \in Procs |-> FALSE]

\* F1. Process p opens the store: it takes the exclusive, non-blocking lock on
\* the writer lock file and becomes the owner. While another process holds the
\* lock the attempt fails with WriterLockContended, so the step is disabled.
\*   P/durable/io.rs:937 | match lock.file.try_lock() {
\*   P/durable/io.rs:942 | Err(TryLockError::WouldBlock) => Err(DurableIoError::WriterLockContended {
\* Every recovery of the directory takes the lock first:
\*   P/durable/startup.rs:303 | recover_physical_with_lease_v1(self.try_acquire_writer()?, limits, wal_limits)
OpenStore(p) ==
  /\ alive[p]
  /\ lock = NoProcess
  /\ lock' = p
  /\ UNCHANGED <<alive, leases, pinned, retiredThrough, priorFrontier, stage,
                 deleted, rewritten, unlinkedBy,
                 deferredOlder, retiredAfterOlder, witnessComplete>>

\* F1, F2. The owner of p closes: dropping the writer lease unlocks the lock
\* file. The owner holds that lease and a read handle does not, so the handles
\* of p, their descriptors and p's registry stay.
\*   P/durable/rspace_authority.rs:258 | lease: DurableWriterLeaseV1,
\*   P/durable/io.rs:1070 | let _ = self._lock.file.unlock();
\*   P/durable/mod.rs:533 | core: std::sync::Arc<admission::DurableAuthorityCoreV1<P>>,
\*   rspace++/tests/persistence/durable_read_handle.rs:78 | .expect("read through the handle after owner teardown"),
CloseOwner(p) ==
  /\ lock = p
  /\ lock' = NoProcess
  /\ UNCHANGED <<alive, leases, pinned, retiredThrough, priorFrontier, stage,
                 deleted, rewritten, unlinkedBy,
                 deferredOlder, retiredAfterOlder, witnessComplete>>

\* K2. Process p terminates for any reason. The kernel closes its descriptors:
\* the writer lock, a lock on an open file description, is released, and the
\* pinned packs lose the descriptors of p. The registry, the leases and the
\* observers are process memory and vanish. The directory keeps its state.
\*   P/retention.rs:561 | static REGISTRIES: OnceLock<Mutex<DirectoryLeaseRegistries>> = OnceLock::new();
Crash(p) ==
  /\ alive[p]
  /\ alive' = [alive EXCEPT ![p] = FALSE]
  /\ lock' = IF lock = p THEN NoProcess ELSE lock
  /\ leases' = [leases EXCEPT ![p] = {}]
  /\ pinned' = [pinned EXCEPT ![p] = {}]
  /\ retiredThrough' = [retiredThrough EXCEPT ![p] = 0]
  /\ priorFrontier' = [priorFrontier EXCEPT ![p] = 0]
  /\ stage' = [stage EXCEPT ![p] = 0]
  /\ unlinkedBy' = [unlinkedBy EXCEPT ![p] = {}]
  /\ deferredOlder' = [deferredOlder EXCEPT ![p] = FALSE]
  /\ retiredAfterOlder' = [retiredAfterOlder EXCEPT ![p] = FALSE]
  /\ witnessComplete' = [witnessComplete EXCEPT ![p] = FALSE]
  /\ UNCHANGED <<deleted, rewritten>>

\* A new process takes the place of a terminated one. It starts with an empty
\* registry and no descriptor, over whatever the directory holds. A registry
\* that retired a range lives as long as its process, not longer:
\*   P/retention.rs:684 | registries.retired.insert(identity, Arc::clone(&self.state));
Restart(p) ==
  /\ ~alive[p]
  /\ alive' = [alive EXCEPT ![p] = TRUE]
  /\ UNCHANGED <<lock, leases, pinned, retiredThrough, priorFrontier, stage,
                 deleted, rewritten, unlinkedBy,
                 deferredOlder, retiredAfterOlder, witnessComplete>>

\* F3, F5, A1. The owner p admits a generation and hands out a read handle on
\* it. The registry refuses a range at or below its retired frontier:
\*   P/retention.rs:626 | if key.first_live_pack <= state.retired_pack_through {
\* Admission pins the generation under the writer lease by opening each of its
\* packs by name, which fails when an entry is gone:
\*   P/durable/admission.rs:2464 | let store = DurableIndexedObjectStoreV1::pin(lease.directory(), &authority, index)?;
\*   P/durable/admission.rs:796 | .open_pack(id)
\* The handle keeps the lease and, through the shared core, the descriptors:
\*   P/durable/mod.rs:535 | _lease: GenerationLeaseV1,
Acquire(p, reader) ==
  /\ lock = p
  /\ reader \in Readers \ leases[p]
  /\ FirstPack(KeyOf(reader)) > retiredThrough[p]
  /\ Packs(KeyOf(reader)) \cap deleted = {}
  /\ leases' = [leases EXCEPT ![p] = @ \cup {reader}]
  /\ pinned' =
       [pinned EXCEPT ![p] = IF ReopenByPathName THEN @ ELSE @ \cup {reader}]
  /\ UNCHANGED <<lock, alive, retiredThrough, priorFrontier, stage,
                 deleted, rewritten, unlinkedBy,
                 deferredOlder, retiredAfterOlder, witnessComplete>>

\* F2, F5. A read handle of p is dropped: its lease leaves the registry and
\* its descriptors close. The step needs no lock; the owner may be gone.
\*   P/retention.rs:729 | state.leases.remove(&self.lease_id);
Release(p, reader) ==
  /\ reader \in leases[p]
  /\ leases' = [leases EXCEPT ![p] = @ \ {reader}]
  /\ pinned' = [pinned EXCEPT ![p] = @ \ {reader}]
  /\ UNCHANGED <<lock, alive, retiredThrough, priorFrontier, stage,
                 deleted, rewritten, unlinkedBy,
                 deferredOlder, retiredAfterOlder, witnessComplete>>

\* F5. The owner p tries to reclaim the first range while a lease of p
\* overlaps it: the registry refuses and reclamation reports the deferral.
\*   P/durable/io.rs:1398 | .retire_pack_range_if_unleased(candidate.old())
\*   P/durable/io.rs:1408 | deferred_for_reader: true,
BlockFirst(p) ==
  /\ lock = p
  /\ stage[p] = 0
  /\ Pending(FirstKey)
  /\ Blocking(p, FirstKey) # {}
  /\ deferredOlder' =
       [deferredOlder EXCEPT ![p] = @ \/ ("older" \in Blocking(p, FirstKey))]
  /\ UNCHANGED <<lock, alive, leases, pinned, retiredThrough, priorFrontier,
                 stage, deleted, rewritten, unlinkedBy,
                 retiredAfterOlder, witnessComplete>>

\* F5. The owner p retires the first range in its registry: no lease of p
\* overlaps it, and the frontier moves up, never down. The leases of another
\* process are not in p's registry and do not defer it. An owner that finds a
\* reclamation pending at startup resumes it through the same registry call.
\*   P/retention.rs:666 | if state.leases.values().any(|candidate| {
\*   P/retention.rs:672 | state.retired_pack_through = state.retired_pack_through.max(key.active_pack);
\*   P/durable/startup.rs:358 | lease.reclaim_reclamation_candidate(candidate, selected)?;
RetireFirst(p) ==
  /\ lock = p
  /\ stage[p] = 0
  /\ Pending(FirstKey)
  /\ Blocking(p, FirstKey) = {}
  /\ stage' = [stage EXCEPT ![p] = 1]
  /\ priorFrontier' = [priorFrontier EXCEPT ![p] = retiredThrough[p]]
  /\ retiredThrough' =
       [retiredThrough EXCEPT ![p] = Max2(@, LastPack(FirstKey))]
  /\ retiredAfterOlder' =
       [retiredAfterOlder EXCEPT ![p] =
          deferredOlder[p] /\ "older" \notin leases[p]]
  /\ UNCHANGED <<lock, alive, leases, pinned, deleted, rewritten, unlinkedBy,
                 deferredOlder, witnessComplete>>

\* F4. Reclamation removes the directory entry of one pack, under the writer
\* lease and after the registry retired the range. It does not touch the bytes
\* of the pack; the RewriteInPlace mutant cuts or overwrites them first.
\*   P/durable/io.rs:1417 | match self.remove_child(&name) {
\*   P/durable/io.rs:1902 | let result = unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) };
Unlink(p, pack) ==
  /\ deleted' = deleted \cup {pack}
  /\ unlinkedBy' = [unlinkedBy EXCEPT ![p] = @ \cup {pack}]
  /\ rewritten' = IF RewriteInPlace THEN rewritten \cup {pack} ELSE rewritten

\* F4. The owner p removes one entry of the first range. An entry that an
\* earlier attempt or an earlier owner removed is skipped:
\*   P/durable/io.rs:1428 | if source.kind() == io::ErrorKind::NotFound => {}
DeleteFirst(p) ==
  /\ lock = p
  /\ stage[p] >= 1
  /\ \E pack \in FirstPacks \ deleted :
       /\ Unlink(p, pack)
       /\ witnessComplete' =
            [witnessComplete EXCEPT ![p] =
               @ \/ (retiredAfterOlder[p] /\
                     FirstPacks \subseteq deleted \cup {pack})]
  /\ UNCHANGED <<lock, alive, leases, pinned, retiredThrough, priorFrontier,
                 stage, deferredOlder, retiredAfterOlder>>

\* F5. The owner p retires the second range once every entry of the first is
\* gone. A process that opened after another process removed the first range
\* starts here, with a registry that never retired the first range.
\*   P/retention.rs:672 | state.retired_pack_through = state.retired_pack_through.max(key.active_pack);
RetireSecond(p) ==
  /\ lock = p
  /\ stage[p] < 2
  /\ FirstPacks \subseteq deleted
  /\ Pending(SecondKey)
  /\ Blocking(p, SecondKey) = {}
  /\ stage' = [stage EXCEPT ![p] = 2]
  /\ priorFrontier' = [priorFrontier EXCEPT ![p] = retiredThrough[p]]
  /\ retiredThrough' =
       [retiredThrough EXCEPT ![p] = Max2(@, LastPack(SecondKey))]
  /\ UNCHANGED <<lock, alive, leases, pinned, deleted, rewritten, unlinkedBy,
                 deferredOlder, retiredAfterOlder, witnessComplete>>

\* F4. The owner p removes one entry of the second range.
\*   P/durable/io.rs:1462 | remove_child_handle(&self.directory.directory, name).map_err(|source| DurableIoError::Io {
DeleteSecond(p) ==
  /\ lock = p
  /\ stage[p] = 2
  /\ \E pack \in SecondPacks \ deleted : Unlink(p, pack)
  /\ UNCHANGED <<lock, alive, leases, pinned, retiredThrough, priorFrontier,
                 stage, deferredOlder, retiredAfterOlder, witnessComplete>>

\* Every action is quantified over the constant sets Procs and Readers, so
\* that TLC labels each state of a trace with the name of its action.
Next ==
  (\E p \in Procs : OpenStore(p)) \/
  (\E p \in Procs : CloseOwner(p)) \/
  (\E p \in Procs : Crash(p)) \/
  (\E p \in Procs : Restart(p)) \/
  (\E p \in Procs : \E reader \in Readers : Acquire(p, reader)) \/
  (\E p \in Procs : \E reader \in Readers : Release(p, reader)) \/
  (\E p \in Procs : BlockFirst(p)) \/
  (\E p \in Procs : RetireFirst(p)) \/
  (\E p \in Procs : DeleteFirst(p)) \/
  (\E p \in Procs : RetireSecond(p)) \/
  (\E p \in Procs : DeleteSecond(p))

Spec == Init /\ [][Next]_vars

\* Typing, and two structural facts: only a handle holds descriptors, and a
\* process removed only entries that are gone. The writer lock is one value,
\* so it has at most one holder (F1):
\*   P/durable/io.rs:927 | pub(crate) fn try_acquire_writer(self) -> Result<DurableWriterLeaseV1, DurableIoError> {
TypeInvariant ==
  /\ lock \in Procs \cup {NoProcess}
  /\ alive \in [Procs -> BOOLEAN]
  /\ leases \in [Procs -> SUBSET Readers]
  /\ pinned \in [Procs -> SUBSET Readers]
  /\ retiredThrough \in [Procs -> 0..4]
  /\ priorFrontier \in [Procs -> 0..4]
  /\ stage \in [Procs -> 0..2]
  /\ deleted \subseteq FirstPacks \cup SecondPacks
  /\ rewritten \subseteq FirstPacks \cup SecondPacks
  /\ unlinkedBy \in [Procs -> SUBSET (FirstPacks \cup SecondPacks)]
  /\ deferredOlder \in [Procs -> BOOLEAN]
  /\ retiredAfterOlder \in [Procs -> BOOLEAN]
  /\ witnessComplete \in [Procs -> BOOLEAN]
  /\ \A p \in Procs : pinned[p] \subseteq leases[p]
  /\ \A p \in Procs : unlinkedBy[p] \subseteq deleted

\* The key constants give the scenario its shape: an older epoch over the
\* packs of the first range, a partial overlap, and disjoint ranges. Catalog
\* epochs can share a pack range, which is why the key carries both:
\*   P/retention.rs:483 | pub struct GenerationLeaseKeyV1 {
PhysicalOverlapTopology ==
  /\ OlderSameKey # FirstKey
  /\ OlderSameKey.index = FirstKey.index
  /\ Packs(OlderSameKey) = FirstPacks
  /\ PartialKey # FirstKey
  /\ Packs(PartialKey) # FirstPacks
  /\ Overlaps(PartialKey, FirstKey)
  /\ ~Overlaps(SecondKey, FirstKey)
  /\ ~Overlaps(OutsideKey, SecondKey)

\* F5. The frontier of a registry never moves down:
\*   P/retention.rs:672 | state.retired_pack_through = state.retired_pack_through.max(key.active_pack);
RetiredFrontierMonotone ==
  \A p \in Procs : priorFrontier[p] <= retiredThrough[p]

\* F5. The frontier of a registry is the last pack of the last range that the
\* registry retired:
\*   P/retention.rs:672 | state.retired_pack_through = state.retired_pack_through.max(key.active_pack);
RetiredFrontierMatchesStage ==
  \A p \in Procs :
    (stage[p] = 0 => retiredThrough[p] = 0) /\
    (stage[p] = 1 => retiredThrough[p] = 2) /\
    (stage[p] = 2 => retiredThrough[p] = 4)

\* F5. No live lease of p overlaps a range that p's registry retired. Within
\* one process this is `FirstPack(KeyOf(reader)) > retiredThrough`. With a
\* second process a lease of p can lie below p's frontier: another process
\* removed that lower range, and p then retired only the range above it.
\*   P/retention.rs:626 | if key.first_live_pack <= state.retired_pack_through {
\*   P/retention.rs:666 | if state.leases.values().any(|candidate| {
LiveLeaseOutsideRetiredFrontier ==
  \A p \in Procs : \A reader \in leases[p] :
    Packs(KeyOf(reader)) \cap RetiredPacks(p) = {}

\* F4, F5. No entry that p removed belongs to a range that a live lease of p
\* covers: reclamation retires the range in the registry before it removes
\* the first entry.
\*   P/durable/io.rs:1398 | .retire_pack_range_if_unleased(candidate.old())
DeletedPackUnleased ==
  \A p \in Procs : \A pack \in unlinkedBy[p] : \A reader \in leases[p] :
    pack \notin Packs(KeyOf(reader))

\* F4, F5. A process removes an entry only inside a range that its own
\* registry retired: the registry call precedes the removal.
\*   P/durable/io.rs:1398 | .retire_pack_range_if_unleased(candidate.old())
\*   P/durable/io.rs:1417 | match self.remove_child(&name) {
DeletionFollowsRetirement ==
  \A p \in Procs :
    /\ (unlinkedBy[p] \cap FirstPacks # {} => stage[p] >= 1)
    /\ (unlinkedBy[p] \cap SecondPacks # {} => stage[p] = 2)
    /\ \A pack \in unlinkedBy[p] : pack <= retiredThrough[p]

\* F5. While p holds a lease of an older epoch over the packs of the first
\* range, p's registry does not retire that range: the overlap test ignores
\* the epoch.
\*   P/retention.rs:667 | candidate.first_live_pack <= key.active_pack &&
OlderEpochPinDefersFirst ==
  \A p \in Procs : "older" \in leases[p] => ~RetiredFirst(p)

\* F5. A lease that overlaps the first range in part defers its retirement
\* too, and it overlaps the second range as well.
\*   P/retention.rs:668 | key.first_live_pack <= candidate.active_pack
PartialOverlapPinDefersFirst ==
  \A p \in Procs : "partial" \in leases[p] => stage[p] = 0

\* F4, F5. While p holds the older-epoch lease, p removes no entry of the
\* first range. The exact-key-only mutant must violate this.
\*   P/durable/io.rs:1408 | deferred_for_reader: true,
OlderEpochPackSafe ==
  \A p \in Procs : "older" \in leases[p] => unlinkedBy[p] \cap FirstPacks = {}

\* F5. Retiring the first range does not close the disjoint successor range:
\* the owner can still hand out a handle on it while its entries exist.
\*   P/retention.rs:626 | if key.first_live_pack <= state.retired_pack_through {
DisjointSuccessorReadRemainsPossible ==
  \A p \in Procs :
    (lock = p /\ stage[p] = 1 /\ "successor" \notin leases[p] /\
     SecondPacks \cap deleted = {})
      => ENABLED Acquire(p, "successor")

\* F5. A pack above every reclaimed range stays readable by the owner.
\*   P/retention.rs:626 | if key.first_live_pack <= state.retired_pack_through {
DisjointPackReadRemainsPossible ==
  \A p \in Procs :
    (lock = p /\ "outside" \notin leases[p]) => ENABLED Acquire(p, "outside")

\* The observers of the older-epoch lifecycle record only what happened in
\* their own process: a deferral, then the retirement, then the last removal.
\*   P/durable/io.rs:1408 | deferred_for_reader: true,
OlderLifecycleHistorySound ==
  \A p \in Procs :
    /\ (retiredAfterOlder[p] => deferredOlder[p] /\ stage[p] >= 1)
    /\ (witnessComplete[p] => retiredAfterOlder[p] /\ FirstPacks \subseteq deleted)

\* Witness target, expected to be violated: some process defers the first
\* range for the older-epoch lease, sees the lease released, retires the range
\* and removes its last entry.
\*   P/durable/io.rs:1408 | deferred_for_reader: true,
OlderLifecycleAbsent == \A p \in Procs : ~witnessComplete[p]

\* F3, F4, K1. Every read that a live handle can issue returns the bytes that
\* were admitted, whatever any process has done to the directory since. The
\* handle reads through descriptors that it holds since admission, reclamation
\* removes entries only, and the kernel keeps an unlinked file for as long as
\* a descriptor on it is open.
\*   P/durable/admission.rs:939 | pinned.file.read_exact_at(offset, &mut bytes)?;
\*   P/durable/io.rs:1902 | let result = unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) };
PinnedReadReturnsAdmittedBytes ==
  \A p \in Procs : \A reader \in leases[p] : \A pack \in Packs(KeyOf(reader)) :
    HandleRead(p, reader, pack) = "admitted"

\* K2. A terminated process holds neither the writer lock nor a lease nor a
\* descriptor: it cannot refuse the next opener, defer a reclamation or keep
\* the storage of a removed pack. The writer lock is a lock on an open file
\* description, which ends when the kernel closes the descriptors of the dead
\* process.
\*   P/durable/io.rs:937 | match lock.file.try_lock() {
DeadProcessHoldsNothing ==
  \A p \in Procs : ~alive[p] => (lock # p /\ leases[p] = {} /\ pinned[p] = {})

=============================================================================

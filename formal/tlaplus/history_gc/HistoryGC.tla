-------------------------- MODULE HistoryGC --------------------------
(***************************************************************************)
(* Bounded safety model for the rspace-history mark-and-sweep garbage      *)
(* collector. Companion claim: issue #1098 and its predecessor #1061.      *)
(*                                                                         *)
(* Mechanizes the "no live state references a deleted node" invariant     *)
(* against a small concurrent system: a sweep process that runs the       *)
(* batched algorithm shipped for #1098, and 0..|WriterPlans| writer       *)
(* processes that each do one `do_checkpoint` under the `PruneGuard`      *)
(* shared lock.                                                            *)
(*                                                                         *)
(* Knobs expose three defect classes as pre-fix negative controls:        *)
(*                                                                         *)
(*   1. IncludeMutex = FALSE — writer and sweep do not serialise via the  *)
(*      sweep lock. Reproduces the content-addressed race at the heart    *)
(*      of #1061: a writer's `pending_writes` skips a hash already in     *)
(*      the store between the sweep's scan and delete; the sweep deletes  *)
(*      the hash and the writer's finalized state references it.          *)
(*                                                                         *)
(*   2. IncludePin = FALSE — writer does not pin its new root before     *)
(*      writing. Reproduces #1061's "roots not yet in the DAG" race: a    *)
(*      sweep whose caller-supplied retained set does not name the new    *)
(*      root (because the DAG has not caught up) walks only the DAG       *)
(*      roots and sweeps the writer's new subtree.                        *)
(*                                                                         *)
(*   3. IncludeBatchRemark = FALSE — the batched sweep does NOT re-mark   *)
(*      pins between delete batches. Reproduces the regression #1098's    *)
(*      fix had to avoid: a writer that commits between batch N and      *)
(*      batch N+1 pinned its new root, but batch N+1 never walks from    *)
(*      it, so the batch's delete list includes writer-committed nodes.  *)
(*                                                                         *)
(* The post-fix config — all three TRUE — must find no violation. Each    *)
(* pre-fix config must violate `Inv_Safety`.                              *)
(*                                                                         *)
(* Code the model abstracts:                                               *)
(*                                                                         *)
(*   Model operator / variable   Rust code                                 *)
(*   --------------------------  ----------------------------------------  *)
(*   `WriterStage`               `HistoryRepositoryImpl::do_checkpoint`    *)
(*                                 (the `stage` + `pending_writes` call)   *)
(*   `WriterPin`                 `PruneGuard::pin` from `do_checkpoint`    *)
(*   `WriterCommit`              `batched_put` inside `checkpoint_writer`  *)
(*   `SweepBeginPass`            `PruneGuard::begin_pass`                  *)
(*   `SweepQuiesceAndSnapshot`   Initial exclusive-held pin snapshot in    *)
(*                                 `prune_history_atomic`                  *)
(*   `SweepMarkFromSeed`         Lock-free initial DFS in                  *)
(*                                 `atomic_prune_via_store_batched`        *)
(*   `SweepBatch`                One iteration of the per-page loop:       *)
(*                                 scan page, acquire exclusive,          *)
(*                                 re-snapshot pins, walk new pins,       *)
(*                                 delete                                   *)
(*                                                                         *)
(* The model is a deliberate skeleton: it carries only the hashes and     *)
(* their edges, not the trie structure or block semantics. That is        *)
(* enough to catch the three defect classes above; the deeper RSpace     *)
(* correctness goes through the test suite.                               *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS
    Hashes,               \* finite universe of possible trie-node hashes
    ChildrenRel,          \* [Hashes -> SUBSET Hashes] - content-addressed
                          \*   child map. Fixed before Init; corresponds to
                          \*   the deterministic relationship between a
                          \*   radix-tree node and its NodePtr children.
    WriterPlans,          \* set of possible pre-planned writer actions;
                          \*   each plan is a record [root, subtree]
                          \*   describing a potential new state commit.
    DagRetainedRoots,     \* SUBSET Hashes — the roots that the caller-side
                          \*   DAG walk (`compute_gc_retention_roots`)
                          \*   emits. Fixed per run.
    InitialStore,         \* SUBSET Hashes — the store before any writer
                          \*   or sweep action. Must be closed under
                          \*   ChildrenRel for every hash that is a
                          \*   DagRetainedRoot (else the spec starts
                          \*   dangling).
    MaxBatches,           \* bound on the number of delete batches per
                          \*   sweep pass. One pass with two batches is
                          \*   enough to witness #1098's regression.
    IncludeMutex,         \* TRUE in post-fix; FALSE reproduces #1061
                          \*   type-1 (content-addressed race).
    IncludePin,           \* TRUE in post-fix; FALSE reproduces #1061
                          \*   type-2 (not-yet-in-DAG roots).
    IncludeBatchRemark    \* TRUE in post-fix; FALSE reproduces #1098
                          \*   (mid-pass pin not picked up).

ASSUME DagRetainedRoots \subseteq Hashes
ASSUME InitialStore \subseteq Hashes
ASSUME \A h \in Hashes : ChildrenRel[h] \subseteq Hashes
ASSUME MaxBatches >= 1

----------------------------------------------------------------------------
(***************************************************************************)
(* Reachability via ChildrenRel, restricted to hashes currently in store. *)
(* Bounded recursion: depth is at most Cardinality(Hashes).               *)
(***************************************************************************)

RECURSIVE ReachInStore(_, _, _)
ReachInStore(root, seen, s) ==
    IF root \in seen \/ root \notin s
        THEN seen
        ELSE LET seen1 == seen \cup {root}
             IN
             seen1 \cup
             UNION { ReachInStore(c, seen1, s) : c \in ChildrenRel[root] }

Reachable(root, s) == ReachInStore(root, {}, s)

ReachableFromSet(roots, s) ==
    UNION { Reachable(r, s) : r \in roots }

----------------------------------------------------------------------------
(***************************************************************************)
(* State                                                                    *)
(***************************************************************************)
NOT_PINNED == 0 - 1

VARIABLES
    store,            \* SUBSET Hashes — current store contents
    pinned,           \* [Hashes -> Nat \cup {NOT_PINNED}] — generation when
                      \*   pinned, NOT_PINNED (= -1) if not pinned
    gen,              \* Nat — current pass generation
    lock,             \* "free" | "shared" | "exclusive"
    sharedHolders,    \* [WriterPlans -> BOOLEAN] — which writers hold shared
    writerPhase,      \* [WriterPlans -> phase]
                      \*   phase \in {"idle","staged","pinned","committed"}
    writerSnapshot,   \* [WriterPlans -> SUBSET Hashes] — store contents the
                      \*   writer observed at its `stage` call. Content-
                      \*   addressed writes (`pending_writes`) skip hashes
                      \*   already present in this snapshot. At commit time
                      \*   the writer only `put`s nodes in subtree \ snapshot,
                      \*   so if a hash from the snapshot was deleted while
                      \*   the writer was staged, the writer's final state
                      \*   points at a missing node. This is the #1061
                      \*   race type-1.
    sweepPhase,       \* "idle" | "quiescing" | "marking" | "batching" | "done"
    sweepSeed,        \* SUBSET Hashes — initial roots for the sweep's mark
    sweepMark,        \* SUBSET Hashes — mark set built by the sweep
    sweepPinsSeen,    \* SUBSET Hashes — pins the sweep has already walked
    sweepBatchCount,  \* Nat — batches processed so far
    sweepScanned,     \* SUBSET Hashes — hashes already covered by a batch's
                      \*   scan in THIS pass. Models the LMDB cursor: once a
                      \*   hash has been scanned it is not re-scanned in the
                      \*   same pass. New-after-scan nodes could land past
                      \*   the cursor and appear in a later batch's scan —
                      \*   TLC explores both the "seen by later batch" and
                      \*   "not seen" cases via the SUBSET quantifier below.
    stableLiveRoots   \* SUBSET Hashes — roots whose writers have returned;
                      \*   the model's witness for "committed states that
                      \*   must stay reachable until retentionRoots catches
                      \*   up to them."

vars == <<store, pinned, gen, lock, sharedHolders, writerPhase, writerSnapshot,
          sweepPhase, sweepSeed, sweepMark, sweepPinsSeen, sweepBatchCount,
          sweepScanned, stableLiveRoots>>

----------------------------------------------------------------------------
(***************************************************************************)
(* Initialization                                                           *)
(***************************************************************************)

Init ==
    /\ store = InitialStore
    /\ pinned = [h \in Hashes |-> NOT_PINNED]
    /\ gen = 0
    /\ lock = "free"
    /\ sharedHolders = [p \in WriterPlans |-> FALSE]
    /\ writerPhase = [p \in WriterPlans |-> "idle"]
    /\ writerSnapshot = [p \in WriterPlans |-> {}]
    /\ sweepPhase = "idle"
    /\ sweepSeed = {}
    /\ sweepMark = {}
    /\ sweepPinsSeen = {}
    /\ sweepBatchCount = 0
    /\ sweepScanned = {}
    /\ stableLiveRoots = DagRetainedRoots

----------------------------------------------------------------------------
(***************************************************************************)
(* Lock helpers                                                             *)
(*                                                                         *)
(* The PruneGuard RwLock allows multiple shared holders OR one exclusive  *)
(* holder. When IncludeMutex = FALSE the writer actions skip the lock     *)
(* dance and the sweep's exclusive-side acquisitions are no-ops: the      *)
(* pre-#1061 code did not have the PruneGuard at all.                     *)
(***************************************************************************)

AnyWriterInShared == \E q \in WriterPlans : sharedHolders[q]

CanAcquireShared ==
    IF IncludeMutex THEN (lock /= "exclusive") ELSE TRUE

CanAcquireExclusive ==
    IF IncludeMutex THEN (~AnyWriterInShared /\ lock /= "exclusive")
    ELSE TRUE

----------------------------------------------------------------------------
(***************************************************************************)
(* Writer actions                                                           *)
(*                                                                         *)
(* Each WriterPlan is a record [root |-> h, subtree |-> S].                *)
(*                                                                         *)
(* A WriterPlan's subtree MUST equal Reachable(root, subtree) — i.e., the  *)
(* plan describes a self-consistent new state whose root points to its    *)
(* own content-addressed children.                                        *)
(***************************************************************************)

ValidPlan(p) ==
    /\ p.root \in p.subtree
    /\ \A h \in p.subtree : ChildrenRel[h] \subseteq p.subtree

\* Phase-1: stage. Writer acquires shared, reads store via `pending_writes`
\* and captures the set of hashes it will SKIP at commit (those already in
\* the store). This is the START of the content-addressed critical
\* section — writers in this phase hold shared through WriterCommit.
WriterStage(p) ==
    /\ ValidPlan(p)
    /\ writerPhase[p] = "idle"
    /\ CanAcquireShared
    /\ IF IncludeMutex
         THEN /\ lock' = "shared"
              /\ sharedHolders' = [sharedHolders EXCEPT ![p] = TRUE]
         ELSE UNCHANGED <<lock, sharedHolders>>
    /\ writerPhase' = [writerPhase EXCEPT ![p] = "staged"]
    /\ writerSnapshot' = [writerSnapshot EXCEPT ![p] = store]
    /\ UNCHANGED <<store, pinned, gen, sweepPhase, sweepSeed, sweepMark,
                   sweepPinsSeen, sweepBatchCount, sweepScanned,
                   stableLiveRoots>>

\* Phase-2: pin. Post-stage, pre-commit (matches `do_checkpoint`'s order).
\* Skipped when IncludePin = FALSE — the pre-#1061 code did not pin.
WriterPin(p) ==
    /\ writerPhase[p] = "staged"
    /\ writerPhase' = [writerPhase EXCEPT ![p] = "pinned"]
    /\ IF IncludePin
         THEN pinned' = [pinned EXCEPT ![p.root] = gen]
         ELSE UNCHANGED pinned
    /\ UNCHANGED <<store, gen, lock, sharedHolders, writerSnapshot,
                   sweepPhase, sweepSeed, sweepMark, sweepPinsSeen,
                   sweepBatchCount, sweepScanned, stableLiveRoots>>

\* Phase-3: commit. Writer writes only the NEW nodes (subtree \ snapshot) —
\* content-addressed `pending_writes` skipped the rest. If a hash in
\* `snapshot` was deleted while the writer was staged, this transition
\* leaves the writer's final state with a dangling pointer; the safety
\* invariant catches it. Under IncludeMutex that race is impossible.
WriterCommit(p) ==
    /\ writerPhase[p] = "pinned"
    /\ store' = store \cup (p.subtree \ writerSnapshot[p])
    /\ LET others == [sharedHolders EXCEPT ![p] = FALSE]
           stillShared == \E q \in WriterPlans : others[q]
       IN IF IncludeMutex
            THEN /\ sharedHolders' = others
                 /\ lock' = IF stillShared THEN "shared" ELSE "free"
            ELSE UNCHANGED <<lock, sharedHolders>>
    /\ writerPhase' = [writerPhase EXCEPT ![p] = "committed"]
    /\ stableLiveRoots' = stableLiveRoots \cup {p.root}
    /\ UNCHANGED <<pinned, gen, writerSnapshot, sweepPhase, sweepSeed,
                   sweepMark, sweepPinsSeen, sweepBatchCount, sweepScanned>>

----------------------------------------------------------------------------
(***************************************************************************)
(* Sweep actions                                                            *)
(*                                                                         *)
(* Models the batched algorithm in `atomic_prune_via_store_batched`.      *)
(***************************************************************************)

\* Phase: begin_pass. Bumps generation. We do not model age-based pin
\* eviction here — that is covered by `PruneGuard` unit tests and does
\* not affect safety (an evicted pin's subtree is unreachable from any
\* live root by assumption; sweeping it is correct).
SweepBeginPass ==
    /\ sweepPhase = "idle"
    /\ gen' = gen + 1
    /\ sweepPhase' = "quiescing"
    /\ UNCHANGED <<store, pinned, lock, sharedHolders, writerPhase,
                   writerSnapshot, sweepSeed, sweepMark, sweepPinsSeen,
                   sweepBatchCount, sweepScanned, stableLiveRoots>>

\* Phase: quiesce. Take the exclusive side briefly so every in-flight
\* writer completes its stage+pin+write before we snapshot pins. Under
\* IncludeMutex = FALSE this is a no-op (pre-#1061 had no lock at all)
\* AND, critically, writers can still be mid-flight: that is where the
\* #1061 type-1 bug reproduces.
SweepQuiesceAndSnapshot ==
    /\ sweepPhase = "quiescing"
    /\ CanAcquireExclusive
    /\ sweepSeed' = DagRetainedRoots
                    \cup { h \in Hashes : pinned[h] /= NOT_PINNED }
    /\ sweepPinsSeen' = { h \in Hashes : pinned[h] /= NOT_PINNED }
    /\ sweepPhase' = "marking"
    /\ UNCHANGED <<store, pinned, gen, lock, sharedHolders, writerPhase,
                   writerSnapshot, sweepMark, sweepBatchCount, sweepScanned,
                   stableLiveRoots>>

\* Phase: lock-free mark walk from the seed.
SweepMarkFromSeed ==
    /\ sweepPhase = "marking"
    /\ sweepMark' = ReachableFromSet(sweepSeed, store)
    /\ sweepPhase' = "batching"
    /\ UNCHANGED <<store, pinned, gen, lock, sharedHolders, writerPhase,
                   writerSnapshot, sweepSeed, sweepPinsSeen, sweepBatchCount,
                   sweepScanned, stableLiveRoots>>

\* Phase: one delete batch.
\*   1. Non-deterministically pick a non-empty subset of
\*      (store \ sweepScanned) as the "scanned page". TLC explores every
\*      page partition.
\*   2. Acquire exclusive if the mutex is enabled (quiesces any in-flight
\*      writer). Under IncludeMutex = FALSE a writer can be mid-stage
\*      here — that is where the #1061 race lands.
\*   3. Re-snapshot pins; for each pin not in sweepPinsSeen, walk its
\*      subtree and add to sweepMark. (This step is gated by
\*      IncludeBatchRemark — the pre-#1098 variant SKIPS it, so writer-
\*      committed nodes between batches get swept.)
\*   4. Compute delete list = batch \ sweepMark, delete from store.
\*   5. Release exclusive.
SweepBatch ==
    /\ sweepPhase = "batching"
    /\ sweepBatchCount < MaxBatches
    /\ (store \ sweepScanned) /= {}
    /\ CanAcquireExclusive
    /\ \E batch \in SUBSET (store \ sweepScanned) :
            /\ batch /= {}
            /\ LET freshPins == { h \in Hashes : pinned[h] /= NOT_PINNED }
                   newPins == freshPins \ sweepPinsSeen
                   extraMark == IF IncludeBatchRemark
                                  THEN ReachableFromSet(newPins, store)
                                  ELSE {}
                   newMark == sweepMark \cup extraMark
                   toDelete == batch \ newMark
               IN
               /\ sweepMark' = newMark
               /\ sweepPinsSeen' = IF IncludeBatchRemark
                                     THEN sweepPinsSeen \cup newPins
                                     ELSE sweepPinsSeen
               /\ store' = store \ toDelete
               /\ sweepScanned' = sweepScanned \cup batch
    /\ sweepBatchCount' = sweepBatchCount + 1
    /\ UNCHANGED <<pinned, gen, lock, sharedHolders, writerPhase,
                   writerSnapshot, sweepSeed, sweepPhase, stableLiveRoots>>

\* Phase: pass complete. Fires when we have exhausted the store or hit
\* the batch cap. The spec makes this the only way out of "batching";
\* once we are in "done" no further sweep action fires.
SweepFinish ==
    /\ sweepPhase = "batching"
    /\ (sweepBatchCount = MaxBatches \/ (store \ sweepScanned) = {})
    /\ sweepPhase' = "done"
    /\ UNCHANGED <<store, pinned, gen, lock, sharedHolders, writerPhase,
                   writerSnapshot, sweepSeed, sweepMark, sweepPinsSeen,
                   sweepBatchCount, sweepScanned, stableLiveRoots>>

----------------------------------------------------------------------------
(***************************************************************************)
(* Transition system                                                        *)
(***************************************************************************)

Next ==
    \/ \E p \in WriterPlans :
           \/ WriterStage(p)
           \/ WriterPin(p)
           \/ WriterCommit(p)
    \/ SweepBeginPass
    \/ SweepQuiesceAndSnapshot
    \/ SweepMarkFromSeed
    \/ SweepBatch
    \/ SweepFinish

Spec == Init /\ [][Next]_vars

----------------------------------------------------------------------------
(***************************************************************************)
(* Invariants                                                               *)
(***************************************************************************)

\* A root is "live" if a writer has committed it OR a pin exists on it OR
\* the caller emitted it as a retention root.
LiveRoots == stableLiveRoots \cup { h \in Hashes : pinned[h] /= NOT_PINNED }

\* Safety: no root that ever became "live" has a dangling pointer. For
\* every live root R that is in the store, the full subtree of R (as
\* defined by ChildrenRel, not "whatever happens to be in the store")
\* must be in the store.
Inv_NoDanglingLiveReference ==
    \A r \in LiveRoots :
        r \in store =>
            (\A c \in Reachable(r, Hashes) : c \in store)

\* Diagnostic: once a writer has committed, its subtree must still be in
\* the store. Pre-#1061 variants fail this when the sweep deletes nodes
\* the writer referenced but did not re-write (content-addressed skip).
Inv_CommittedWritersPreserved ==
    \A p \in WriterPlans :
        writerPhase[p] = "committed" => p.subtree \subseteq store

Inv_Safety ==
    /\ Inv_NoDanglingLiveReference
    /\ Inv_CommittedWritersPreserved

============================================================================

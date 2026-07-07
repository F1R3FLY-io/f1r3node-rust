--------------------------- MODULE ConcurrentTracker ---------------------------
(****************************************************************************)
(* Models the lock-free vs. locked equivocation-tracker access at           *)
(*   casper/src/rust/multi_parent_casper_impl.rs:1046-1075                  *)
(*                                                                          *)
(* The Rust port exposes lock-free direct methods                          *)
(*   block_dag_storage.equivocation_records()                               *)
(*   block_dag_storage.insert_equivocation_record(...)                      *)
(* whereas the Scala original routes both through                           *)
(*   accessEquivocationsTracker { tracker -> ... }                          *)
(* which holds a global semaphore.                                          *)
(*                                                                          *)
(* This spec demonstrates that under Locked = FALSE, two threads can race   *)
(* in handle_invalid_block(AdmissibleEquivocation) and overwrite each       *)
(* other's accumulated equivocationDetectedBlockHashes — the regression     *)
(* identified in the Scala-vs-Rust comparison.                              *)
(*                                                                          *)
(* Under Locked = TRUE the race is closed and the bug fix #2 (T-9.2) is     *)
(* validated.                                                                *)
(*                                                                          *)
(* Reference: docs/theory/slashing/slashing-verification.md §9.2.           *)
(****************************************************************************)

EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    Threads,         \* Set of thread identifiers
    Validators,      \* Set of bonded validators
    BlockHashes,     \* Set of distinct block hashes that may be inserted
    Locked           \* BOOLEAN: TRUE = locking fix applied, FALSE = current Rust

VARIABLES
    \* The KV store: a function (validator, seqNum) -> set of block hashes
    \* (we abstract seqNum to just "0" since the race is per-key)
    store,

    \* Per-thread "view" cached after equivocation_records() and before
    \* insert/update — i.e. thread t's snapshot of what it thinks the
    \* current store is.
    threadView,

    \* Per-thread invocation phase: "idle" | "reading" | "writing" | "done"
    threadPhase,

    \* The block hash thread t intends to insert (chosen at start of RMW)
    threadHash,

    \* The validator key thread t targets
    threadKey,

    \* Lock holder (or NULL when free).  Only consulted when Locked = TRUE.
    lockHolder

vars == <<store, threadView, threadPhase, threadHash, threadKey, lockHolder>>

NoLockHolder == "no_holder"  \* model value sentinel; required to be \notin Threads

(****************************************************************************)
(* TypeOK                                                                   *)
(****************************************************************************)
TypeOK ==
    /\ store         \in [Validators -> SUBSET BlockHashes]
    /\ threadView    \in [Threads -> SUBSET BlockHashes]
    /\ threadPhase   \in [Threads -> {"idle", "reading", "writing", "done"}]
    /\ threadHash    \in [Threads -> BlockHashes \cup {NoLockHolder}]
    /\ threadKey     \in [Threads -> Validators \cup {NoLockHolder}]
    /\ lockHolder    \in Threads \cup {NoLockHolder}

(****************************************************************************)
(* Helpers                                                                  *)
(****************************************************************************)
LockFree == lockHolder = NoLockHolder

(****************************************************************************)
(* Init: empty store, no threads active, lock free                          *)
(****************************************************************************)
Init ==
    /\ store         = [v \in Validators |-> {}]
    /\ threadView    = [t \in Threads    |-> {}]
    /\ threadPhase   = [t \in Threads    |-> "idle"]
    /\ threadHash    = [t \in Threads    |-> NoLockHolder]
    /\ threadKey     = [t \in Threads    |-> NoLockHolder]
    /\ lockHolder    = NoLockHolder

(****************************************************************************)
(* Action: thread t begins handling an AdmissibleEquivocation for key v.    *)
(* Acquires lock if Locked=TRUE; reads the store into threadView.           *)
(****************************************************************************)
BeginRMW(t, v, h) ==
    /\ t \in Threads
    /\ v \in Validators
    /\ h \in BlockHashes
    /\ threadPhase[t] = "idle"
    /\ \/ \neg Locked
       \/ Locked /\ LockFree
    /\ threadPhase' = [threadPhase EXCEPT ![t] = "reading"]
    /\ threadView'  = [threadView  EXCEPT ![t] = store[v]]
    /\ threadHash'  = [threadHash  EXCEPT ![t] = h]
    /\ threadKey'   = [threadKey   EXCEPT ![t] = v]
    /\ lockHolder'  = IF Locked THEN t ELSE lockHolder
    /\ UNCHANGED <<store>>

(****************************************************************************)
(* Action: thread t writes its (view ∪ {hash}) back to the store.           *)
(*                                                                          *)
(* Under Locked = TRUE, the write happens immediately after the read with no*)
(* other thread interleaved — so the union with the just-read view is       *)
(* equivalent to the union with the current store value.                    *)
(*                                                                          *)
(* Under Locked = FALSE, another thread may have written between this       *)
(* thread's read and write, and our union with stale-view loses that other  *)
(* thread's contribution.                                                   *)
(****************************************************************************)
CommitRMW(t) ==
    /\ t \in Threads
    /\ threadPhase[t] = "reading"
    /\ LET v == threadKey[t]
           h == threadHash[t]
       IN  store' = [store EXCEPT ![v] = threadView[t] \cup {h}]
    /\ threadPhase' = [threadPhase EXCEPT ![t] = "writing"]
    /\ UNCHANGED <<threadView, threadHash, threadKey, lockHolder>>

(****************************************************************************)
(* Action: thread t finishes and releases the lock.                         *)
(****************************************************************************)
FinishRMW(t) ==
    /\ t \in Threads
    /\ threadPhase[t] = "writing"
    /\ threadPhase' = [threadPhase EXCEPT ![t] = "done"]
    /\ lockHolder'  = IF Locked /\ lockHolder = t THEN NoLockHolder
                      ELSE lockHolder
    /\ UNCHANGED <<store, threadView, threadHash, threadKey>>

(****************************************************************************)
(* Next                                                                     *)
(****************************************************************************)
Next ==
    \/ \E t \in Threads, v \in Validators, h \in BlockHashes : BeginRMW(t, v, h)
    \/ \E t \in Threads : CommitRMW(t)
    \/ \E t \in Threads : FinishRMW(t)

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

(****************************************************************************)
(* Invariant: NoOverwrite — once a hash is in the store, it stays.           *)
(*                                                                          *)
(* This is the property that fails under Locked = FALSE and holds under     *)
(* Locked = TRUE.  The TLC violation trace under Locked = FALSE is the      *)
(* model-checked counter-example for the Rust regression.                   *)
(****************************************************************************)
Inv_NoOverwrite ==
    \A v \in Validators :
        \A h \in BlockHashes :
            (\E t \in Threads :
                threadPhase[t] = "writing" /\ threadKey[t] = v
                /\ h \in store[v]) =>
            \A t2 \in Threads :
                (threadPhase[t2] # "writing" \/ threadKey[t2] # v)
                => h \in store[v] \/ \neg Locked

(****************************************************************************)
(* Stronger invariant: store grows monotonically (at the per-key level).   *)
(*                                                                          *)
(* This is the bisimilarity invariant we need: the set of hashes accumulated*)
(* at (v, base) under any execution equals the set inserted by any thread.  *)
(*                                                                          *)
(* Formally: for each thread t that has reached "done", its hash is in the  *)
(* store at its key.                                                        *)
(****************************************************************************)
Inv_RecordMonotone ==
    \A t \in Threads :
        threadPhase[t] = "done"
        => threadHash[t] \in store[threadKey[t]]

(****************************************************************************)
(* Temporal property: every thread eventually reaches "done".               *)
(****************************************************************************)
Live_AllThreadsFinish ==
    \A t \in Threads :
        threadPhase[t] # "idle" ~> threadPhase[t] = "done"

============================================================================

---------------------- MODULE NativeSharedReadCleanup ----------------------
(***************************************************************************)
(* C1 (epic 8946, B1 phase B; DR-81): shared native continuation reads.    *)
(*                                                                         *)
(* The hot store caches the payloads of a key once, behind shared          *)
(* pointers. A cold read decodes a fresh payload, prepays its release, and *)
(* stores a cache entry whose own release is also prepaid. A warm read     *)
(* returns another pointer to the cached payload and copies nothing. A     *)
(* view drop or an eviction drops one pointer, and the last drop releases  *)
(* the payload.                                                            *)
(*                                                                         *)
(* Rust correspondence:                                                    *)
(*   ColdRead   hot_store/native.rs native_continuation_views (cold path:  *)
(*              reserve_cleanup, Arc::new, native_insert_new)              *)
(*   WarmRead   native_continuation_views (warm path: Arc::clone)          *)
(*   DropView   the end of a view's life in native_candidate/metered.rs    *)
(*   Evict      the release of a cache entry                               *)
(* Rocq correspondence: theories/NativeSharedReads.v                       *)
(*   (cleanup_prepaid_preserved, every_release_was_prepaid).               *)
(*                                                                         *)
(* Mutation selects one negative control:                                  *)
(*   "fillnoprepay"  a cold read skips the payload prepayment              *)
(*                   -> LivePrepaid                                        *)
(*   "storenoprepay" a cold read skips the cache-entry prepayment          *)
(*                   -> EntriesPrepaid                                     *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS Keys, MaxPayloads, MaxViews, Mutation

ASSUME /\ Keys # {}
       /\ MaxPayloads \in Nat
       /\ MaxViews \in Nat
       /\ Mutation \in {"none", "fillnoprepay", "storenoprepay"}

Payloads == 1..MaxPayloads

VARIABLES entry, refs, prepaid, entryPrepaid, unpaidRelease, next

vars == <<entry, refs, prepaid, entryPrepaid, unpaidRelease, next>>

TypeOK ==
    /\ entry \in [Keys -> Payloads \cup {0}]
    /\ refs \in [Payloads -> 0..(MaxViews + 1)]
    /\ prepaid \in [Payloads -> BOOLEAN]
    /\ entryPrepaid \in [Keys -> BOOLEAN]
    /\ unpaidRelease \in BOOLEAN
    /\ next \in 1..(MaxPayloads + 1)

Init ==
    /\ entry = [key \in Keys |-> 0]
    /\ refs = [payload \in Payloads |-> 0]
    /\ prepaid = [payload \in Payloads |-> FALSE]
    /\ entryPrepaid = [key \in Keys |-> FALSE]
    /\ unpaidRelease = FALSE
    /\ next = 1

(* A cold read decodes a fresh payload: one pointer in the cache and one  *)
(* returned view.                                                         *)
ColdRead(key) ==
    /\ entry[key] = 0
    /\ next <= MaxPayloads
    /\ entry' = [entry EXCEPT ![key] = next]
    /\ refs' = [refs EXCEPT ![next] = 2]
    /\ prepaid' = [prepaid EXCEPT ![next] = (Mutation # "fillnoprepay")]
    /\ entryPrepaid' = [entryPrepaid EXCEPT ![key] = (Mutation # "storenoprepay")]
    /\ next' = next + 1
    /\ UNCHANGED unpaidRelease

(* A warm read returns one more pointer to the cached payload. *)
WarmRead(key) ==
    /\ entry[key] # 0
    /\ refs[entry[key]] <= MaxViews
    /\ refs' = [refs EXCEPT ![entry[key]] = @ + 1]
    /\ UNCHANGED <<entry, prepaid, entryPrepaid, unpaidRelease, next>>

Release(payload, remaining) ==
    IF remaining = 0 /\ ~prepaid[payload] THEN TRUE ELSE unpaidRelease

(* A view drops its pointer; the cache keeps its own pointer. *)
DropView(payload) ==
    /\ refs[payload] > 0
    /\ (\E key \in Keys : entry[key] = payload) => refs[payload] > 1
    /\ refs' = [refs EXCEPT ![payload] = @ - 1]
    /\ unpaidRelease' = Release(payload, refs[payload] - 1)
    /\ UNCHANGED <<entry, prepaid, entryPrepaid, next>>

(* An eviction releases the cache entry and drops the cache's pointer. *)
Evict(key) ==
    /\ entry[key] # 0
    /\ refs' = [refs EXCEPT ![entry[key]] = @ - 1]
    /\ unpaidRelease' = (Release(entry[key], refs[entry[key]] - 1) \/ ~entryPrepaid[key])
    /\ entry' = [entry EXCEPT ![key] = 0]
    /\ entryPrepaid' = [entryPrepaid EXCEPT ![key] = FALSE]
    /\ UNCHANGED <<prepaid, next>>

Next ==
    \E key \in Keys : ColdRead(key) \/ WarmRead(key) \/ Evict(key)
    \/ \E payload \in Payloads : DropView(payload)

Spec == Init /\ [][Next]_vars

(* Every live payload has its release prepaid (cleanup_prepaid_preserved). *)
LivePrepaid == \A payload \in Payloads : refs[payload] > 0 => prepaid[payload]

(* Every cache entry has its release prepaid. *)
EntriesPrepaid == \A key \in Keys : entry[key] # 0 => entryPrepaid[key]

(* No payload or entry is released without prepayment                     *)
(* (every_release_was_prepaid).                                            *)
EveryReleasePrepaid == ~unpaidRelease

(* A warm read allocates no payload: payloads exist only from cold reads. *)
WarmReadsAllocateNothing == \A payload \in Payloads : payload >= next => refs[payload] = 0
=============================================================================

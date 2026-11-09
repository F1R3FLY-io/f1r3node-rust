--------------------------- MODULE JoinScopeLock ---------------------------
(***************************************************************************)
(* The lock of a join scope (decision 94).                                *)
(*                                                                         *)
(* A collection operation of a native space classifies itself and reads   *)
(* its join scope: the closure of its own collections under the join      *)
(* groups of the space. The scope is space state, and racing evaluation   *)
(* (DR-132) runs the jobs of one space at the same time. The code before  *)
(* decision 94 read the scope in a first transaction, released it, and    *)
(* began a second transaction over the lock keys of the scope. Another    *)
(* job of the space could commit between the two transactions, so the     *)
(* scope could be stale ("a prefix is outside its join scope", "a join    *)
(* group holds a collection outside its scope", UnlockedChannel).         *)
(*                                                                         *)
(* Decision 94 takes the form of the host produce (produce_lock in dev    *)
(* and in the base, with DR-132 F1): phase A on the own keys of the       *)
(* operation, which hold the config record C(sid); the classification and *)
(* the scope read under phase A; then phase B on the own keys and the     *)
(* keys of the scope, in one sorted call. Every transaction of a space    *)
(* locks C(sid), so the phase A lock on C(sid) blocks every other         *)
(* transaction of the space from the scope read to the release. Staging   *)
(* checks each touched channel against the exact phase B keys, so no      *)
(* outcome depends on the lock stripes.                                   *)
(*                                                                         *)
(* The model runs two transactions of one space and one host produce. The *)
(* stripe map is any map of the channels to 1..NumStripes: Init chooses   *)
(* it, and no action changes it. So every result holds for every hash of  *)
(* the channels, and the stripes of the space channels can collide with   *)
(* the stripes of the host channels.                                      *)
(*                                                                         *)
(* Invariants:                                                             *)
(*   NoStaleScope      a transaction stages only with the scope of the    *)
(*                     current state.                                     *)
(*   NoFalseRefusal    the coverage check passes for every channel that a *)
(*                     transaction with a current scope touches.          *)
(*   NoDeadlock        every state has a process that can step, or every  *)
(*                     process ended (an explicit CanStep, as in           *)
(*                     NativeReplay.tla).                                 *)
(*   CanStepIsEnabled  CanStep(i) is the enabledness of Step(i), so        *)
(*                     NoDeadlock is not vacuous.                         *)
(*   MutualExclusion   two processes that hold all their locks touch      *)
(*                     disjoint channels.                                 *)
(*                                                                         *)
(* Unsafe controls (each refutes exactly its invariant):                  *)
(*   EscalationRebeginUnsafe   Escalation = "Rebegin": the code before    *)
(*                             decision 94. Refutes NoStaleScope.         *)
(*   PhaseAWithoutConfigUnsafe ConfigInPhaseA = FALSE: phase A holds only *)
(*                             the own collections. Refutes NoStaleScope. *)
(*   PhaseBSplitUnsafe         Escalation = "SplitPhaseB": phase B in two *)
(*                             calls. Refutes NoDeadlock.                 *)
(*   PhaseAAfterPhaseBUnsafe   Escalation = "PhaseAAfterB": a wait for    *)
(*                             phase A while the process holds phase B.   *)
(*                             Refutes NoDeadlock.                        *)
(*   CoverageByHostUseUnsafe   CoverageByPhaseB = FALSE: the host rule of *)
(*                             the code before decision 94, where a       *)
(*                             consume needs both phases. Refutes         *)
(*                             NoFalseRefusal.                            *)
(*                                                                         *)
(* Code (rs = rspace++/src/rspace, sp = rholang/src/rust/interpreter/     *)
(* spaces):                                                                *)
(*   stripes, phases   rs/striped_locks.rs:41-82 (acquire_locks sorts and *)
(*                     deduplicates the stripe indices and takes them in  *)
(*                     ascending order; consume_lock takes phase A, then  *)
(*                     phase B)                                           *)
(*   host produce      rs/rspace/locks.rs:33-66 (phase A on the channel,  *)
(*                     the join read, phase B on the join channels and    *)
(*                     the channel, DR-132 F1);                           *)
(*                     rs/replay_rspace/native_session/locks.rs:77-103    *)
(*                     (phase B on the join channels). The channel of h   *)
(*                     is in its join group, so the two forms take the    *)
(*                     same stripes.                                      *)
(*   phase A           SpaceTxnHost::begin_phase_a: phase A of the own    *)
(*                     lock keys (rs/space_store_handle.rs:482-486;       *)
(*                     rs/rspace/space_play.rs:416,                       *)
(*                     rs/replay_rspace/space_replay.rs:607 and 649,      *)
(*                     rs/replay_rspace/native_session/space.rs:489);     *)
(*                     SpaceEnv::begin_phase_a (sp/agent.rs:147-155)      *)
(*   own keys          sp/generic.rs:546-555 (CollectionChannels: C(sid), *)
(*                     S(sid), S(sid,k), D(sid,k)), sp/generic.rs:2023-   *)
(*                     2041 (begin_collection), sp/generic.rs:2281-2311   *)
(*                     (begin_join)                                       *)
(*   scope keys        sp/join.rs:422-526 (join_scope: C(sid), S(sid),    *)
(*                     then the channels of each collection of the        *)
(*                     closure), sp/pathmap/join.rs:536-575 (the prefix   *)
(*                     scope: C(sid), S(sid), D(p), I(q))                 *)
(*   phase B           sp/generic.rs:2095-2111 (prepare_collection_op:    *)
(*                     the classification under phase A, then             *)
(*                     SpaceEnv::lock_phase_b with the scope keys);       *)
(*                     SpaceTxn::lock_phase_b (rs/space_store_handle.rs:  *)
(*                     804, 3486-3501): phase B of the own keys and the   *)
(*                     scope keys in one sorted call. The default         *)
(*                     SpaceTxnHost::begin (:467-475) takes both phases   *)
(*                     of the own keys.                                   *)
(*   coverage          rs/space_store_handle.rs:1050-1072 (LockCoverage:  *)
(*                     the exact phase B keys), :1919-1933 (admit_group,  *)
(*                     which stage_consume calls), :2140 and :2173        *)
(*                     (stage_produce: the channel and the joined         *)
(*                     channels)                                          *)
(*                                                                         *)
(* Abstractions:                                                           *)
(*   - The channel Cfg stands for C(sid) and S(sid): every transaction of  *)
(*     the space locks both. A collection Dk stands for S(sid,k) and      *)
(*     D(sid,k), which the lock keys always hold together.                *)
(*   - The join groups of the space are links between two collections: a *)
(*     parked waiter of a join over them. t1 is a produce on D1, which    *)
(*     fires the waiter of the link {D1, D2} when the link exists. t2 is  *)
(*     a join receive over D2 and D3, which parks and adds the link       *)
(*     {D2, D3}. The space starts with the link {D1, D2}. So t2 can grow  *)
(*     the scope of t1, and t1 can shrink the scope of t2.               *)
(*   - A transaction touches Cfg, its own collections and every           *)
(*     collection of its scope, each as a consume (the counter bump, the  *)
(*     fire, and the lift and re-offer of the cascade). This is the most  *)
(*     that the join engine can touch.                                    *)
(*   - The engine reads the join groups of the store while it stages. A   *)
(*     group outside the scope ends the transaction (Inconsistent). The   *)
(*     model checks this first, as err = "stale", and then the coverage   *)
(*     of the touched channels, as err = "unlocked". Either error ends    *)
(*     the transaction with no effect and releases its locks.            *)
(*   - The host produce h on h1 has the join group {h1, h2}, which no     *)
(*     action changes. Host channels and space channels are disjoint: a   *)
(*     host operation never touches a storage channel of a space.         *)
(*   - A lock is one stripe of one phase. A process takes the stripes of  *)
(*     one call one at a time, the smallest first, and waits while the    *)
(*     next stripe has a holder, itself included: a tokio mutex is not    *)
(*     reentrant.                                                         *)
(*   - Classification, staging and the host run are atomic steps. A       *)
(*     process ends in one step after its staging step.                   *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS
    \* "TwoPhase" (decision 94), "Rebegin" (the code before decision 94),
    \* "SplitPhaseB" or "PhaseAAfterB" (Unsafe controls).
    Escalation,
    \* TRUE: phase A holds C(sid) with the own collections. FALSE: an
    \* Unsafe control.
    ConfigInPhaseA,
    \* TRUE: staging checks every touched channel against the exact phase B
    \* keys. FALSE: the host rule by phase (an Unsafe control).
    CoverageByPhaseB,
    \* The stripes are 1..NumStripes.
    NumStripes

ASSUME Escalation \in {"TwoPhase", "Rebegin", "SplitPhaseB", "PhaseAAfterB"}
ASSUME ConfigInPhaseA \in BOOLEAN
ASSUME CoverageByPhaseB \in BOOLEAN
ASSUME NumStripes \in 1..4

-----------------------------------------------------------------------------
(* Channels, processes and stripes                                          *)

Cfg == "Cfg"
Colls == {"D1", "D2", "D3"}
SpaceChans == {Cfg} \cup Colls
HostChans == {"h1", "h2"}
Chans == SpaceChans \cup HostChans
Stripes == 1..NumStripes

SpaceTxns == {"t1", "t2"}
Host == "h"
Procs == SpaceTxns \cup {Host}
NoProc == "none"

\* The links that a waiter of a join over two collections makes.
Links == {{a, b} : a, b \in Colls} \ {{a} : a \in Colls}

\* The own collections of each transaction.
Own(i) == IF i = "t1" THEN {"D1"} ELSE {"D2", "D3"}

\* The own keys of each transaction: its phase A keys.
OwnKeys(i) == (IF ConfigInPhaseA THEN {Cfg} ELSE {}) \cup Own(i)

\* The join group of the host channel h1.
HostJoin == {"h1", "h2"}

\* One closure step: the collections that a link joins to S.
Reach(S, L) == S \cup {d \in Colls : \E l \in L : d \in l /\ l \cap S /= {}}

\* The closure of S under the links L. Three collections reach the fixed
\* point in two steps.
Closure(S, L) == Reach(Reach(S, L), L)

\* The lock keys of a scope: C(sid) and S(sid), then each collection.
ScopeKeys(scope) == {Cfg} \cup scope

Min(S) == CHOOSE s \in S : \A t \in S : s <= t

-----------------------------------------------------------------------------
VARIABLES
    \* The program counter of each process.
    pc,
    \* The stripe of each channel: chosen in Init, never changed.
    stripe,
    \* The holder of each stripe of each phase, or NoProc.
    ownerA,
    ownerB,
    \* The stripes that the current lock call of each process still takes.
    wantA,
    wantB,
    \* The exact keys of each phase that each process holds.
    keysA,
    keysB,
    \* The join groups of the space.
    links,
    \* The scope that each transaction read in its classification.
    scope,
    \* The error of each transaction: "none", "stale" or "unlocked".
    err

vars == <<pc, stripe, ownerA, ownerB, wantA, wantB, keysA, keysB, links, scope, err>>

StripesOf(K) == {stripe[c] : c \in K}

\* The stripes of a phase that process i holds.
HeldA(i) == {s \in Stripes : ownerA[s] = i}
HeldB(i) == {s \in Stripes : ownerB[s] = i}

-----------------------------------------------------------------------------
(* Locks                                                                    *)

\* Take the next stripe of the current phase-A call of i, when it is free.
TakeA(i) ==
    /\ wantA[i] /= {}
    /\ ownerA[Min(wantA[i])] = NoProc
    /\ ownerA' = [ownerA EXCEPT ![Min(wantA[i])] = i]
    /\ wantA' = [wantA EXCEPT ![i] = wantA[i] \ {Min(wantA[i])}]

TakeB(i) ==
    /\ wantB[i] /= {}
    /\ ownerB[Min(wantB[i])] = NoProc
    /\ ownerB' = [ownerB EXCEPT ![Min(wantB[i])] = i]
    /\ wantB' = [wantB EXCEPT ![i] = wantB[i] \ {Min(wantB[i])}]

\* Release every lock of i.
ReleaseAll(i) ==
    /\ ownerA' = [s \in Stripes |-> IF ownerA[s] = i THEN NoProc ELSE ownerA[s]]
    /\ ownerB' = [s \in Stripes |-> IF ownerB[s] = i THEN NoProc ELSE ownerB[s]]
    /\ keysA' = [keysA EXCEPT ![i] = {}]
    /\ keysB' = [keysB EXCEPT ![i] = {}]

-----------------------------------------------------------------------------
(* The transactions of the space                                            *)

\* The begin: the phase A call over the own keys. Decision 94 takes only
\* phase A. The code before it (and the controls of the escalation) takes
\* both phases of the own keys (consume_lock).
Start(i) ==
    /\ pc[i] = "idle"
    /\ pc' = [pc EXCEPT ![i] = "lockA0"]
    /\ wantA' = [wantA EXCEPT ![i] = StripesOf(OwnKeys(i))]
    /\ keysA' = [keysA EXCEPT ![i] = OwnKeys(i)]
    /\ UNCHANGED <<stripe, ownerA, ownerB, wantB, keysB, links, scope, err>>

LockA0(i) ==
    /\ pc[i] = "lockA0"
    /\ \/ /\ TakeA(i)
          /\ UNCHANGED <<pc, wantB, keysB>>
       \/ /\ wantA[i] = {}
          /\ IF Escalation = "TwoPhase"
             THEN /\ pc' = [pc EXCEPT ![i] = "classify"]
                  /\ UNCHANGED <<wantB, keysB>>
             ELSE /\ pc' = [pc EXCEPT ![i] = "lockB0"]
                  /\ wantB' = [wantB EXCEPT ![i] = StripesOf(OwnKeys(i))]
                  /\ keysB' = [keysB EXCEPT ![i] = OwnKeys(i)]
          /\ UNCHANGED <<ownerA, wantA>>
    /\ UNCHANGED <<stripe, ownerB, keysA, links, scope, err>>

LockB0(i) ==
    /\ pc[i] = "lockB0"
    /\ \/ /\ TakeB(i)
          /\ UNCHANGED pc
       \/ /\ wantB[i] = {}
          /\ pc' = [pc EXCEPT ![i] = "classify"]
          /\ UNCHANGED <<ownerB, wantB>>
    /\ UNCHANGED <<stripe, ownerA, wantA, keysA, keysB, links, scope, err>>

\* The classification reads the scope from the current state. Decision 94
\* then takes phase B over the own keys and the scope keys in one call. The
\* other forms run in the first transaction when the scope is within the
\* own collections (scope.covers), and otherwise escalate.
Classify(i) ==
    /\ pc[i] = "classify"
    /\ LET read == Closure(Own(i), links)
           extra == ScopeKeys(read) \ OwnKeys(i)
       IN /\ scope' = [scope EXCEPT ![i] = read]
          /\ IF Escalation = "TwoPhase"
             THEN /\ pc' = [pc EXCEPT ![i] = "lockB"]
                  /\ wantB' = [wantB EXCEPT ![i] = StripesOf(OwnKeys(i) \cup ScopeKeys(read))]
                  /\ keysB' = [keysB EXCEPT ![i] = OwnKeys(i) \cup ScopeKeys(read)]
                  /\ UNCHANGED <<ownerA, ownerB, wantA, keysA>>
             ELSE IF read \subseteq Own(i)
             THEN /\ pc' = [pc EXCEPT ![i] = "stage"]
                  /\ UNCHANGED <<ownerA, ownerB, wantA, wantB, keysA, keysB>>
             ELSE IF Escalation = "Rebegin"
             THEN \* drop(txn); env.begin(scope.lock_keys()).
                  /\ ownerA' = [s \in Stripes |-> IF ownerA[s] = i THEN NoProc ELSE ownerA[s]]
                  /\ ownerB' = [s \in Stripes |-> IF ownerB[s] = i THEN NoProc ELSE ownerB[s]]
                  /\ pc' = [pc EXCEPT ![i] = "relockA"]
                  /\ wantA' = [wantA EXCEPT ![i] = StripesOf(ScopeKeys(read))]
                  /\ keysA' = [keysA EXCEPT ![i] = ScopeKeys(read)]
                  /\ keysB' = [keysB EXCEPT ![i] = {}]
                  /\ UNCHANGED wantB
             ELSE IF Escalation = "SplitPhaseB"
             THEN \* A second phase B call over the keys outside the own keys.
                  \* It skips the stripes that the process holds, so only a
                  \* cycle with another process can block it.
                  /\ pc' = [pc EXCEPT ![i] = "lockB"]
                  /\ wantB' = [wantB EXCEPT ![i] = StripesOf(extra) \ HeldB(i)]
                  /\ keysB' = [keysB EXCEPT ![i] = keysB[i] \cup extra]
                  /\ UNCHANGED <<ownerA, ownerB, wantA, keysA>>
             ELSE \* "PhaseAAfterB": both phases of the keys outside the own
                  \* keys, while the process holds phase B of its own keys.
                  \* Each call skips the stripes that the process holds.
                  /\ pc' = [pc EXCEPT ![i] = "lockA2"]
                  /\ wantA' = [wantA EXCEPT ![i] = StripesOf(extra) \ HeldA(i)]
                  /\ keysA' = [keysA EXCEPT ![i] = keysA[i] \cup extra]
                  /\ UNCHANGED <<ownerA, ownerB, wantB, keysB>>
    /\ UNCHANGED <<stripe, links, err>>

\* The phase A call of the new transaction of "Rebegin", then its phase B.
RelockA(i) ==
    /\ pc[i] = "relockA"
    /\ \/ /\ TakeA(i)
          /\ UNCHANGED <<pc, wantB, keysB>>
       \/ /\ wantA[i] = {}
          /\ pc' = [pc EXCEPT ![i] = "lockB"]
          /\ wantB' = [wantB EXCEPT ![i] = StripesOf(keysA[i])]
          /\ keysB' = [keysB EXCEPT ![i] = keysA[i]]
          /\ UNCHANGED <<ownerA, wantA>>
    /\ UNCHANGED <<stripe, ownerB, keysA, links, scope, err>>

\* The phase A call of "PhaseAAfterB" over the extra keys, then its phase B.
LockA2(i) ==
    /\ pc[i] = "lockA2"
    /\ \/ /\ TakeA(i)
          /\ UNCHANGED <<pc, wantB, keysB>>
       \/ /\ wantA[i] = {}
          /\ pc' = [pc EXCEPT ![i] = "lockB"]
          /\ wantB' = [wantB EXCEPT ![i] = StripesOf(keysA[i] \ keysB[i]) \ HeldB(i)]
          /\ keysB' = [keysB EXCEPT ![i] = keysA[i]]
          /\ UNCHANGED <<ownerA, wantA>>
    /\ UNCHANGED <<stripe, ownerB, keysA, links, scope, err>>

\* The last phase B call before staging.
LockB(i) ==
    /\ pc[i] = "lockB"
    /\ \/ /\ TakeB(i)
          /\ UNCHANGED pc
       \/ /\ wantB[i] = {}
          /\ pc' = [pc EXCEPT ![i] = "stage"]
          /\ UNCHANGED <<ownerB, wantB>>
    /\ UNCHANGED <<stripe, ownerA, wantA, keysA, keysB, links, scope, err>>

\* The channels that transaction i touches, and the coverage check.
Touched(i) == {Cfg} \cup Own(i) \cup scope[i]

Covered(i) ==
    IF CoverageByPhaseB
    THEN Touched(i) \subseteq keysB[i]
    ELSE Touched(i) \subseteq keysA[i] \cap keysB[i]

\* The effect of a transaction on the join groups.
Effect(i, L) ==
    IF i = "t1"
    THEN L \ {{"D1", "D2"}}
    ELSE L \cup {{"D2", "D3"}}

\* Staging and commit, then the release.
Stage(i) ==
    /\ pc[i] = "stage"
    /\ IF scope[i] /= Closure(Own(i), links)
       THEN /\ err' = [err EXCEPT ![i] = "stale"]
            /\ UNCHANGED links
       ELSE IF ~Covered(i)
       THEN /\ err' = [err EXCEPT ![i] = "unlocked"]
            /\ UNCHANGED links
       ELSE /\ links' = Effect(i, links)
            /\ UNCHANGED err
    /\ ReleaseAll(i)
    /\ pc' = [pc EXCEPT ![i] = "done"]
    /\ UNCHANGED <<stripe, wantA, wantB, scope>>

SpaceStep(i) ==
    \/ Start(i) \/ LockA0(i) \/ LockB0(i) \/ Classify(i)
    \/ RelockA(i) \/ LockA2(i) \/ LockB(i) \/ Stage(i)

-----------------------------------------------------------------------------
(* The host produce on h1 (produce_lock)                                    *)

HostStart ==
    /\ pc[Host] = "idle"
    /\ pc' = [pc EXCEPT ![Host] = "hostA"]
    /\ wantA' = [wantA EXCEPT ![Host] = {stripe["h1"]}]
    /\ keysA' = [keysA EXCEPT ![Host] = {"h1"}]
    /\ UNCHANGED <<stripe, ownerA, ownerB, wantB, keysB, links, scope, err>>

HostLockA ==
    /\ pc[Host] = "hostA"
    /\ \/ /\ TakeA(Host)
          /\ UNCHANGED <<pc, wantB, keysB>>
       \/ /\ wantA[Host] = {}
          \* The join read under phase A, then phase B on the join channels
          \* and the channel.
          /\ pc' = [pc EXCEPT ![Host] = "hostB"]
          /\ wantB' = [wantB EXCEPT ![Host] = StripesOf(HostJoin \cup {"h1"})]
          /\ keysB' = [keysB EXCEPT ![Host] = HostJoin \cup {"h1"}]
          /\ UNCHANGED <<ownerA, wantA>>
    /\ UNCHANGED <<stripe, ownerB, keysA, links, scope, err>>

HostLockB ==
    /\ pc[Host] = "hostB"
    /\ \/ /\ TakeB(Host)
          /\ UNCHANGED pc
       \/ /\ wantB[Host] = {}
          /\ pc' = [pc EXCEPT ![Host] = "hostRun"]
          /\ UNCHANGED <<ownerB, wantB>>
    /\ UNCHANGED <<stripe, ownerA, wantA, keysA, keysB, links, scope, err>>

HostRun ==
    /\ pc[Host] = "hostRun"
    /\ ReleaseAll(Host)
    /\ pc' = [pc EXCEPT ![Host] = "done"]
    /\ UNCHANGED <<stripe, wantA, wantB, links, scope, err>>

HostStep == HostStart \/ HostLockA \/ HostLockB \/ HostRun

-----------------------------------------------------------------------------

Step(i) == IF i = Host THEN HostStep ELSE SpaceStep(i)

Init ==
    /\ pc = [i \in Procs |-> "idle"]
    /\ stripe \in [Chans -> Stripes]
    /\ ownerA = [s \in Stripes |-> NoProc]
    /\ ownerB = [s \in Stripes |-> NoProc]
    /\ wantA = [i \in Procs |-> {}]
    /\ wantB = [i \in Procs |-> {}]
    /\ keysA = [i \in Procs |-> {}]
    /\ keysB = [i \in Procs |-> {}]
    /\ links = {{"D1", "D2"}}
    /\ scope = [i \in SpaceTxns |-> {}]
    /\ err = [i \in SpaceTxns |-> "none"]

Next == \E i \in Procs : Step(i)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants                                                               *)

PcValues == {"idle", "lockA0", "lockB0", "classify", "relockA", "lockA2", "lockB", "stage",
             "hostA", "hostB", "hostRun", "done"}

TypeOK ==
    /\ pc \in [Procs -> PcValues]
    /\ stripe \in [Chans -> Stripes]
    /\ ownerA \in [Stripes -> Procs \cup {NoProc}]
    /\ ownerB \in [Stripes -> Procs \cup {NoProc}]
    /\ wantA \in [Procs -> SUBSET Stripes]
    /\ wantB \in [Procs -> SUBSET Stripes]
    /\ keysA \in [Procs -> SUBSET Chans]
    /\ keysB \in [Procs -> SUBSET Chans]
    /\ links \subseteq Links
    /\ scope \in [SpaceTxns -> SUBSET Colls]
    /\ err \in [SpaceTxns -> {"none", "stale", "unlocked"}]

\* A transaction stages only with the scope of the current state.
NoStaleScope == \A i \in SpaceTxns : err[i] /= "stale"

\* A transaction with a current scope passes the coverage check.
NoFalseRefusal == \A i \in SpaceTxns : err[i] /= "unlocked"

\* The lock calls in progress: the next stripe of the call is free, or the
\* call has taken every stripe.
FreeA(i) == wantA[i] = {} \/ ownerA[Min(wantA[i])] = NoProc
FreeB(i) == wantB[i] = {} \/ ownerB[Min(wantB[i])] = NoProc

CanStep(i) ==
    IF pc[i] \in {"idle", "classify", "stage", "hostRun"} THEN TRUE
    ELSE IF pc[i] \in {"lockA0", "relockA", "lockA2", "hostA"} THEN FreeA(i)
    ELSE IF pc[i] \in {"lockB0", "lockB", "hostB"} THEN FreeB(i)
    ELSE FALSE

AllEnded == \A i \in Procs : pc[i] = "done"

NoDeadlock == AllEnded \/ \E i \in Procs : CanStep(i)

CanStepIsEnabled == \A i \in Procs : CanStep(i) = ENABLED Step(i)

\* A process at its staging or run step holds every lock of its keys.
InCriticalSection(i) == pc[i] \in {"stage", "hostRun"}

TouchKeys(i) == IF i = Host THEN HostJoin ELSE Touched(i)

MutualExclusion ==
    \A i, j \in Procs :
        (i /= j /\ InCriticalSection(i) /\ InCriticalSection(j))
            => TouchKeys(i) \cap TouchKeys(j) = {}

=============================================================================

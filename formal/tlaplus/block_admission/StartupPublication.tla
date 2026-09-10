------------------------- MODULE StartupPublication -------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Publishers, HoldBoth, DropAfterUnlock, StopBeforeDrop, EventAfterSwap
VARIABLES pc, engineLock, controlLock, engine, context, committed, announced,
          rejected, dropped, stopped, signalStopped, stopPhase, badDrop,
          badStopDrop
vars == <<pc, engineLock, controlLock, engine, context, committed, announced,
          rejected, dropped, stopped, signalStopped, stopPhase, badDrop,
          badStopDrop>>

Init ==
    /\ pc = [a \in Publishers |-> "start"]
    /\ engineLock = 0 /\ controlLock = 0 /\ engine = 0 /\ context = 0
    /\ committed = {} /\ announced = {} /\ rejected = {} /\ dropped = {}
    /\ stopped = FALSE /\ signalStopped = FALSE /\ stopPhase = "start"
    /\ badDrop = FALSE /\ badStopDrop = FALSE

AcquireEngine(a) ==
    /\ pc[a] = "start" /\ engineLock = 0
    /\ engineLock' = a /\ pc' = [pc EXCEPT ![a] = "engine"]
    /\ UNCHANGED <<controlLock, engine, context, committed, announced, rejected,
                   dropped, stopped, signalStopped, stopPhase, badDrop, badStopDrop>>

AcquireControl(a) ==
    /\ pc[a] = "engine" /\ controlLock = 0
    /\ controlLock' = a /\ pc' = [pc EXCEPT ![a] = "control"]
    /\ UNCHANGED <<engineLock, engine, context, committed, announced, rejected,
                   dropped, stopped, signalStopped, stopPhase, badDrop, badStopDrop>>

Reject(a) ==
    /\ pc[a] \in {"engine", "control"}
    /\ rejected' = rejected \cup {a}
    /\ pc' = [pc EXCEPT ![a] = IF DropAfterUnlock THEN "rejected" ELSE "drop-rejected"]
    /\ UNCHANGED <<engineLock, controlLock, engine, context, committed, announced,
                   dropped, stopped, signalStopped, stopPhase, badDrop, badStopDrop>>

Register(a) ==
    /\ pc[a] = "control" /\ ~stopped
    /\ context' = a /\ pc' = [pc EXCEPT ![a] = "registered"]
    /\ controlLock' = IF HoldBoth THEN controlLock ELSE 0
    /\ UNCHANGED <<engineLock, engine, committed, announced, rejected, dropped,
                   stopped, signalStopped, stopPhase, badDrop, badStopDrop>>

Swap(a) ==
    /\ pc[a] = "registered"
    /\ engine' = a /\ committed' = committed \cup {a}
    /\ pc' = [pc EXCEPT ![a] = "swapped"]
    /\ UNCHANGED <<engineLock, controlLock, context, announced, rejected, dropped,
                   stopped, signalStopped, stopPhase, badDrop, badStopDrop>>

ReleaseControl(a) ==
    /\ pc[a] \in {"swapped", "rejected"}
    /\ controlLock' = IF controlLock = a THEN 0 ELSE controlLock
    /\ pc' = [pc EXCEPT ![a] = "released-control"]
    /\ UNCHANGED <<engineLock, engine, context, committed, announced, rejected,
                   dropped, stopped, signalStopped, stopPhase, badDrop, badStopDrop>>

ReleaseEngine(a) ==
    /\ pc[a] = "released-control" /\ engineLock = a
    /\ engineLock' = 0 /\ pc' = [pc EXCEPT ![a] = "released"]
    /\ UNCHANGED <<controlLock, engine, context, committed, announced, rejected,
                   dropped, stopped, signalStopped, stopPhase, badDrop, badStopDrop>>

Destroy(a) ==
    /\ pc[a] \in {"released", "drop-rejected"}
    /\ dropped' = dropped \cup {a}
    /\ badDrop' = (badDrop \/ engineLock = a \/ controlLock = a)
    /\ pc' = [pc EXCEPT ![a] = IF pc[a] = "drop-rejected" THEN "rejected" ELSE "done"]
    /\ UNCHANGED <<engineLock, controlLock, engine, context, committed, announced,
                   rejected, stopped, signalStopped, stopPhase, badStopDrop>>

Announce(a) ==
    /\ a \notin announced
    /\ IF EventAfterSwap THEN a \in committed /\ pc[a] \in {"released", "done"}
       ELSE pc[a] = "start"
    /\ announced' = announced \cup {a}
    /\ UNCHANGED <<pc, engineLock, controlLock, engine, context, committed, rejected,
                   dropped, stopped, signalStopped, stopPhase, badDrop, badStopDrop>>

StopOwner ==
    /\ stopPhase = "start" /\ controlLock = 0
    /\ stopped' = TRUE /\ context' = 0
    /\ stopPhase' = "marked"
    /\ UNCHANGED <<pc, engineLock, controlLock, engine, committed, announced,
                   rejected, dropped, signalStopped, badDrop, badStopDrop>>

StopSignal ==
    /\ stopPhase = IF StopBeforeDrop THEN "marked" ELSE "destroyed"
    /\ signalStopped' = TRUE
    /\ stopPhase' = IF StopBeforeDrop THEN "signaled" ELSE "done"
    /\ UNCHANGED <<pc, engineLock, controlLock, engine, context, committed, announced,
                   rejected, dropped, stopped, badDrop, badStopDrop>>

DestroyPending ==
    /\ stopPhase = IF StopBeforeDrop THEN "signaled" ELSE "marked"
    /\ badStopDrop' = (badStopDrop \/ ~signalStopped)
    /\ stopPhase' = IF StopBeforeDrop THEN "done" ELSE "destroyed"
    /\ UNCHANGED <<pc, engineLock, controlLock, engine, context, committed, announced,
                   rejected, dropped, stopped, signalStopped, badDrop>>

Next == StopOwner \/ StopSignal \/ DestroyPending \/
        (\E a \in Publishers : AcquireEngine(a) \/ AcquireControl(a) \/ Reject(a)
            \/ Register(a) \/ Swap(a) \/ ReleaseControl(a) \/ ReleaseEngine(a)
            \/ Destroy(a) \/ Announce(a))

TypeOK ==
    /\ pc \in [Publishers -> {"start", "engine", "control", "registered", "swapped",
                              "rejected", "drop-rejected", "released-control", "released", "done"}]
    /\ engineLock \in Publishers \cup {0} /\ controlLock \in Publishers \cup {0}
    /\ engine \in Publishers \cup {0} /\ context \in Publishers \cup {0}
    /\ committed \subseteq Publishers /\ announced \subseteq Publishers
    /\ rejected \subseteq Publishers /\ dropped \subseteq Publishers
    /\ stopped \in BOOLEAN /\ signalStopped \in BOOLEAN
    /\ stopPhase \in {"start", "marked", "signaled", "destroyed", "done"}
    /\ badDrop \in BOOLEAN /\ badStopDrop \in BOOLEAN

Inv_PublicationVisible == (controlLock = 0 /\ ~stopped) => engine = context
Inv_DestructionOutsideGuards == ~badDrop
Inv_StopPublishedBeforeDestruction == ~badStopDrop
Inv_OnlyCommittedRunningEvents == announced \subseteq committed
Inv_RejectedPublicationUnchanged == rejected \intersect committed = {}
Spec == Init /\ [][Next]_vars
=============================================================================

---------------------------- MODULE RecoveryPumpWake ----------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Actors, Requests, StoreNotification, PreservePending
VARIABLES owner, issued, outstanding, handled, cancelled, pending, debt, notice, stopped, pc
vars == <<owner, issued, outstanding, handled, cancelled, pending, debt, notice, stopped, pc>>

Init ==
    /\ owner \in [Requests -> Actors]
    /\ issued = {}
    /\ outstanding = {}
    /\ handled = {}
    /\ cancelled = {}
    /\ pending = [a \in Actors |-> FALSE]
    /\ debt = {}
    /\ notice = [a \in Actors |-> FALSE]
    /\ stopped = [a \in Actors |-> FALSE]
    /\ pc = [a \in Actors |-> "check"]

Publish(r) ==
    /\ r \notin issued
    /\ ~stopped[owner[r]]
    /\ issued' = issued \cup {r}
    /\ outstanding' = outstanding \cup {r}
    /\ pending' = [pending EXCEPT ![owner[r]] = TRUE]
    /\ debt' = debt \cup {r}
    /\ UNCHANGED <<owner, handled, cancelled, notice, stopped, pc>>

Notify(r) ==
    /\ r \in debt
    /\ debt' = debt \ {r}
    /\ notice' = [notice EXCEPT ![owner[r]] = @ \/ StoreNotification \/ pc[owner[r]] = "wait"]
    /\ UNCHANGED <<owner, issued, outstanding, handled, cancelled, pending, stopped, pc>>

Check(a) ==
    /\ pc[a] = "check"
    /\ LET selected == {r \in outstanding : owner[r] = a}
       IN /\ handled' = IF ~stopped[a] /\ pending[a] THEN handled \cup selected ELSE handled
          /\ outstanding' = IF ~stopped[a] /\ pending[a] THEN outstanding \ selected ELSE outstanding
    /\ pc' = [pc EXCEPT ![a] = IF stopped[a] THEN "done" ELSE IF pending[a] THEN "work" ELSE "arm"]
    /\ pending' = [pending EXCEPT ![a] = FALSE]
    /\ UNCHANGED <<owner, issued, cancelled, debt, notice, stopped>>

Arm(a) ==
    /\ pc[a] = "arm"
    /\ pc' = [pc EXCEPT ![a] = "wait"]
    /\ UNCHANGED <<owner, issued, outstanding, handled, cancelled, pending, debt, notice, stopped>>

Wake(a) ==
    /\ pc[a] = "wait"
    /\ notice[a]
    /\ notice' = [notice EXCEPT ![a] = FALSE]
    /\ pc' = [pc EXCEPT ![a] = "check"]
    /\ UNCHANGED <<owner, issued, outstanding, handled, cancelled, pending, debt, stopped>>

Complete(a) ==
    /\ pc[a] = "work"
    /\ pc' = [pc EXCEPT ![a] = "check"]
    /\ pending' = IF PreservePending THEN pending ELSE [pending EXCEPT ![a] = FALSE]
    /\ UNCHANGED <<owner, issued, outstanding, handled, cancelled, debt, notice, stopped>>

Stop(a) ==
    /\ ~stopped[a]
    /\ LET selected == {r \in outstanding : owner[r] = a}
       IN /\ cancelled' = cancelled \cup selected
          /\ outstanding' = outstanding \ selected
    /\ stopped' = [stopped EXCEPT ![a] = TRUE]
    /\ pending' = [pending EXCEPT ![a] = FALSE]
    /\ notice' = [notice EXCEPT ![a] = TRUE]
    /\ UNCHANGED <<owner, issued, handled, debt, pc>>

Next ==
    \/ \E r \in Requests : Publish(r) \/ Notify(r)
    \/ \E a \in Actors : Check(a) \/ Arm(a) \/ Wake(a) \/ Complete(a) \/ Stop(a)

TypeOK ==
    /\ owner \in [Requests -> Actors]
    /\ issued \subseteq Requests
    /\ outstanding \subseteq issued
    /\ handled \subseteq issued
    /\ cancelled \subseteq issued
    /\ debt \subseteq issued
    /\ pending \in [Actors -> BOOLEAN]
    /\ notice \in [Actors -> BOOLEAN]
    /\ stopped \in [Actors -> BOOLEAN]
    /\ pc \in [Actors -> {"check", "arm", "wait", "work", "done"}]

Inv_RequestConservation ==
    /\ issued = outstanding \cup handled \cup cancelled
    /\ outstanding \cap handled = {}
    /\ outstanding \cap cancelled = {}
    /\ handled \cap cancelled = {}
Inv_PendingRetained == \A r \in outstanding : pending[owner[r]]
Inv_NoLostWake == \A a \in Actors :
    pending[a] /\ pc[a] = "wait" => notice[a] \/ (\E r \in debt : owner[r] = a)
Inv_StoppedCannotPublish == \A a \in Actors : stopped[a] => ~pending[a]
Live_RequestObserved == \A r \in Requests : (r \in issued) ~> (r \in handled \cup cancelled)
Live_StopTerminates == \A a \in Actors : stopped[a] ~> (pc[a] = "done")
Spec == Init /\ [][Next]_vars
        /\ (\A r \in Requests : WF_vars(Notify(r)))
        /\ (\A a \in Actors : WF_vars(Check(a)) /\ WF_vars(Arm(a))
                              /\ WF_vars(Wake(a)) /\ WF_vars(Complete(a)))
=============================================================================

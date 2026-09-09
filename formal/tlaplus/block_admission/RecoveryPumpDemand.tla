--------------------------- MODULE RecoveryPumpDemand ---------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Requests, ContinueRequests, ClearNewProposal, RespectStop, PreserveTakenDemand
VARIABLES kind, issued, outstanding, demand, stopped, phase, currentProposal, failed,
          nextOutstanding, nextDemand
vars == <<kind, issued, outstanding, demand, stopped, phase, currentProposal, failed,
          nextOutstanding, nextDemand>>
Expected == IF outstanding = {} THEN 0
            ELSE IF \E r \in outstanding : kind[r] THEN 2 ELSE 1
ExpectedNext == IF nextOutstanding = {} THEN 0
                ELSE IF \E r \in nextOutstanding : kind[r] THEN 2 ELSE 1
Combined == IF demand = 2 \/ nextDemand = 2 THEN 2
            ELSE IF demand > 0 \/ nextDemand > 0 THEN 1 ELSE 0

Init ==
    /\ kind \in [Requests -> BOOLEAN]
    /\ issued = {}
    /\ outstanding = {}
    /\ demand = 0
    /\ stopped = FALSE
    /\ phase = "idle"
    /\ currentProposal = FALSE
    /\ failed = FALSE
    /\ nextOutstanding = {} /\ nextDemand = 0

Publish(r) ==
    /\ r \notin issued /\ (~RespectStop \/ ~stopped)
    /\ issued' = issued \cup {r}
    /\ outstanding' = outstanding \cup {r}
    /\ demand' = IF kind[r] \/ demand = 2 THEN 2 ELSE 1
    /\ UNCHANGED <<kind, stopped, phase, currentProposal, failed, nextOutstanding, nextDemand>>

Begin ==
    /\ phase = "idle" /\ ~stopped /\ Combined > 0
    /\ currentProposal' = (Combined = 2)
    /\ demand' = 0
    /\ outstanding' = {}
    /\ phase' = "page"
    /\ failed' = FALSE
    /\ nextOutstanding' = {} /\ nextDemand' = 0
    /\ UNCHANGED <<kind, issued, stopped>>

TakeIntoNext ==
    /\ phase \in {"page", "between", "last"} /\ ~stopped /\ demand > 0
    /\ nextOutstanding' = nextOutstanding \cup outstanding
    /\ nextDemand' = IF PreserveTakenDemand THEN Combined ELSE nextDemand
    /\ outstanding' = {} /\ demand' = 0
    /\ UNCHANGED <<kind, issued, stopped, phase, currentProposal, failed>>

Page(error) ==
    /\ phase = "page" /\ ~stopped
    /\ failed' = (failed \/ error)
    /\ phase' = "between"
    /\ UNCHANGED <<kind, issued, outstanding, demand, stopped, currentProposal, nextOutstanding, nextDemand>>

Continue ==
    /\ phase = "between" /\ ~stopped
    /\ demand' = IF ContinueRequests /\ demand = 0 THEN 1 ELSE demand
    /\ phase' = "last"
    /\ UNCHANGED <<kind, issued, outstanding, stopped, currentProposal, failed, nextOutstanding, nextDemand>>

Finish ==
    /\ phase = "last" /\ ~stopped
    /\ demand' = IF ClearNewProposal /\ failed /\ demand = 2 THEN 1 ELSE demand
    /\ phase' = "idle"
    /\ currentProposal' = FALSE
    /\ UNCHANGED <<kind, issued, outstanding, stopped, failed, nextOutstanding, nextDemand>>

Stop ==
    /\ ~stopped
    /\ stopped' = TRUE
    /\ outstanding' = {}
    /\ demand' = 0
    /\ phase' = "stopped"
    /\ currentProposal' = FALSE
    /\ nextOutstanding' = {} /\ nextDemand' = 0
    /\ UNCHANGED <<kind, issued, failed>>

Next == (\E r \in Requests : Publish(r)) \/ Begin
        \/ (\E error \in BOOLEAN : Page(error)) \/ TakeIntoNext \/ Continue \/ Finish \/ Stop
TypeOK ==
    /\ kind \in [Requests -> BOOLEAN]
    /\ issued \subseteq Requests /\ outstanding \subseteq issued
    /\ nextOutstanding \subseteq issued /\ outstanding \cap nextOutstanding = {}
    /\ nextDemand \in 0..2
    /\ demand \in 0..2 /\ stopped \in BOOLEAN /\ failed \in BOOLEAN
    /\ currentProposal \in BOOLEAN
    /\ phase \in {"idle", "page", "between", "last", "stopped"}
Inv_ExactDemand == demand = Expected
Inv_ExactNextDemand == nextDemand = ExpectedNext
Inv_StopIsTerminal == stopped => demand = 0 /\ nextDemand = 0 /\ phase = "stopped" /\ ~currentProposal
Live_DemandServed == (Combined > 0) ~> (Combined = 0)
Spec == Init /\ [][Next]_vars /\ WF_vars(Begin)
        /\ WF_vars(\E error \in BOOLEAN : Page(error)) /\ WF_vars(Continue) /\ WF_vars(Finish)
=============================================================================

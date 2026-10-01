------------------------ MODULE LocalValidationRecovery ------------------------
EXTENDS Naturals, FiniteSets

CONSTANT DeferLocalFault

Parent == "parent"
Child == "child"
Blocks == {Parent, Child}
Phases == {"blocked", "ready", "in-flight", "deferred", "valid", "invalid"}

VARIABLES
    phase,
    faultRemaining,
    recoveryFailuresRemaining,
    recoveryOutstanding,
    immediateSelfRequeues

vars == <<
    phase,
    faultRemaining,
    recoveryFailuresRemaining,
    recoveryOutstanding,
    immediateSelfRequeues
>>

Init ==
    /\ phase = [block \in Blocks |-> IF block = Parent THEN "ready" ELSE "blocked"]
    /\ faultRemaining = 1
    /\ recoveryFailuresRemaining = 1
    /\ recoveryOutstanding = {}
    /\ immediateSelfRequeues = 0

DispatchParent ==
    /\ phase[Parent] = "ready"
    /\ phase' = [phase EXCEPT ![Parent] = "in-flight"]
    /\ UNCHANGED <<faultRemaining, recoveryFailuresRemaining,
                    recoveryOutstanding, immediateSelfRequeues>>

ObserveLocalFault ==
    /\ phase[Parent] = "in-flight"
    /\ faultRemaining = 1
    /\ faultRemaining' = 0
    /\ phase' =
        [phase EXCEPT ![Parent] = IF DeferLocalFault THEN "deferred" ELSE "ready"]
    /\ recoveryOutstanding' =
        IF DeferLocalFault THEN {Parent} ELSE recoveryOutstanding
    /\ immediateSelfRequeues' =
        IF DeferLocalFault THEN immediateSelfRequeues ELSE immediateSelfRequeues + 1
    /\ UNCHANGED recoveryFailuresRemaining

RecoveryRequestFails ==
    /\ phase[Parent] = "deferred"
    /\ Parent \in recoveryOutstanding
    /\ recoveryFailuresRemaining = 1
    /\ recoveryFailuresRemaining' = 0
    /\ UNCHANGED <<phase, faultRemaining, recoveryOutstanding,
                    immediateSelfRequeues>>

RecoveryRequestSucceeds ==
    /\ phase[Parent] = "deferred"
    /\ Parent \in recoveryOutstanding
    /\ recoveryFailuresRemaining = 0
    /\ phase' = [phase EXCEPT ![Parent] = "ready"]
    /\ recoveryOutstanding' = recoveryOutstanding \ {Parent}
    /\ UNCHANGED <<faultRemaining, recoveryFailuresRemaining,
                    immediateSelfRequeues>>

ValidateParent ==
    /\ phase[Parent] = "in-flight"
    /\ faultRemaining = 0
    /\ phase' = [phase EXCEPT
                    ![Parent] = "valid",
                    ![Child] = "ready"]
    /\ UNCHANGED <<faultRemaining, recoveryFailuresRemaining,
                    recoveryOutstanding, immediateSelfRequeues>>

DispatchChild ==
    /\ phase[Child] = "ready"
    /\ phase[Parent] = "valid"
    /\ phase' = [phase EXCEPT ![Child] = "in-flight"]
    /\ UNCHANGED <<faultRemaining, recoveryFailuresRemaining,
                    recoveryOutstanding, immediateSelfRequeues>>

ValidateChild ==
    /\ phase[Child] = "in-flight"
    /\ phase[Parent] = "valid"
    /\ phase' = [phase EXCEPT ![Child] = "valid"]
    /\ UNCHANGED <<faultRemaining, recoveryFailuresRemaining,
                    recoveryOutstanding, immediateSelfRequeues>>

Next ==
    DispatchParent
    \/ ObserveLocalFault
    \/ RecoveryRequestFails
    \/ RecoveryRequestSucceeds
    \/ ValidateParent
    \/ DispatchChild
    \/ ValidateChild

Spec ==
    Init
    /\ [][Next]_vars
    /\ WF_vars(DispatchParent)
    /\ WF_vars(ObserveLocalFault)
    /\ WF_vars(RecoveryRequestFails)
    /\ WF_vars(RecoveryRequestSucceeds)
    /\ WF_vars(ValidateParent)
    /\ WF_vars(DispatchChild)
    /\ WF_vars(ValidateChild)

TypeOK ==
    /\ phase \in [Blocks -> Phases]
    /\ faultRemaining \in 0..1
    /\ recoveryFailuresRemaining \in 0..1
    /\ recoveryOutstanding \subseteq Blocks
    /\ immediateSelfRequeues \in Nat

Inv_DeferredBlockIsNotReady ==
    phase[Parent] = "deferred" => phase[Parent] # "ready"

Inv_AtMostOneRecoveryOutstanding == Cardinality(recoveryOutstanding) <= 1

Inv_LocalFaultNeverCreatesInvalidity == phase[Parent] # "invalid"

Inv_ChildWaitsForValidParent ==
    phase[Child] \in {"ready", "in-flight", "valid"}
    => phase[Parent] = "valid"

Inv_NoImmediateSelfRequeue == immediateSelfRequeues = 0

Live_ParentAndChildValidate ==
    <> (phase[Parent] = "valid" /\ phase[Child] = "valid")
=============================================================================

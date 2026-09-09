--------------------- MODULE RecoveryMetadataReadiness ---------------------
EXTENDS Naturals, Sequences, FiniteSets
CONSTANTS Keys, References, RequireVisibility, RequireRow, PreserveErrors, CheckAfterMissing
VARIABLES rows, admitted, references, observations, next, missing, result
vars == <<rows, admitted, references, observations, next, missing, result>>

Init ==
    /\ rows = [key \in Keys |-> "absent"]
    /\ admitted = {}
    /\ references \in [1..References -> Keys]
    /\ observations = <<>> /\ next = 1 /\ missing = FALSE /\ result = "checking"

WriteRow(key, value) ==
    /\ rows' = [rows EXCEPT ![key] = value]
    /\ UNCHANGED <<admitted, references, observations, next, missing, result>>

Publish(key) ==
    /\ rows[key] = "valid"
    /\ admitted' = admitted \cup {key}
    /\ UNCHANGED <<rows, references, observations, next, missing, result>>

Read ==
    /\ result = "checking" /\ next <= References
    /\ LET key == references[next]
           visible == key \in admitted
           row == rows[key]
           observed == IF RequireVisibility /\ ~visible THEN "absent"
                       ELSE IF ~RequireRow /\ visible THEN "valid"
                       ELSE IF ~PreserveErrors /\ row = "error" THEN "absent"
                       ELSE row
       IN /\ observations' = Append(observations,
                  [key |-> key, visible |-> visible, row |-> row, observed |-> observed])
          /\ next' = next + 1
          /\ missing' = (missing \/ observed = "absent")
          /\ result' = IF observed = "error" THEN "error"
                       ELSE IF ~CheckAfterMissing /\ observed = "absent" THEN "missing"
                       ELSE result
    /\ UNCHANGED <<rows, admitted, references>>

Finish ==
    /\ result = "checking" /\ next = References + 1
    /\ result' = IF missing THEN "missing" ELSE "ready"
    /\ UNCHANGED <<rows, admitted, references, observations, next, missing>>

Next == Read \/ Finish \/ (\E key \in Keys : Publish(key))
        \/ (\E key \in Keys, value \in {"absent", "valid", "error"} : WriteRow(key, value))

TypeOK ==
    /\ rows \in [Keys -> {"absent", "valid", "error"}]
    /\ admitted \subseteq Keys /\ references \in [1..References -> Keys]
    /\ next \in 1..(References + 1) /\ Len(observations) = next - 1
    /\ missing \in BOOLEAN /\ result \in {"checking", "ready", "missing", "error"}

Inv_ReadWitness ==
    \A i \in 1..Len(observations) :
        observations[i].observed = "valid" => observations[i].visible /\ observations[i].row = "valid"

Inv_ErrorPreserved ==
    \A i \in 1..Len(observations) :
        observations[i].visible /\ observations[i].row = "error" => observations[i].observed = "error"

Inv_CompleteOrError == result \in {"ready", "missing"} => next = References + 1
Inv_ReadyHasEveryWitness ==
    result = "ready" => Len(observations) = References /\
        (\A i \in 1..Len(observations) : observations[i].observed = "valid")
Inv_UnpublishedCannotRead ==
    \A i \in 1..Len(observations) : ~observations[i].visible => observations[i].observed = "absent"
Spec == Init /\ [][Next]_vars
=============================================================================

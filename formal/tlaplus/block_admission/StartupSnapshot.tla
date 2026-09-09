--------------------------- MODULE StartupSnapshot ---------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Keys, FreezeSnapshot, CompleteFilterFirst, PreserveSelected
VARIABLES live, captured, snapshot, remaining, checked, present, selected,
          admitted, phase, capacity
vars == <<live, captured, snapshot, remaining, checked, present, selected,
          admitted, phase, capacity>>

Init ==
    /\ live \in SUBSET Keys
    /\ captured = {} /\ snapshot = {} /\ remaining = {} /\ checked = {}
    /\ present = {} /\ selected = {} /\ admitted = {}
    /\ phase = "new" /\ capacity \in BOOLEAN

ChangeMembership(key) ==
    /\ live' = IF key \in live THEN live \ {key} ELSE live \cup {key}
    /\ snapshot' = IF FreezeSnapshot \/ phase = "new" THEN snapshot ELSE live'
    /\ selected' = IF PreserveSelected \/ phase # "admit" THEN selected ELSE selected \cap live'
    /\ UNCHANGED <<captured, remaining, checked, present, admitted, phase, capacity>>

Capture ==
    /\ phase = "new"
    /\ captured' = live /\ snapshot' = live /\ remaining' = live
    /\ phase' = "filter"
    /\ UNCHANGED <<live, checked, present, selected, admitted, capacity>>

Filter(key, result) ==
    /\ phase = "filter" /\ capacity /\ key \in remaining
    /\ result \in {"present", "absent", "error"}
    /\ remaining' = remaining \ {key}
    /\ checked' = checked \cup {key}
    /\ present' = IF result = "present" THEN present \cup {key} ELSE present
    /\ selected' = IF result = "present" THEN selected \cup {key} ELSE selected
    /\ phase' = IF result = "error" THEN "failed" ELSE phase
    /\ UNCHANGED <<live, captured, snapshot, admitted, capacity>>

FilterComplete ==
    /\ phase = "filter" /\ remaining = {}
    /\ phase' = "admit"
    /\ remaining' = selected
    /\ UNCHANGED <<live, captured, snapshot, checked, present, selected, admitted, capacity>>

Process(key, result) ==
    /\ (phase = "admit" \/ (~CompleteFilterFirst /\ phase = "filter"))
    /\ capacity /\ key \in selected
    /\ IF phase = "admit" THEN key \in remaining ELSE key \notin admitted
    /\ result \in {"present", "absent", "error"}
    /\ admitted' = IF result = "present" THEN admitted \cup {key} ELSE admitted
    /\ remaining' = remaining \ {key}
    /\ phase' = IF result = "error" THEN "failed" ELSE phase
    /\ UNCHANGED <<live, captured, snapshot, checked, present, selected, capacity>>

Complete ==
    /\ phase = "admit" /\ remaining = {}
    /\ phase' = "done"
    /\ UNCHANGED <<live, captured, snapshot, remaining, checked, present, selected, admitted, capacity>>

Cancel ==
    /\ phase \in {"filter", "admit"}
    /\ phase' = "cancelled"
    /\ UNCHANGED <<live, captured, snapshot, remaining, checked, present, selected, admitted, capacity>>

CapacityChange ==
    /\ capacity' = ~capacity
    /\ UNCHANGED <<live, captured, snapshot, remaining, checked, present, selected, admitted, phase>>

Next == (\E key \in Keys : ChangeMembership(key)) \/ Capture \/ FilterComplete \/ Complete \/ Cancel
        \/ CapacityChange
        \/ (\E key \in Keys, result \in {"present", "absent", "error"} : Filter(key, result) \/ Process(key, result))
TypeOK ==
    /\ live \subseteq Keys /\ captured \subseteq Keys /\ snapshot \subseteq Keys
    /\ remaining \subseteq Keys /\ checked \subseteq Keys /\ present \subseteq Keys
    /\ selected \subseteq Keys /\ admitted \subseteq Keys
    /\ phase \in {"new", "filter", "admit", "failed", "done", "cancelled"}
    /\ capacity \in BOOLEAN
Inv_ImmutableSnapshot == phase # "new" => snapshot = captured
Inv_SelectionPreserved == selected = present
Inv_OnlyCapturedHashes == checked \subseteq captured /\ present \subseteq checked /\ admitted \subseteq present
Inv_FilterBeforeAdmission == admitted # {} => checked = captured
Inv_CompleteCoverage == phase = "done" => checked = captured /\ remaining = {}
Spec == Init /\ [][Next]_vars
=============================================================================

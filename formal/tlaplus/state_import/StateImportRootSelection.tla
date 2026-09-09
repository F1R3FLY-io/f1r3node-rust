-------------------------- MODULE StateImportRootSelection --------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS Actors, Roots, InitialRoot, Defect
VARIABLES rows, markers, currentRoot, phase, target, loadResult, writes, terminalWrites, result

None == "none"
Malformed == "malformed"
Phases == {"Idle", "Loading", "Loaded", "Committed", "Done"}
LoadResults == {"Unread", "Success", "Absent", "Malformed", "WrongHash", "Error"}
BadLoadResults == LoadResults \ {"Unread", "Success"}
Results == {"Unread", "Success", "Error", "Unknown"}
vars == <<rows, markers, currentRoot, phase, target, loadResult, writes, terminalWrites, result>>

ASSUME /\ Actors # {} /\ IsFiniteSet(Actors)
       /\ InitialRoot \in Roots /\ IsFiniteSet(Roots)
       /\ None \notin Roots /\ Malformed \notin Roots
       /\ Defect \in {"Safe", "SelectBeforeLoad", "SelectAfterLoadFailure"}

Init ==
    /\ rows \in [Roots -> Roots \cup {None, Malformed}]
    /\ rows[InitialRoot] = InitialRoot
    /\ markers \in SUBSET Roots /\ InitialRoot \in markers
    /\ currentRoot = InitialRoot
    /\ phase = [actor \in Actors |-> "Idle"]
    /\ target = [actor \in Actors |-> None]
    /\ loadResult = [actor \in Actors |-> "Unread"]
    /\ writes = [actor \in Actors |-> 0]
    /\ terminalWrites = [actor \in Actors |-> 0]
    /\ result = [actor \in Actors |-> "Unread"]

Begin(actor, root) ==
    /\ phase[actor] = "Idle" /\ root \in Roots
    /\ phase' = [phase EXCEPT ![actor] = "Loading"]
    /\ target' = [target EXCEPT ![actor] = root]
    /\ UNCHANGED <<rows, markers, currentRoot, loadResult, writes, terminalWrites, result>>

Load(actor, ioFailure) ==
    /\ phase[actor] = "Loading"
    /\ LET observed == rows[target[actor]]
           outcome == IF ioFailure THEN "Error" ELSE
               CASE observed = None -> "Absent"
               [] observed = Malformed -> "Malformed"
               [] observed # target[actor] -> "WrongHash"
               [] OTHER -> "Success"
       IN /\ loadResult' = [loadResult EXCEPT ![actor] = outcome]
          /\ phase' = [phase EXCEPT ![actor] = IF outcome = "Success" THEN "Loaded" ELSE "Done"]
          /\ result' = [result EXCEPT ![actor] = IF outcome = "Success" THEN "Unread" ELSE "Error"]
          /\ terminalWrites' = IF outcome = "Success" THEN terminalWrites
                ELSE [terminalWrites EXCEPT ![actor] = writes[actor]]
    /\ UNCHANGED <<rows, markers, currentRoot, target, writes>>

Select(actor) ==
    /\ phase[actor] = "Loaded" /\ target[actor] \in markers
    /\ currentRoot' = target[actor]
    /\ writes' = [writes EXCEPT ![actor] = @ + 1]
    /\ phase' = [phase EXCEPT ![actor] = "Committed"]
    /\ UNCHANGED <<rows, markers, target, loadResult, terminalWrites, result>>

SelectionAborts(actor, outcome) ==
    /\ phase[actor] = "Loaded" /\ outcome \in {"Error", "Unknown"}
    /\ phase' = [phase EXCEPT ![actor] = "Done"]
    /\ result' = [result EXCEPT ![actor] = outcome]
    /\ terminalWrites' = [terminalWrites EXCEPT ![actor] = writes[actor]]
    /\ UNCHANGED <<rows, markers, currentRoot, target, loadResult, writes>>

ObserveSelection(actor, outcome) ==
    /\ phase[actor] = "Committed" /\ outcome \in {"Success", "Unknown"}
    /\ phase' = [phase EXCEPT ![actor] = "Done"]
    /\ result' = [result EXCEPT ![actor] = outcome]
    /\ terminalWrites' = [terminalWrites EXCEPT ![actor] = writes[actor]]
    /\ UNCHANGED <<rows, markers, currentRoot, target, loadResult, writes>>

ConcurrentFill(root) ==
    /\ root \in Roots /\ rows[root] = None
    /\ rows' = [rows EXCEPT ![root] = root]
    /\ UNCHANGED <<markers, currentRoot, phase, target, loadResult, writes, terminalWrites, result>>

ConcurrentMark(root) ==
    /\ root \in Roots \ markers /\ rows[root] = root
    /\ markers' = markers \cup {root}
    /\ UNCHANGED <<rows, currentRoot, phase, target, loadResult, writes, terminalWrites, result>>

SelectBeforeLoad(actor) ==
    /\ Defect = "SelectBeforeLoad" /\ phase[actor] = "Loading" /\ writes[actor] = 0
    /\ currentRoot' = target[actor]
    /\ writes' = [writes EXCEPT ![actor] = @ + 1]
    /\ UNCHANGED <<rows, markers, phase, target, loadResult, terminalWrites, result>>

SelectAfterLoadFailure(actor) ==
    /\ Defect = "SelectAfterLoadFailure" /\ phase[actor] = "Done"
    /\ loadResult[actor] \in BadLoadResults /\ writes[actor] = 0
    /\ currentRoot' = target[actor]
    /\ writes' = [writes EXCEPT ![actor] = @ + 1]
    /\ UNCHANGED <<rows, markers, phase, target, loadResult, terminalWrites, result>>

Next ==
    \/ \E actor \in Actors :
        \/ \E root \in Roots : Begin(actor, root)
        \/ \E ioFailure \in BOOLEAN : Load(actor, ioFailure)
        \/ Select(actor)
        \/ \E outcome \in {"Error", "Unknown"} : SelectionAborts(actor, outcome)
        \/ \E outcome \in {"Success", "Unknown"} : ObserveSelection(actor, outcome)
        \/ SelectBeforeLoad(actor) \/ SelectAfterLoadFailure(actor)
    \/ \E root \in Roots : ConcurrentFill(root) \/ ConcurrentMark(root)

TypeOK ==
    /\ rows \in [Roots -> Roots \cup {None, Malformed}] /\ currentRoot \in Roots
    /\ markers \subseteq Roots /\ currentRoot \in markers
    /\ phase \in [Actors -> Phases] /\ target \in [Actors -> Roots \cup {None}]
    /\ loadResult \in [Actors -> LoadResults] /\ result \in [Actors -> Results]
    /\ writes \in [Actors -> Nat] /\ terminalWrites \in [Actors -> Nat]
SelectionRequiresSuccessfulLoad == \A actor \in Actors : writes[actor] > 0 => loadResult[actor] = "Success"
SelectionRequiresMarker == \A actor \in Actors : writes[actor] > 0 => target[actor] \in markers
FailedLoadHasNoSelectionWrite == \A actor \in Actors : loadResult[actor] \in BadLoadResults => writes[actor] = 0
CurrentRootHasCheckedRow == rows[currentRoot] = currentRoot
LoadedRowIsPreserved == \A actor \in Actors : loadResult[actor] = "Success" => rows[target[actor]] = target[actor]
TerminalSelectionDoesNotRepeat == \A actor \in Actors :
    /\ writes[actor] <= 1
    /\ (phase[actor] = "Done" => writes[actor] = terminalWrites[actor])
ReportedSelectionHasCommitted == \A actor \in Actors : result[actor] = "Success" =>
    /\ phase[actor] = "Done" /\ loadResult[actor] = "Success" /\ writes[actor] = 1
AllInvariants == TypeOK /\ SelectionRequiresSuccessfulLoad /\ SelectionRequiresMarker /\ FailedLoadHasNoSelectionWrite
    /\ CurrentRootHasCheckedRow /\ LoadedRowIsPreserved /\ TerminalSelectionDoesNotRepeat
    /\ ReportedSelectionHasCommitted
Spec == Init /\ [][Next]_vars
=============================================================================

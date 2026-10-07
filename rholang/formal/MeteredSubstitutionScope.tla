--------------------- MODULE MeteredSubstitutionScope ---------------------
EXTENDS Integers, Sequences

\* Exception boundary for the cost-branch recursive oracle versus the target
\* MeTTaIL/frontend substitution contract. The first two jobs have the same
\* structural reservation order in both implementations, but the EMatches
\* pattern is interpreted one target binder deeper. The last two jobs are
\* target-only EPathMap host work; source leaves that collection opaque.
CONSTANTS InitialCredit, BaseDepth
ASSUME InitialCredit \in Nat /\ BaseDepth \in Nat

Jobs == <<
    [slot |-> "matches_target", kind |-> "shared",
     sourceDepth |-> BaseDepth, targetDepth |-> BaseDepth, consensus |-> 0],
    [slot |-> "matches_pattern", kind |-> "shared",
     sourceDepth |-> BaseDepth, targetDepth |-> BaseDepth + 1, consensus |-> 0],
    [slot |-> "path_decode", kind |-> "target_only",
     sourceDepth |-> -1, targetDepth |-> BaseDepth, consensus |-> 0],
    [slot |-> "path_insert", kind |-> "target_only",
     sourceDepth |-> -1, targetDepth |-> BaseDepth, consensus |-> 0]
>>

RECURSIVE Shared(_)
Shared(events) ==
    IF Len(events) = 0 THEN <<>>
    ELSE IF Head(events).kind = "shared"
         THEN <<Head(events).slot>> \o Shared(Tail(events))
         ELSE Shared(Tail(events))

VARIABLES index, phase, credit, admitted, applied, consensus
vars == <<index, phase, credit, admitted, applied, consensus>>

Init ==
    /\ index = 1 /\ phase = "ready" /\ credit = InitialCredit
    /\ admitted = <<>> /\ applied = <<>> /\ consensus = 0

Reserve ==
    /\ phase = "ready" /\ index <= Len(Jobs) /\ credit > 0
    /\ admitted' = Append(admitted, Jobs[index])
    /\ credit' = credit - 1
    /\ phase' = "reserved"
    /\ UNCHANGED <<index, applied, consensus>>

Apply ==
    /\ phase = "reserved"
    /\ applied' = Append(applied, Jobs[index])
    /\ consensus' = consensus + Jobs[index].consensus
    /\ index' = index + 1
    /\ phase' = "ready"
    /\ UNCHANGED <<credit, admitted>>

Deny ==
    /\ phase = "ready" /\ index <= Len(Jobs) /\ credit = 0
    /\ phase' = "rejected"
    /\ UNCHANGED <<index, credit, admitted, applied, consensus>>

Finish ==
    /\ phase = "ready" /\ index > Len(Jobs)
    /\ phase' = "finished"
    /\ UNCHANGED <<index, credit, admitted, applied, consensus>>

Terminal == phase \in {"rejected", "finished"} /\ UNCHANGED vars
Next == Reserve \/ Apply \/ Deny \/ Finish \/ Terminal
Spec == Init /\ [][Next]_vars

InvAdmittedPrefix ==
    Len(admitted) <= Len(Jobs)
    /\ \A i \in 1..Len(admitted) : admitted[i] = Jobs[i]
InvAppliedPrefix ==
    Len(applied) <= Len(admitted)
    /\ \A i \in 1..Len(applied) : applied[i] = admitted[i]
InvOnePending == Len(admitted) - Len(applied) =
                 IF phase = "reserved" THEN 1 ELSE 0
InvCredit == credit + Len(admitted) = InitialCredit
InvScope ==
    \A i \in 1..Len(applied) :
        IF applied[i].slot = "matches_pattern"
        THEN applied[i].targetDepth = applied[i].sourceDepth + 1
        ELSE IF applied[i].kind = "shared"
             THEN applied[i].targetDepth = applied[i].sourceDepth
             ELSE applied[i].sourceDepth = -1
InvProjection == Shared(applied) = Shared(SubSeq(Jobs, 1, Len(applied)))
InvHostOnly == consensus = 0
               /\ \A i \in 1..Len(applied) :
                   applied[i].kind = "target_only" => applied[i].consensus = 0
InvRejected == phase = "rejected" => Len(admitted) = Len(applied)
InvFinished == phase = "finished" => Shared(applied) = <<"matches_target", "matches_pattern">>
=============================================================================

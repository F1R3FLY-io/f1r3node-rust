------------------------ MODULE MeteredMatcherPDA ------------------------
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS InitialA, InitialB
ASSUME InitialA \in Nat /\ InitialB \in Nat

Lanes == {"a", "b"}
Jobs == <<
    [lane |-> "a", kind |-> "match"],
    [lane |-> "b", kind |-> "clone"],
    [lane |-> "b", kind |-> "vec"],
    [lane |-> "a", kind |-> "match"]
>>
Initial == [lane \in Lanes |-> IF lane = "a" THEN InitialA ELSE InitialB]
PrefixCount(lane, n) == Cardinality({i \in 1..n : Jobs[i].lane = lane})

VARIABLES index, phase, credit, admitted, applied
vars == <<index, phase, credit, admitted, applied>>

Init ==
    /\ index = 1
    /\ phase = "ready"
    /\ credit = Initial
    /\ admitted = [lane \in Lanes |-> 0]
    /\ applied = [lane \in Lanes |-> 0]

Reserve ==
    /\ phase = "ready" /\ index <= Len(Jobs)
    /\ credit[Jobs[index].lane] > 0
    /\ credit' = [credit EXCEPT ![Jobs[index].lane] = @ - 1]
    /\ admitted' = [admitted EXCEPT ![Jobs[index].lane] = @ + 1]
    /\ phase' = "reserved"
    /\ UNCHANGED <<index, applied>>

Apply ==
    /\ phase = "reserved"
    /\ applied' = [applied EXCEPT ![Jobs[index].lane] = @ + 1]
    /\ index' = index + 1
    /\ phase' = "ready"
    /\ UNCHANGED <<credit, admitted>>

Deny ==
    /\ phase = "ready" /\ index <= Len(Jobs)
    /\ credit[Jobs[index].lane] = 0
    /\ phase' = "denied"
    /\ UNCHANGED <<index, credit, admitted, applied>>

ObserveError ==
    /\ phase = "denied"
    /\ phase' = "rejected"
    /\ UNCHANGED <<index, credit, admitted, applied>>

Finish ==
    /\ phase = "ready" /\ index > Len(Jobs)
    /\ phase' = "finished"
    /\ UNCHANGED <<index, credit, admitted, applied>>

Terminal == phase \in {"rejected", "finished"} /\ UNCHANGED vars
Next == Reserve \/ Apply \/ Deny \/ ObserveError \/ Finish \/ Terminal
Spec == Init /\ [][Next]_vars

InvConservation == \A lane \in Lanes : credit[lane] + admitted[lane] = Initial[lane]
InvNoUnmeteredWork == \A lane \in Lanes : applied[lane] <= admitted[lane]
InvLanePrefix == \A lane \in Lanes : applied[lane] = PrefixCount(lane, index - 1)
InvSinglePending ==
    \A lane \in Lanes :
        admitted[lane] - applied[lane] =
            IF phase = "reserved" THEN
                IF Jobs[index].lane = lane THEN 1 ELSE 0
            ELSE 0
InvTerminal ==
    phase \in {"rejected", "finished"} =>
        \A lane \in Lanes : admitted[lane] = applied[lane]
=============================================================================

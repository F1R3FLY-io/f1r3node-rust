---------------------- MODULE WitnessEquivalentCarrier ----------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
    \* @type: Set(Int);
    Nodes,
    \* @type: Set(Int);
    CarrierBlocks,
    \* @type: Set(Int);
    Digests,
    \* @type: Set(Int);
    Floors,
    \* @type: Set(Int);
    States,
    \* @type: Int;
    Protocol,
    \* @type: Int -> Int;
    LocalFloor,
    \* @type: Int -> Int;
    LocalState,
    \* @type: Int -> Int;
    LocalDigest,
    \* @type: Int -> Int;
    CarrierFloor,
    \* @type: Int -> Int;
    CarrierState,
    \* @type: Int -> Int;
    CarrierDigest,
    \* @type: Int -> Int;
    CarrierProtocol,
    \* @type: Int -> Bool;
    CarrierAccepted,
    \* @type: Int -> Bool;
    CarrierCausal,
    \* @type: Bool;
    ExactLocalDigestUnsafe,
    \* @type: Bool;
    FloorOnlyUnsafe,
    \* @type: Bool;
    CopyLocalDigestUnsafe,
    \* @type: Bool;
    WakeUnsafe

VARIABLES
    \* @type: Int -> Set(Int);
    admitted,
    \* @type: Int -> Bool;
    parked,
    \* @type: Int -> Bool;
    requested,
    \* @type: Int -> Bool;
    appended,
    \* @type: Int -> Int;
    selectedBlock,
    \* @type: Int -> Int;
    selectedDigest,
    \* @type: Int -> Set(Int);
    closureAtAppend

vars == << admitted, parked, requested, appended,
           selectedBlock, selectedDigest, closureAtAppend >>

None == 0

MC_LocalFloor == [n \in Nodes |-> 10]
MC_LocalState == [n \in Nodes |-> 20]
MC_LocalDigest == [n \in Nodes |-> IF n = 1 THEN 31 ELSE 32]
MC_CarrierFloor == [c \in CarrierBlocks |-> 10]
MC_CarrierState == [c \in CarrierBlocks |-> IF c = 3 THEN 21 ELSE 20]
MC_CarrierDigest == [c \in CarrierBlocks |-> 30 + c]
MC_CarrierProtocol == [c \in CarrierBlocks |-> 6]
MC_CarrierAccepted == [c \in CarrierBlocks |-> TRUE]
MC_CarrierCausal == [c \in CarrierBlocks |-> TRUE]

Least(S) == CHOOSE c \in S : \A d \in S : c <= d

SemanticEligible(n, known) ==
    {c \in known :
        CarrierAccepted[c]
        /\ CarrierCausal[c]
        /\ CarrierProtocol[c] = Protocol
        /\ CarrierFloor[c] = LocalFloor[n]
        /\ CarrierState[c] = LocalState[n]}

OperationalEligible(n, known) ==
    {c \in known :
        CarrierAccepted[c]
        /\ CarrierCausal[c]
        /\ CarrierProtocol[c] = Protocol
        /\ CarrierFloor[c] = LocalFloor[n]
        /\ (FloorOnlyUnsafe \/ CarrierState[c] = LocalState[n])
        /\ (~ExactLocalDigestUnsafe \/ CarrierDigest[c] = LocalDigest[n])}

Init ==
    /\ admitted = [n \in Nodes |-> {}]
    /\ parked = [n \in Nodes |-> FALSE]
    /\ requested = [n \in Nodes |-> FALSE]
    /\ appended = [n \in Nodes |-> FALSE]
    /\ selectedBlock = [n \in Nodes |-> None]
    /\ selectedDigest = [n \in Nodes |-> None]
    /\ closureAtAppend = [n \in Nodes |-> {}]

Admit(n, c) ==
    /\ c \notin admitted[n]
    /\ admitted' = [admitted EXCEPT ![n] = @ \cup {c}]
    /\ IF parked[n]
          /\ SemanticEligible(n, admitted[n] \cup {c}) # {}
          /\ ~WakeUnsafe
       THEN /\ parked' = [parked EXCEPT ![n] = FALSE]
            /\ requested' = [requested EXCEPT ![n] = TRUE]
       ELSE /\ UNCHANGED parked
            /\ UNCHANGED requested
    /\ UNCHANGED << appended, selectedBlock, selectedDigest, closureAtAppend >>

Evaluate(n) ==
    /\ ~appended[n]
    /\ LET eligible == OperationalEligible(n, admitted[n]) IN
       IF eligible = {}
       THEN /\ parked' = [parked EXCEPT ![n] = TRUE]
            /\ requested' = [requested EXCEPT ![n] = FALSE]
            /\ UNCHANGED << appended, selectedBlock,
                            selectedDigest, closureAtAppend >>
       ELSE LET carrier == Least(eligible) IN
            /\ appended' = [appended EXCEPT ![n] = TRUE]
            /\ parked' = [parked EXCEPT ![n] = FALSE]
            /\ requested' = [requested EXCEPT ![n] = FALSE]
            /\ selectedBlock' = [selectedBlock EXCEPT ![n] = carrier]
            /\ selectedDigest' = [selectedDigest EXCEPT
                  ![n] = IF CopyLocalDigestUnsafe
                        THEN LocalDigest[n]
                        ELSE CarrierDigest[carrier]]
            /\ closureAtAppend' = [closureAtAppend EXCEPT ![n] = admitted[n]]
    /\ UNCHANGED admitted

Restart(n) ==
    /\ ~appended[n]
    /\ parked' = [parked EXCEPT ![n] = FALSE]
    /\ requested' = [requested EXCEPT ![n] = FALSE]
    /\ UNCHANGED << admitted, appended, selectedBlock,
                    selectedDigest, closureAtAppend >>

Next ==
    \/ \E n \in Nodes, c \in CarrierBlocks : Admit(n, c)
    \/ \E n \in Nodes : Evaluate(n)
    \/ \E n \in Nodes : Restart(n)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ admitted \in [Nodes -> SUBSET CarrierBlocks]
    /\ parked \in [Nodes -> BOOLEAN]
    /\ requested \in [Nodes -> BOOLEAN]
    /\ appended \in [Nodes -> BOOLEAN]
    /\ selectedBlock \in [Nodes -> CarrierBlocks \cup {None}]
    /\ selectedDigest \in [Nodes -> Digests \cup {None}]
    /\ closureAtAppend \in [Nodes -> SUBSET CarrierBlocks]

SelectedCarrierHasExactSemanticState ==
    \A n \in Nodes : appended[n] =>
        /\ selectedBlock[n] \in closureAtAppend[n]
        /\ CarrierAccepted[selectedBlock[n]]
        /\ CarrierCausal[selectedBlock[n]]
        /\ CarrierProtocol[selectedBlock[n]] = Protocol
        /\ CarrierFloor[selectedBlock[n]] = LocalFloor[n]
        /\ CarrierState[selectedBlock[n]] = LocalState[n]

SelectedCarrierDigestIsPaired ==
    \A n \in Nodes : appended[n] =>
        selectedDigest[n] = CarrierDigest[selectedBlock[n]]

EquivalentSemanticViewsIgnoreLocalWitness ==
    \A left, right \in Nodes :
        appended[left] /\ appended[right]
        /\ LocalFloor[left] = LocalFloor[right]
        /\ LocalState[left] = LocalState[right]
        /\ closureAtAppend[left] = closureAtAppend[right]
        => selectedBlock[left] = selectedBlock[right]

SemanticCarrierCannotRemainParked ==
    \A n \in Nodes :
        parked[n] /\ SemanticEligible(n, admitted[n]) # {}
        => requested[n] \/ appended[n]

=============================================================================

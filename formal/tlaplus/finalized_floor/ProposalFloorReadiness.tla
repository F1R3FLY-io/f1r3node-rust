------------------------ MODULE ProposalFloorReadiness ------------------------
EXTENDS Integers, TLC

CONSTANTS
  \* @type: Int;
  MaxFloor,
  \* @type: Str;
  Defect

ASSUME /\ MaxFloor \in Nat \ {0}
       /\ Defect \in {
            "None",
            "PendingNoRequest",
            "NonFloorRequests",
            "BypassReadiness",
            "EqualityOnly",
            "NonStrictCandidate",
            "StateRegressiveMaterialize",
            "RetryContextMismatch"
          }

Nodes == {1, 2}
ProposalStates == {"Idle", "Deferred", "Created"}
Reasons == {
  "None",
  "FloorPending",
  "FloorRegression",
  "FloorConflict",
  "ContextMismatch",
  "IncompleteSlots",
  "InactiveProposer",
  "StalePermit"
}
Relations == {
  "SameContext",
  "AheadStrictPreserving",
  "Regression",
  "Conflict",
  "SameFloorMismatch",
  "AheadStateDropping",
  "AheadUncertified"
}

VARIABLES
  \* @type: Int -> Int;
  materializedFloor,
  \* @type: Int -> Int;
  candidateFloor,
  \* @type: Int -> Str;
  candidateRelation,
  \* @type: Int -> Bool;
  slotsComplete,
  \* @type: Int -> Bool;
  proposerActive,
  \* @type: Int -> Bool;
  permitRequired,
  \* @type: Int -> Bool;
  permitFresh,
  \* @type: Int -> Str;
  proposalState,
  \* @type: Int -> Str;
  deferralReason,
  \* @type: Int -> Bool;
  finalizationRequested,
  \* @type: Int -> Bool;
  floorDeferralObserved,
  \* @type: Int -> Bool;
  nonFloorRequestObserved,
  \* @type: Int -> Bool;
  badMaterializationObserved

vars == <<materializedFloor, candidateFloor, candidateRelation, slotsComplete,
          proposerActive, permitRequired, permitFresh, proposalState,
          deferralReason, finalizationRequested, floorDeferralObserved,
          nonFloorRequestObserved, badMaterializationObserved>>

RelationReason(relation) ==
  CASE relation = "SameContext" -> "None"
    [] relation = "AheadStrictPreserving" -> "FloorPending"
    [] relation = "Regression" -> "FloorRegression"
    [] relation = "Conflict" -> "FloorConflict"
    [] relation = "SameFloorMismatch" -> "ContextMismatch"
    [] relation = "AheadStateDropping" -> "FloorConflict"
    [] relation = "AheadUncertified" -> "FloorConflict"

ConfiguredRelationReason(relation) ==
  IF Defect = "EqualityOnly" /\ relation # "SameContext"
  THEN "FloorPending"
  ELSE IF Defect = "NonStrictCandidate" /\ relation = "AheadUncertified"
       THEN "FloorPending"
       ELSE IF Defect = "StateRegressiveMaterialize" /\ relation = "AheadStateDropping"
            THEN "FloorPending"
            ELSE IF Defect = "RetryContextMismatch" /\ relation = "SameFloorMismatch"
                 THEN "FloorPending"
                 ELSE RelationReason(relation)

ExpectedReason(node) ==
  IF permitRequired[node] /\ ~permitFresh[node]
  THEN "StalePermit"
  ELSE LET relationReason == ConfiguredRelationReason(candidateRelation[node])
       IN IF relationReason # "None"
          THEN relationReason
          ELSE IF ~slotsComplete[node]
               THEN "IncompleteSlots"
               ELSE IF ~proposerActive[node]
                    THEN "InactiveProposer"
                    ELSE "None"

Init ==
  /\ materializedFloor = [node \in Nodes |-> 0]
  /\ candidateFloor = [node \in Nodes |-> 0]
  /\ candidateRelation = [node \in Nodes |-> "SameContext"]
  /\ slotsComplete = [node \in Nodes |-> TRUE]
  /\ proposerActive = [node \in Nodes |-> TRUE]
  /\ permitRequired = [node \in Nodes |-> FALSE]
  /\ permitFresh = [node \in Nodes |-> TRUE]
  /\ proposalState = [node \in Nodes |-> "Idle"]
  /\ deferralReason = [node \in Nodes |-> "None"]
  /\ finalizationRequested = [node \in Nodes |-> FALSE]
  /\ floorDeferralObserved = [node \in Nodes |-> FALSE]
  /\ nonFloorRequestObserved = [node \in Nodes |-> FALSE]
  /\ badMaterializationObserved = [node \in Nodes |-> FALSE]

Attempt(node) ==
  /\ proposalState[node] # "Created"
  /\ LET reason == ExpectedReason(node)
         bypass == Defect = "BypassReadiness" /\ reason # "None"
         schedulesFloor == reason = "FloorPending" /\ Defect # "PendingNoRequest"
         schedulesNonFloor == reason \in (Reasons \ {"None", "FloorPending"})
                               /\ Defect = "NonFloorRequests"
     IN
       /\ proposalState' = [proposalState EXCEPT
            ![node] = IF reason = "None" \/ bypass THEN "Created" ELSE "Deferred"]
       /\ deferralReason' = [deferralReason EXCEPT
            ![node] = IF reason = "None" \/ bypass THEN "None" ELSE reason]
       /\ finalizationRequested' = [finalizationRequested EXCEPT
            ![node] = @ \/ schedulesFloor \/ schedulesNonFloor]
       /\ floorDeferralObserved' = [floorDeferralObserved EXCEPT
            ![node] = @ \/ reason = "FloorPending"]
       /\ nonFloorRequestObserved' = [nonFloorRequestObserved EXCEPT
            ![node] = @ \/ schedulesNonFloor]
  /\ UNCHANGED <<materializedFloor, candidateFloor, candidateRelation,
                  slotsComplete, proposerActive, permitRequired, permitFresh,
                  badMaterializationObserved>>

Materialize(node) ==
  /\ finalizationRequested[node]
  /\ candidateFloor[node] # materializedFloor[node]
  /\ candidateRelation[node] \in {
       "AheadStrictPreserving",
       IF Defect = "NonStrictCandidate" THEN "AheadUncertified" ELSE "AheadStrictPreserving",
       IF Defect = "StateRegressiveMaterialize" THEN "AheadStateDropping" ELSE "AheadStrictPreserving"
     }
  /\ materializedFloor' = [materializedFloor EXCEPT ![node] = candidateFloor[node]]
  /\ candidateRelation' = [candidateRelation EXCEPT ![node] = "SameContext"]
  /\ finalizationRequested' = [finalizationRequested EXCEPT ![node] = FALSE]
  /\ proposalState' = [proposalState EXCEPT ![node] = "Idle"]
  /\ deferralReason' = [deferralReason EXCEPT ![node] = "None"]
  /\ badMaterializationObserved' = [badMaterializationObserved EXCEPT
       ![node] = @ \/ candidateRelation[node] # "AheadStrictPreserving"]
  /\ UNCHANGED <<candidateFloor, slotsComplete, proposerActive,
                  permitRequired, permitFresh, floorDeferralObserved,
                  nonFloorRequestObserved>>

CandidateChoiceWellFormed(node, floor, relation) ==
  /\ floor \in 0..MaxFloor
  /\ relation \in Relations
  /\ ((floor = materializedFloor[node])
       <=> relation \in {"SameContext", "SameFloorMismatch"})

SetCandidate(node, floor, relation) ==
  /\ proposalState[node] # "Created"
  /\ CandidateChoiceWellFormed(node, floor, relation)
  /\ candidateFloor' = [candidateFloor EXCEPT ![node] = floor]
  /\ candidateRelation' = [candidateRelation EXCEPT ![node] = relation]
  /\ proposalState' = [proposalState EXCEPT ![node] = "Idle"]
  /\ deferralReason' = [deferralReason EXCEPT ![node] = "None"]
  /\ UNCHANGED <<materializedFloor, slotsComplete, proposerActive,
                  permitRequired, permitFresh, finalizationRequested,
                  floorDeferralObserved, nonFloorRequestObserved,
                  badMaterializationObserved>>

SetSlots(node, value) ==
  /\ proposalState[node] # "Created"
  /\ value \in BOOLEAN
  /\ slotsComplete' = [slotsComplete EXCEPT ![node] = value]
  /\ UNCHANGED <<materializedFloor, candidateFloor, candidateRelation,
                  proposerActive, permitRequired, permitFresh, proposalState,
                  deferralReason, finalizationRequested, floorDeferralObserved,
                  nonFloorRequestObserved, badMaterializationObserved>>

SetActive(node, value) ==
  /\ proposalState[node] # "Created"
  /\ value \in BOOLEAN
  /\ proposerActive' = [proposerActive EXCEPT ![node] = value]
  /\ UNCHANGED <<materializedFloor, candidateFloor, candidateRelation,
                  slotsComplete, permitRequired, permitFresh, proposalState,
                  deferralReason, finalizationRequested, floorDeferralObserved,
                  nonFloorRequestObserved, badMaterializationObserved>>

SetPermit(node, required, fresh) ==
  /\ proposalState[node] # "Created"
  /\ required \in BOOLEAN
  /\ fresh \in BOOLEAN
  /\ permitRequired' = [permitRequired EXCEPT ![node] = required]
  /\ permitFresh' = [permitFresh EXCEPT ![node] = fresh]
  /\ UNCHANGED <<materializedFloor, candidateFloor, candidateRelation,
                  slotsComplete, proposerActive, proposalState, deferralReason,
                  finalizationRequested, floorDeferralObserved,
                  nonFloorRequestObserved, badMaterializationObserved>>

ResetProposal(node) ==
  /\ proposalState[node] = "Created"
  /\ proposalState' = [proposalState EXCEPT ![node] = "Idle"]
  /\ deferralReason' = [deferralReason EXCEPT ![node] = "None"]
  /\ UNCHANGED <<materializedFloor, candidateFloor, candidateRelation,
                  slotsComplete, proposerActive, permitRequired, permitFresh,
                  finalizationRequested, floorDeferralObserved,
                  nonFloorRequestObserved, badMaterializationObserved>>

Next ==
  \/ \E node \in Nodes : Attempt(node)
  \/ \E node \in Nodes : Materialize(node)
  \/ \E node \in Nodes, floor \in 0..MaxFloor, relation \in Relations :
       SetCandidate(node, floor, relation)
  \/ \E node \in Nodes : SetSlots(node, TRUE) \/ SetSlots(node, FALSE)
  \/ \E node \in Nodes : SetActive(node, TRUE) \/ SetActive(node, FALSE)
  \/ \E node \in Nodes :
       SetPermit(node, FALSE, TRUE)
         \/ SetPermit(node, TRUE, TRUE)
         \/ SetPermit(node, TRUE, FALSE)
  \/ \E node \in Nodes : ResetProposal(node)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ materializedFloor \in [Nodes -> 0..MaxFloor]
  /\ candidateFloor \in [Nodes -> 0..MaxFloor]
  /\ candidateRelation \in [Nodes -> Relations]
  /\ slotsComplete \in [Nodes -> BOOLEAN]
  /\ proposerActive \in [Nodes -> BOOLEAN]
  /\ permitRequired \in [Nodes -> BOOLEAN]
  /\ permitFresh \in [Nodes -> BOOLEAN]
  /\ proposalState \in [Nodes -> ProposalStates]
  /\ deferralReason \in [Nodes -> Reasons]
  /\ finalizationRequested \in [Nodes -> BOOLEAN]
  /\ floorDeferralObserved \in [Nodes -> BOOLEAN]
  /\ nonFloorRequestObserved \in [Nodes -> BOOLEAN]
  /\ badMaterializationObserved \in [Nodes -> BOOLEAN]

Inv_CreationRequiresReadyContext ==
  \A node \in Nodes : proposalState[node] = "Created" => ExpectedReason(node) = "None"

Inv_FloorPendingRequestsFinalization ==
  \A node \in Nodes :
    proposalState[node] = "Deferred" /\ deferralReason[node] = "FloorPending"
      => finalizationRequested[node]

Inv_FloorPendingIsStrictStatePreserving ==
  \A node \in Nodes :
    proposalState[node] = "Deferred" /\ deferralReason[node] = "FloorPending"
      => candidateRelation[node] = "AheadStrictPreserving"

Inv_NonFloorDeferralDoesNotRequest ==
  \A node \in Nodes : ~nonFloorRequestObserved[node]

Inv_OnlyStrictStatePreservingFloorsMaterialize ==
  \A node \in Nodes : ~badMaterializationObserved[node]

Safety ==
  /\ TypeOK
  /\ Inv_CreationRequiresReadyContext
  /\ Inv_FloorPendingRequestsFinalization
  /\ Inv_FloorPendingIsStrictStatePreserving
  /\ Inv_NonFloorDeferralDoesNotRequest
  /\ Inv_OnlyStrictStatePreservingFloorsMaterialize

=============================================================================

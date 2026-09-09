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
            "DescendantGate",
            "ConflictGate",
            "CancelProposal",
            "NonStrictCandidate",
            "StateRegressiveMaterialize",
            "RetryContextMismatch"
          }

Nodes == {1, 2}
ProposalStates == {"Idle", "Deferred", "Created"}
Reasons == {
  "None",
  "CandidateGate",
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
  strictCandidateObserved,
  \* @type: Int -> Bool;
  nonFloorRequestObserved,
  \* @type: Int -> Bool;
  badMaterializationObserved,
  \* @type: Int -> Bool;
  proposalCancelledByFinalizer

vars == <<materializedFloor, candidateFloor, candidateRelation, slotsComplete,
          proposerActive, permitRequired, permitFresh, proposalState,
          deferralReason, finalizationRequested, strictCandidateObserved,
          nonFloorRequestObserved, badMaterializationObserved,
          proposalCancelledByFinalizer>>

ConfiguredRelationReason(relation) ==
  IF Defect = "EqualityOnly" /\ relation # "SameContext"
  THEN "CandidateGate"
  ELSE IF Defect = "DescendantGate" /\ relation = "AheadStrictPreserving"
       THEN "CandidateGate"
       ELSE IF Defect = "ConflictGate"
               /\ relation \in {"Regression", "Conflict", "SameFloorMismatch"}
            THEN "CandidateGate"
            ELSE "None"

CandidateRequestsFinalization(relation) ==
  \/ relation = "AheadStrictPreserving"
  \/ Defect = "NonStrictCandidate" /\ relation = "AheadUncertified"
  \/ Defect = "StateRegressiveMaterialize" /\ relation = "AheadStateDropping"
  \/ Defect = "RetryContextMismatch" /\ relation = "SameFloorMismatch"

CertifiedReason(node) ==
  IF permitRequired[node] /\ ~permitFresh[node]
  THEN "StalePermit"
  ELSE IF ~slotsComplete[node]
       THEN "IncompleteSlots"
       ELSE IF ~proposerActive[node]
            THEN "InactiveProposer"
            ELSE "None"

ExpectedReason(node) ==
  LET certifiedReason == CertifiedReason(node)
      relationReason == ConfiguredRelationReason(candidateRelation[node])
  IN IF certifiedReason # "None" THEN certifiedReason ELSE relationReason

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
  /\ strictCandidateObserved = [node \in Nodes |-> FALSE]
  /\ nonFloorRequestObserved = [node \in Nodes |-> FALSE]
  /\ badMaterializationObserved = [node \in Nodes |-> FALSE]
  /\ proposalCancelledByFinalizer = [node \in Nodes |-> FALSE]

Attempt(node) ==
  /\ proposalState[node] # "Created"
  /\ LET reason == ExpectedReason(node)
         bypass == Defect = "BypassReadiness" /\ reason # "None"
     IN
       /\ proposalState' = [proposalState EXCEPT
            ![node] = IF reason = "None" \/ bypass THEN "Created" ELSE "Deferred"]
       /\ deferralReason' = [deferralReason EXCEPT
            ![node] = IF reason = "None" \/ bypass THEN "None" ELSE reason]
  /\ UNCHANGED <<materializedFloor, candidateFloor, candidateRelation,
                  slotsComplete, proposerActive, permitRequired, permitFresh,
                  finalizationRequested, strictCandidateObserved,
                  nonFloorRequestObserved, badMaterializationObserved,
                  proposalCancelledByFinalizer>>

ObserveCandidate(node) ==
  LET relation == candidateRelation[node]
      strict == relation = "AheadStrictPreserving"
      schedulesCandidate == CandidateRequestsFinalization(relation)
                              /\ ~(Defect = "PendingNoRequest" /\ strict)
      schedulesNonFloor == relation # "AheadStrictPreserving"
                            /\ relation # "SameContext"
                            /\ Defect = "NonFloorRequests"
  IN
    /\ finalizationRequested' = [finalizationRequested EXCEPT
         ![node] = @ \/ schedulesCandidate \/ schedulesNonFloor]
    /\ strictCandidateObserved' = [strictCandidateObserved EXCEPT
         ![node] = @ \/ strict]
    /\ nonFloorRequestObserved' = [nonFloorRequestObserved EXCEPT
         ![node] = @ \/ schedulesNonFloor]
    /\ UNCHANGED <<materializedFloor, candidateFloor, candidateRelation,
                    slotsComplete, proposerActive, permitRequired, permitFresh,
                    proposalState, deferralReason, badMaterializationObserved,
                    proposalCancelledByFinalizer>>

Materialize(node) ==
  /\ finalizationRequested[node]
  /\ (candidateFloor[node] # materializedFloor[node]
       \/ candidateRelation[node] = "SameFloorMismatch")
  /\ candidateRelation[node] \in {
       "AheadStrictPreserving",
       IF Defect = "NonStrictCandidate" THEN "AheadUncertified" ELSE "AheadStrictPreserving",
       IF Defect = "StateRegressiveMaterialize" THEN "AheadStateDropping" ELSE "AheadStrictPreserving",
       IF Defect = "RetryContextMismatch" THEN "SameFloorMismatch" ELSE "AheadStrictPreserving"
     }
  /\ materializedFloor' = [materializedFloor EXCEPT ![node] = candidateFloor[node]]
  /\ candidateRelation' = [candidateRelation EXCEPT ![node] = "SameContext"]
  /\ finalizationRequested' = [finalizationRequested EXCEPT ![node] = FALSE]
  /\ proposalState' = [proposalState EXCEPT
       ![node] = IF Defect = "CancelProposal" THEN "Idle" ELSE @]
  /\ deferralReason' = [deferralReason EXCEPT
       ![node] = IF Defect = "CancelProposal" THEN "None" ELSE @]
  /\ strictCandidateObserved' = [strictCandidateObserved EXCEPT ![node] = FALSE]
  /\ badMaterializationObserved' = [badMaterializationObserved EXCEPT
       ![node] = @ \/ candidateRelation[node] # "AheadStrictPreserving"]
  /\ proposalCancelledByFinalizer' = [proposalCancelledByFinalizer EXCEPT
       ![node] = @ \/ (proposalState[node] = "Created"
                         /\ proposalState'[node] # "Created")]
  /\ UNCHANGED <<candidateFloor, slotsComplete, proposerActive,
                  permitRequired, permitFresh,
                  nonFloorRequestObserved>>

CandidateChoiceWellFormed(node, floor, relation) ==
  /\ floor \in 0..MaxFloor
  /\ relation \in Relations
  /\ ((floor = materializedFloor[node])
       <=> relation \in {"SameContext", "SameFloorMismatch"})

SetCandidate(node, floor, relation) ==
  /\ ~finalizationRequested[node]
  /\ CandidateChoiceWellFormed(node, floor, relation)
  /\ candidateFloor' = [candidateFloor EXCEPT ![node] = floor]
  /\ candidateRelation' = [candidateRelation EXCEPT ![node] = relation]
  /\ strictCandidateObserved' = [strictCandidateObserved EXCEPT ![node] = FALSE]
  /\ UNCHANGED <<materializedFloor, slotsComplete, proposerActive,
                  permitRequired, permitFresh, proposalState, deferralReason,
                  finalizationRequested, nonFloorRequestObserved,
                  badMaterializationObserved, proposalCancelledByFinalizer>>

SetSlots(node, value) ==
  /\ proposalState[node] # "Created"
  /\ value \in BOOLEAN
  /\ slotsComplete' = [slotsComplete EXCEPT ![node] = value]
  /\ proposalState' = [proposalState EXCEPT ![node] = "Idle"]
  /\ deferralReason' = [deferralReason EXCEPT ![node] = "None"]
  /\ UNCHANGED <<materializedFloor, candidateFloor, candidateRelation,
                  proposerActive, permitRequired, permitFresh,
                  finalizationRequested, strictCandidateObserved,
                  nonFloorRequestObserved, badMaterializationObserved,
                  proposalCancelledByFinalizer>>

SetActive(node, value) ==
  /\ proposalState[node] # "Created"
  /\ value \in BOOLEAN
  /\ proposerActive' = [proposerActive EXCEPT ![node] = value]
  /\ proposalState' = [proposalState EXCEPT ![node] = "Idle"]
  /\ deferralReason' = [deferralReason EXCEPT ![node] = "None"]
  /\ UNCHANGED <<materializedFloor, candidateFloor, candidateRelation,
                  slotsComplete, permitRequired, permitFresh,
                  finalizationRequested, strictCandidateObserved,
                  nonFloorRequestObserved, badMaterializationObserved,
                  proposalCancelledByFinalizer>>

SetPermit(node, required, fresh) ==
  /\ proposalState[node] # "Created"
  /\ required \in BOOLEAN
  /\ fresh \in BOOLEAN
  /\ permitRequired' = [permitRequired EXCEPT ![node] = required]
  /\ permitFresh' = [permitFresh EXCEPT ![node] = fresh]
  /\ proposalState' = [proposalState EXCEPT ![node] = "Idle"]
  /\ deferralReason' = [deferralReason EXCEPT ![node] = "None"]
  /\ UNCHANGED <<materializedFloor, candidateFloor, candidateRelation,
                  slotsComplete, proposerActive,
                  finalizationRequested, strictCandidateObserved,
                  nonFloorRequestObserved, badMaterializationObserved,
                  proposalCancelledByFinalizer>>

ResetProposal(node) ==
  /\ proposalState[node] = "Created"
  /\ proposalState' = [proposalState EXCEPT ![node] = "Idle"]
  /\ deferralReason' = [deferralReason EXCEPT ![node] = "None"]
  /\ UNCHANGED <<materializedFloor, candidateFloor, candidateRelation,
                  slotsComplete, proposerActive, permitRequired, permitFresh,
                  finalizationRequested, strictCandidateObserved,
                  nonFloorRequestObserved, badMaterializationObserved,
                  proposalCancelledByFinalizer>>

Next ==
  \/ \E node \in Nodes : Attempt(node)
  \/ \E node \in Nodes : ObserveCandidate(node)
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
  /\ strictCandidateObserved \in [Nodes -> BOOLEAN]
  /\ nonFloorRequestObserved \in [Nodes -> BOOLEAN]
  /\ badMaterializationObserved \in [Nodes -> BOOLEAN]
  /\ proposalCancelledByFinalizer \in [Nodes -> BOOLEAN]

Inv_CreationRequiresReadyContext ==
  \A node \in Nodes : proposalState[node] = "Created" => CertifiedReason(node) = "None"

Inv_StrictCandidateRequestsFinalization ==
  \A node \in Nodes :
    strictCandidateObserved[node] => finalizationRequested[node]

Inv_OnlyStrictCandidatesRequestFinalization ==
  \A node \in Nodes :
    finalizationRequested[node] => candidateRelation[node] = "AheadStrictPreserving"

Inv_CandidateEvidenceDoesNotGateProposal ==
  \A node \in Nodes :
    proposalState[node] = "Deferred" => CertifiedReason(node) # "None"

Inv_NonCandidateEvidenceDoesNotRequestFinalization ==
  \A node \in Nodes : ~nonFloorRequestObserved[node]

Inv_OnlyStrictStatePreservingFloorsMaterialize ==
  \A node \in Nodes : ~badMaterializationObserved[node]

Inv_FinalizerDoesNotCancelProposal ==
  \A node \in Nodes : ~proposalCancelledByFinalizer[node]

Safety ==
  /\ TypeOK
  /\ Inv_CreationRequiresReadyContext
  /\ Inv_StrictCandidateRequestsFinalization
  /\ Inv_OnlyStrictCandidatesRequestFinalization
  /\ Inv_CandidateEvidenceDoesNotGateProposal
  /\ Inv_NonCandidateEvidenceDoesNotRequestFinalization
  /\ Inv_OnlyStrictStatePreservingFloorsMaterialize
  /\ Inv_FinalizerDoesNotCancelProposal

=============================================================================

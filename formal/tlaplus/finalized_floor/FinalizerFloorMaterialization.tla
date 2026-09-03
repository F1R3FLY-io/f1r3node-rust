------------------ MODULE FinalizerFloorMaterialization ------------------
EXTENDS Integers, FiniteSets, TLC

CONSTANT
  \* @type: Str;
  Defect

ASSUME Defect \in {"None", "MainParentOnly", "CausalOnlyRejected"}

Nodes == {"n1", "n2"}
Validators == {"v1", "v2", "v3", "v4"}
Blocks == {"G", "S1", "S2", "S3", "M"}
Floors == {"G", "S2", "S3"}
ProposalStates == {"Idle", "Deferred", "Created"}
NoTarget == "NoTarget"
Targets == Floors \cup {NoTarget}

Weight(validator) ==
  CASE validator = "v1" -> 1
    [] validator = "v2" -> 3
    [] validator = "v3" -> 5
    [] OTHER -> 7

WeightOf(validators) ==
    (IF "v1" \in validators THEN 1 ELSE 0)
  + (IF "v2" \in validators THEN 3 ELSE 0)
  + (IF "v3" \in validators THEN 5 ELSE 0)
  + (IF "v4" \in validators THEN 7 ELSE 0)

Tip == [validator \in Validators |->
  CASE validator = "v1" -> "S1"
    [] validator = "v2" -> "S2"
    [] validator = "v3" -> "S3"
    [] OTHER -> "M"]

MainPast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "S1" -> {"G", "S1"}
    [] block = "S2" -> {"G", "S2"}
    [] block = "S3" -> {"G", "S3"}
    [] OTHER -> {"G", "S1", "M"}]

DagPast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "S1" -> {"G", "S1"}
    [] block = "S2" -> {"G", "S2"}
    [] block = "S3" -> {"G", "S3"}
    [] OTHER -> {"G", "S1", "S2", "S3", "M"}]

StatePast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "S1" -> {"G", "S1"}
    [] block = "S2" -> {"G", "S2"}
    [] block = "S3" -> {"G", "S3"}
    [] OTHER -> {"G", "S1", "S3", "M"}]

VARIABLES
  \* @type: Str -> Set(Str);
  known,
  \* @type: Str -> Str;
  materializedFloor,
  \* @type: Str -> Str;
  requestedTarget,
  \* @type: Str -> Str;
  proposalState,
  \* @type: Str -> Bool;
  permanentPendingObserved,
  \* @type: Str -> Bool;
  wrongTargetObserved

vars == <<known, materializedFloor, requestedTarget, proposalState,
          permanentPendingObserved, wrongTargetObserved>>

KnownTips(node) == {Tip[validator] : validator \in known[node]}

Supporters(node, target, past) ==
  {validator \in known[node] : target \in past[Tip[validator]]}

StrictlyCertified(node, target, past) ==
  target = "G" \/ WeightOf(Supporters(node, target, past)) > 8

CausallyCertified(node, target) ==
  StrictlyCertified(node, target, DagPast)

StateCertified(node, target) ==
  StrictlyCertified(node, target, StatePast)

CandidateFloor(node) ==
  IF CausallyCertified(node, "S3") /\ StateCertified(node, "S3")
  THEN "S3"
  ELSE "G"

FinalizerPast == IF Defect = "MainParentOnly" THEN MainPast ELSE DagPast

FinalizerCausallyCertified(node, target) ==
  StrictlyCertified(node, target, FinalizerPast)

FinalizerStateCertified(node, target) ==
  IF Defect = "CausalOnlyRejected"
  THEN TRUE
  ELSE StateCertified(node, target)

SelectedTarget(node) ==
  IF Defect = "CausalOnlyRejected" THEN "S2" ELSE requestedTarget[node]

Init ==
  /\ known = [node \in Nodes |-> {}]
  /\ materializedFloor = [node \in Nodes |-> "G"]
  /\ requestedTarget = [node \in Nodes |-> NoTarget]
  /\ proposalState = [node \in Nodes |-> "Idle"]
  /\ permanentPendingObserved = [node \in Nodes |-> FALSE]
  /\ wrongTargetObserved = [node \in Nodes |-> FALSE]

Deliver(node, validator) ==
  /\ validator \notin known[node]
  /\ known' = [known EXCEPT ![node] = @ \union {validator}]
  /\ proposalState' = [proposalState EXCEPT ![node] = "Idle"]
  /\ UNCHANGED <<materializedFloor, requestedTarget,
                  permanentPendingObserved, wrongTargetObserved>>

Attempt(node) ==
  /\ proposalState[node] # "Created"
  /\ LET candidate == CandidateFloor(node)
     IN /\ proposalState' = [proposalState EXCEPT
              ![node] = IF candidate = materializedFloor[node]
                         THEN "Created" ELSE "Deferred"]
        /\ requestedTarget' = [requestedTarget EXCEPT
              ![node] = IF candidate = materializedFloor[node]
                         THEN NoTarget ELSE candidate]
        /\ permanentPendingObserved' = [permanentPendingObserved EXCEPT
              ![node] = @ \/ (candidate # materializedFloor[node]
                /\ ~FinalizerCausallyCertified(node, candidate))]
  /\ UNCHANGED <<known, materializedFloor, wrongTargetObserved>>

Materialize(node) ==
  /\ requestedTarget[node] # NoTarget
  /\ LET target == SelectedTarget(node)
     IN /\ FinalizerCausallyCertified(node, target)
        /\ FinalizerStateCertified(node, target)
        /\ target \in Floors
        /\ materializedFloor' = [materializedFloor EXCEPT ![node] = target]
        /\ wrongTargetObserved' = [wrongTargetObserved EXCEPT
              ![node] = @ \/ target # requestedTarget[node]]
  /\ requestedTarget' = [requestedTarget EXCEPT ![node] = NoTarget]
  /\ proposalState' = [proposalState EXCEPT ![node] = "Idle"]
  /\ UNCHANGED <<known, permanentPendingObserved>>

Next ==
  \/ \E node \in Nodes, validator \in Validators : Deliver(node, validator)
  \/ \E node \in Nodes : Attempt(node)
  \/ \E node \in Nodes : Materialize(node)

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A node \in Nodes, validator \in Validators :
       WF_vars(Deliver(node, validator))
  /\ \A node \in Nodes : WF_vars(Attempt(node))
  /\ \A node \in Nodes : WF_vars(Materialize(node))

TypeOK ==
  /\ known \in [Nodes -> SUBSET Validators]
  /\ materializedFloor \in [Nodes -> Floors]
  /\ requestedTarget \in [Nodes -> Targets]
  /\ proposalState \in [Nodes -> ProposalStates]
  /\ permanentPendingObserved \in [Nodes -> BOOLEAN]
  /\ wrongTargetObserved \in [Nodes -> BOOLEAN]

Inv_StrictHalfDoesNotCertify ==
  \A node \in Nodes :
    known[node] = Validators => ~CausallyCertified(node, "S1")

Inv_RejectedSiblingIsNotStateCertified ==
  \A node \in Nodes :
    known[node] = Validators => ~StateCertified(node, "S2")

Inv_CompleteCoverageFindsSecondaryFloor ==
  \A node \in Nodes :
    known[node] = Validators => CandidateFloor(node) = "S3"

Inv_FinalizerDiscoversCandidate ==
  \A node \in Nodes :
    known[node] = Validators =>
      FinalizerCausallyCertified(node, CandidateFloor(node))

Inv_SelectedTargetBindsRequestedCertificate ==
  \A node \in Nodes :
    requestedTarget[node] # NoTarget =>
      SelectedTarget(node) = requestedTarget[node]

Inv_RequestedTargetIsDualCertified ==
  \A node \in Nodes :
    requestedTarget[node] # NoTarget =>
      /\ FinalizerCausallyCertified(node, requestedTarget[node])
      /\ StateCertified(node, requestedTarget[node])

Inv_MaterializedFloorIsStateCertified ==
  \A node \in Nodes : StateCertified(node, materializedFloor[node])

Inv_MaterializationUsesRequestedTarget ==
  \A node \in Nodes : ~wrongTargetObserved[node]

Inv_NoCertifiedPendingStarvation ==
  \A node \in Nodes : ~permanentPendingObserved[node]

Live_AllNodesMaterializeSecondaryFloor ==
  <>(\A node \in Nodes : materializedFloor[node] = "S3")
=============================================================================

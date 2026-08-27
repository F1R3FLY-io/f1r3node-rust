----------------------- MODULE StateLineageFinality -----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
  \* @type: Bool;
  EnforceStateLineage,
  \* @type: Bool;
  EnforceStateSupport,
  \* @type: Bool;
  RequireMainLineage,
  \* @type: Int;
  ThresholdNum,
  \* @type: Int;
  ThresholdDen,
  \* @type: Int;
  TotalStake

ASSUME /\ EnforceStateLineage \in BOOLEAN
       /\ EnforceStateSupport \in BOOLEAN
       /\ RequireMainLineage \in BOOLEAN
       /\ ThresholdDen > 0
       /\ TotalStake > 0

Blocks == {"G", "F", "X", "S", "P", "R"}
Nodes == {"n1", "n2"}
Candidates == {"S", "P", "R"}
Validators == {"v1", "v2", "v3"}

Stake == [validator \in Validators |->
  CASE validator = "v1" -> 60
    [] validator = "v2" -> 20
    [] OTHER -> 15]

CausalAgreeing == [block \in Blocks |->
  CASE block = "X" -> {}
    [] block \in {"S", "R"} -> {"v1"}
    [] block = "P" -> {"v1", "v2"}
    [] OTHER -> Validators]

StateAgreeing == [block \in Blocks |->
  CASE block = "X" -> {}
    [] block \in {"S", "R"} -> {"v1"}
    [] block = "P" -> {"v2"}
    [] OTHER -> Validators]

WeightOf(validators) ==
  (IF "v1" \in validators THEN Stake["v1"] ELSE 0) +
  (IF "v2" \in validators THEN Stake["v2"] ELSE 0) +
  (IF "v3" \in validators THEN Stake["v3"] ELSE 0)

CausalCliqueWeight(block) == WeightOf(CausalAgreeing[block])
StateCliqueWeight(block) == WeightOf(StateAgreeing[block])

ExactStrictVote(weight) ==
  /\ 2 * weight > TotalStake
  /\ 2 * weight * ThresholdDen
       > TotalStake * (ThresholdDen + ThresholdNum)

CausalCertified ==
  {block \in Blocks : ExactStrictVote(CausalCliqueWeight(block))}
StateCertified ==
  {block \in Blocks : ExactStrictVote(StateCliqueWeight(block))}

MainPast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "F" -> {"G", "F"}
    [] block = "X" -> {"G", "X"}
    [] block = "S" -> {"G", "F", "S"}
    [] block = "P" -> {"G", "F", "P"}
    [] OTHER -> {"G", "X", "R"}]

DagPast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "F" -> {"G", "F"}
    [] block = "X" -> {"G", "X"}
    [] block = "S" -> {"G", "F", "S"}
    [] block = "P" -> {"G", "F", "P"}
    [] OTHER -> {"G", "F", "X", "R"}]

StatePast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "F" -> {"G", "F"}
    [] block = "X" -> {"G", "X"}
    [] block = "S" -> {"G", "S"}
    [] block = "P" -> {"G", "F", "P"}
    [] OTHER -> {"G", "F", "R"}]

MainDescendant(ancestor, descendant) == ancestor \in MainPast[descendant]
DagDescendant(ancestor, descendant) == ancestor \in DagPast[descendant]
StateDescendant(ancestor, descendant) == ancestor \in StatePast[descendant]

VARIABLES
  \* @type: Str -> Str;
  lfb,
  \* @type: Str -> Set(Str);
  committed,
  \* @type: Str -> Set(Str);
  known

vars == <<lfb, committed, known>>

Init ==
  /\ lfb = [node \in Nodes |-> "F"]
  /\ committed = [node \in Nodes |-> {"G", "F"}]
  /\ known = [node \in Nodes |-> {}]

Admissible(current, candidate) ==
  /\ candidate \in CausalCertified
  /\ (~EnforceStateSupport \/ candidate \in StateCertified)
  /\ candidate # current
  /\ (~RequireMainLineage \/ MainDescendant(current, candidate))
  /\ (~EnforceStateLineage \/ StateDescendant(current, candidate))

Eligible(node, candidate) ==
  /\ candidate \in known[node]
  /\ Admissible(lfb[node], candidate)

Deliver(node, candidate) ==
  /\ candidate \notin known[node]
  /\ known' = [known EXCEPT ![node] = @ \union {candidate}]
  /\ UNCHANGED <<lfb, committed>>

Promote(node, candidate) ==
  /\ Eligible(node, candidate)
  /\ lfb' = [lfb EXCEPT ![node] = candidate]
  /\ committed' = [committed EXCEPT ![node] = @ \union {candidate}]
  /\ UNCHANGED known

Idle == UNCHANGED vars

Next ==
  \/ \E node \in Nodes, candidate \in Candidates : Deliver(node, candidate)
  \/ \E node \in Nodes, candidate \in Candidates : Promote(node, candidate)
  \/ Idle

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A node \in Nodes, candidate \in Candidates : WF_vars(Deliver(node, candidate))
  /\ \A node \in Nodes : WF_vars(Promote(node, "R"))

TypeOK ==
  /\ lfb \in [Nodes -> Blocks]
  /\ committed \in [Nodes -> SUBSET Blocks]
  /\ known \in [Nodes -> SUBSET Candidates]

Inv_AllCommittedStatesRemainInLineage ==
  \A node \in Nodes : committed[node] \subseteq StatePast[lfb[node]]

Inv_CliqueCertificateIsUnchanged ==
  CausalCertified = {"G", "F", "S", "P", "R"}

Inv_AsymmetricStakeTopology ==
  /\ TotalStake = WeightOf(Validators)
  /\ Stake["v1"] = 60
  /\ Stake["v2"] = 20
  /\ Stake["v3"] = 15
  /\ CausalCliqueWeight("S") = 60
  /\ CausalCliqueWeight("P") = 80
  /\ StateCliqueWeight("P") = 20
  /\ CausalCliqueWeight("R") = 60
  /\ StateCliqueWeight("R") = 60

Inv_StaleMergeSeparatesDagAndState ==
  /\ MainDescendant("F", "S")
  /\ DagDescendant("F", "S")
  /\ ~StateDescendant("F", "S")
  /\ "S" \in CausalCertified

Inv_OffMainRebaseRestoresEligibility ==
  /\ ~MainDescendant("F", "R")
  /\ DagDescendant("F", "R")
  /\ StateDescendant("F", "R")
  /\ "R" \in CausalCertified
  /\ "R" \in StateCertified
  /\ Admissible("F", "R")

Inv_CausalMergeVoteIsNotStateSupport ==
  /\ "P" \in CausalCertified
  /\ "P" \notin StateCertified
  /\ StateDescendant("F", "P")
  /\ ~Admissible("F", "P")

Inv_NoUnsupportedStateFloor ==
  \A node \in Nodes : lfb[node] # "P"

Live_RebaseProgress == <>(\A node \in Nodes : lfb[node] = "R")
=============================================================================

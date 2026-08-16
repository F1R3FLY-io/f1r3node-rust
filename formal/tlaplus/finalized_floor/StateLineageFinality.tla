----------------------- MODULE StateLineageFinality -----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
  \* @type: Bool;
  EnforceStateLineage,
  \* @type: Bool;
  RequireMainLineage,
  \* @type: Int;
  ThresholdNum,
  \* @type: Int;
  ThresholdDen,
  \* @type: Int;
  TotalStake

ASSUME /\ EnforceStateLineage \in BOOLEAN
       /\ RequireMainLineage \in BOOLEAN
       /\ ThresholdDen > 0
       /\ TotalStake > 0

Blocks == {"G", "F", "X", "S", "R"}
Nodes == {"n1", "n2"}
Candidates == {"S", "R"}
Validators == {"v1", "v2", "v3"}

Stake == [validator \in Validators |->
  CASE validator = "v1" -> 60
    [] validator = "v2" -> 20
    [] OTHER -> 15]

Agreeing == [block \in Blocks |->
  CASE block = "X" -> {}
    [] block \in {"S", "R"} -> {"v1"}
    [] OTHER -> Validators]

WeightOf(validators) ==
  (IF "v1" \in validators THEN Stake["v1"] ELSE 0) +
  (IF "v2" \in validators THEN Stake["v2"] ELSE 0) +
  (IF "v3" \in validators THEN Stake["v3"] ELSE 0)

CliqueWeight(block) == WeightOf(Agreeing[block])

ExactStrictVote(block) ==
  /\ 2 * CliqueWeight(block) > TotalStake
  /\ 2 * CliqueWeight(block) * ThresholdDen
       > TotalStake * (ThresholdDen + ThresholdNum)

Certified == {block \in Blocks : ExactStrictVote(block)}

MainPast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "F" -> {"G", "F"}
    [] block = "X" -> {"G", "X"}
    [] block = "S" -> {"G", "F", "S"}
    [] OTHER -> {"G", "X", "R"}]

DagPast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "F" -> {"G", "F"}
    [] block = "X" -> {"G", "X"}
    [] block = "S" -> {"G", "F", "S"}
    [] OTHER -> {"G", "F", "X", "R"}]

StatePast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "F" -> {"G", "F"}
    [] block = "X" -> {"G", "X"}
    [] block = "S" -> {"G", "S"}
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
  /\ candidate \in Certified
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

Inv_CliqueCertificateIsUnchanged == Certified = {"G", "F", "S", "R"}

Inv_AsymmetricStakeTopology ==
  /\ TotalStake = WeightOf(Validators)
  /\ Stake["v1"] = 60
  /\ Stake["v2"] = 20
  /\ Stake["v3"] = 15
  /\ CliqueWeight("S") = 60
  /\ CliqueWeight("R") = 60

Inv_StaleMergeSeparatesDagAndState ==
  /\ MainDescendant("F", "S")
  /\ DagDescendant("F", "S")
  /\ ~StateDescendant("F", "S")
  /\ "S" \in Certified

Inv_OffMainRebaseRestoresEligibility ==
  /\ ~MainDescendant("F", "R")
  /\ DagDescendant("F", "R")
  /\ StateDescendant("F", "R")
  /\ "R" \in Certified
  /\ Admissible("F", "R")

Live_RebaseProgress == <>(\A node \in Nodes : lfb[node] = "R")
=============================================================================

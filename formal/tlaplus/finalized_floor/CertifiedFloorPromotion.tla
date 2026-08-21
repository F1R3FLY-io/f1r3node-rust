-------------------- MODULE CertifiedFloorPromotion --------------------
EXTENDS Naturals, FiniteSets

CONSTANT
  \* @type: Bool;
  UseCausalClosure

ASSUME UseCausalClosure \in BOOLEAN

Nodes == {"n1", "n2"}
Validators == {"v1", "v2", "v3"}
Blocks == {"G", "F", "A", "B", "M1", "M2", "M3"}

Tip == [validator \in Validators |->
  CASE validator = "v1" -> "M1"
    [] validator = "v2" -> "M2"
    [] OTHER -> "M3"]

MainPast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "F" -> {"G", "F"}
    [] block = "A" -> {"G", "A"}
    [] block = "B" -> {"G", "B"}
    [] block = "M1" -> {"G", "A", "M1"}
    [] block = "M2" -> {"G", "B", "M2"}
    [] OTHER -> {"G", "A", "M3"}]

DagPast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "F" -> {"G", "F"}
    [] block = "A" -> {"G", "A"}
    [] block = "B" -> {"G", "B"}
    [] block = "M1" -> {"G", "F", "A", "M1"}
    [] block = "M2" -> {"G", "F", "B", "M2"}
    [] OTHER -> {"G", "F", "A", "M3"}]

Certified == {"G", "F"}
StateCertified == {"G", "F"}

VARIABLES
  \* @type: Str -> Set(Str);
  known,
  \* @type: Str -> Str;
  floor

vars == <<known, floor>>

VisiblePast(block) ==
  IF UseCausalClosure THEN DagPast[block] ELSE MainPast[block]

KnownTips(node) ==
  {Tip[validator] : validator \in known[node]}

Discovered(node) ==
  UNION {VisiblePast(tip) : tip \in KnownTips(node)}

Universal(node) ==
  {candidate \in Blocks :
    \A tip \in KnownTips(node) : candidate \in DagPast[tip]}

Eligible(node) ==
  Discovered(node) \cap Universal(node)
    \cap Certified \cap StateCertified

ChosenFloor(node) ==
  IF "F" \in Eligible(node) THEN "F" ELSE "G"

Init ==
  /\ known = [node \in Nodes |-> {}]
  /\ floor = [node \in Nodes |-> "G"]

Deliver(node, validator) ==
  /\ validator \notin known[node]
  /\ known' = [known EXCEPT ![node] = @ \union {validator}]
  /\ UNCHANGED floor

Derive(node) ==
  /\ known[node] # {}
  /\ floor' = [floor EXCEPT ![node] = ChosenFloor(node)]
  /\ UNCHANGED known

Next ==
  \/ \E node \in Nodes, validator \in Validators : Deliver(node, validator)
  \/ \E node \in Nodes : Derive(node)

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A node \in Nodes, validator \in Validators :
       WF_vars(Deliver(node, validator))
  /\ \A node \in Nodes : WF_vars(Derive(node))

TypeOK ==
  /\ known \in [Nodes -> SUBSET Validators]
  /\ floor \in [Nodes -> {"G", "F"}]

Inv_FinalizedStateIsSecondaryToEveryTip ==
  /\ "F" \notin MainPast[Tip["v1"]]
  /\ "F" \notin MainPast[Tip["v2"]]
  /\ "F" \notin MainPast[Tip["v3"]]
  /\ "F" \in DagPast[Tip["v1"]]
  /\ "F" \in DagPast[Tip["v2"]]
  /\ "F" \in DagPast[Tip["v3"]]

Inv_DualCertificateUnchanged ==
  /\ Certified = {"G", "F"}
  /\ StateCertified = {"G", "F"}

Inv_CompleteEvidencePromotesCertifiedFloor ==
  \A node \in Nodes :
    known[node] = Validators => ChosenFloor(node) = "F"

Inv_DerivedFloorHasBothCertificates ==
  \A node \in Nodes :
    floor[node] \in Certified /\ floor[node] \in StateCertified

Live_AllNodesPromoteCertifiedFloor ==
  <>(\A node \in Nodes : floor[node] = "F")
=============================================================================

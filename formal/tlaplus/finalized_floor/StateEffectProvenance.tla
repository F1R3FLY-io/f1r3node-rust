---------------------- MODULE StateEffectProvenance ----------------------
EXTENDS Naturals, FiniteSets, Sequences

CONSTANT
  \* @type: Bool;
  UseUnionParents

ASSUME UseUnionParents \in BOOLEAN

Effects == {"successful:0", "failed-settlement:1", "sibling-b:0", "sibling-c:0"}
SourceEffects == {"successful:0", "failed-settlement:1"}
OmittedApplied == {"sibling-c:0"}
Blocks == {"G", "A", "B", "C", "O1", "O2", "Q1", "Q2", "N1", "N2", "N3", "R"}
BaseBlocks == {"G", "A", "B", "C"}
FirstRound == {"O1", "O2", "Q1", "Q2"}
SecondRound == {"N1", "N2", "N3", "R"}
Nodes == {"n1", "n2"}
Validators == {"v1", "v2", "v3"}

ParentOrder == [block \in Blocks |->
  CASE block = "G" -> <<>>
    [] block \in {"A", "B", "C"} -> <<"G">>
    [] block \in {"O1", "Q1"} -> <<"A", "B", "C">>
    [] block \in {"O2", "Q2"} -> <<"C", "B", "A">>
    [] block = "N1" -> <<"Q1", "O1">>
    [] block = "N2" -> <<"Q2", "O2">>
    [] block = "N3" -> <<"O2", "Q1">>
    [] OTHER -> <<"O1", "A">>]

Parents(block) ==
  {ParentOrder[block][index] : index \in DOMAIN ParentOrder[block]}

StateParent == [block \in Blocks |->
  CASE block = "G" -> "G"
    [] block \in {"A", "B", "C"} -> "G"
    [] block \in {"O1", "O2", "Q1", "Q2"} -> "B"
    [] block = "N1" -> "Q1"
    [] block = "N2" -> "Q2"
    [] block = "N3" -> "Q1"
    [] OTHER -> "O1"]

Own == [block \in Blocks |->
  CASE block = "A" -> SourceEffects
    [] block = "B" -> {"sibling-b:0"}
    [] block = "C" -> {"sibling-c:0"}
    [] OTHER -> {}]

Applied == [block \in Blocks |->
  CASE block \in {"O1", "O2"} -> OmittedApplied
    [] block \in {"Q1", "Q2"} -> OmittedApplied \union SourceEffects
    [] block = "R" -> SourceEffects
    [] OTHER -> {}]

DirectRejected == [block \in Blocks |->
  IF block \in {"O1", "O2"} THEN SourceEffects ELSE {}]

Inputs(block) ==
  IF block = "G"
  THEN {}
  ELSE IF UseUnionParents THEN Parents(block) ELSE {StateParent[block]}

Construct(previous, block) ==
  Own[block] \union Applied[block] \union
    UNION {previous[input] : input \in Inputs(block)}

Active0 == [block \in Blocks |-> Own[block]]

Active1 == [block \in Blocks |->
  IF block \in FirstRound
  THEN Construct(Active0, block)
  ELSE Active0[block]]

Active == [block \in Blocks |->
  IF block \in SecondRound
  THEN Construct(Active1, block)
  ELSE Active1[block]]

DagPast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "A" -> {"G", "A"}
    [] block = "B" -> {"G", "B"}
    [] block = "C" -> {"G", "C"}
    [] block \in FirstRound -> {"G", "A", "B", "C", block}
    [] block = "N1" -> {"G", "A", "B", "C", "O1", "Q1", "N1"}
    [] block = "N2" -> {"G", "A", "B", "C", "O2", "Q2", "N2"}
    [] block = "N3" -> {"G", "A", "B", "C", "O2", "Q1", "N3"}
    [] OTHER -> {"G", "A", "B", "C", "O1", "R"}]

DagDescendant(ancestor, descendant) == ancestor \in DagPast[descendant]
Preserves(ancestor, descendant) == Active[ancestor] \subseteq Active[descendant]

Tip == [validator \in Validators |->
  CASE validator = "v1" -> "N1"
    [] validator = "v2" -> "N2"
    [] OTHER -> "N3"]

VARIABLES
  \* @type: Str -> Str;
  lfb,
  \* @type: Str -> Set(Str);
  known

vars == <<lfb, known>>

StateSupporting(node, candidate) ==
  {validator \in known[node] :
    DagDescendant(candidate, Tip[validator]) /\
    Preserves(candidate, Tip[validator])}

CausalSupporting(node, candidate) ==
  {validator \in known[node] : DagDescendant(candidate, Tip[validator])}

HasMajority(supporting) ==
  /\ supporting \subseteq Validators
  /\ \/ {"v1", "v2"} \subseteq supporting
     \/ {"v1", "v3"} \subseteq supporting
     \/ {"v2", "v3"} \subseteq supporting

StateCertified(node, candidate) ==
  HasMajority(StateSupporting(node, candidate))

ExactStateCertified(node, candidate) ==
  2 * Cardinality(StateSupporting(node, candidate)) > Cardinality(Validators)

Init ==
  /\ lfb = [node \in Nodes |-> "G"]
  /\ known = [node \in Nodes |-> {}]

Deliver(node, validator) ==
  /\ validator \notin known[node]
  /\ known' = [known EXCEPT ![node] = @ \union {validator}]
  /\ UNCHANGED lfb

Promote(node) ==
  /\ lfb[node] = "G"
  /\ StateCertified(node, "A")
  /\ lfb' = [lfb EXCEPT ![node] = "A"]
  /\ UNCHANGED known

Idle == UNCHANGED vars

Next ==
  \/ \E node \in Nodes, validator \in Validators : Deliver(node, validator)
  \/ \E node \in Nodes : Promote(node)
  \/ Idle

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A node \in Nodes, validator \in Validators :
       WF_vars(Deliver(node, validator))
  /\ \A node \in Nodes : WF_vars(Promote(node))

TypeOK ==
  /\ lfb \in [Nodes -> {"G", "A"}]
  /\ known \in [Nodes -> SUBSET Validators]
  /\ Active \in [Blocks -> SUBSET Effects]

Inv_ActiveIsExactPositiveRecurrence ==
  /\ \A block \in FirstRound : Active1[block] = Construct(Active0, block)
  /\ \A block \in SecondRound : Active[block] = Construct(Active1, block)

Inv_ParentOrderInvariant ==
  /\ Parents("O1") = Parents("O2")
  /\ Parents("Q1") = Parents("Q2")
  /\ Active["O1"] = Active["O2"]
  /\ Active["Q1"] = Active["Q2"]

Inv_OmittedParentEffectAbsent ==
  /\ SourceEffects \intersect Active["O1"] = {}
  /\ SourceEffects \intersect Active["O2"] = {}

Inv_AcceptedSiblingEffectPresent ==
  \A block \in {"Q1", "Q2", "N1", "N2", "N3"} :
    SourceEffects \subseteq Active[block]

Inv_DirectRejectionIsEvidenceOnly ==
  /\ DirectRejected["O1"] = SourceEffects
  /\ DirectRejected["O1"] \intersect Applied["O1"] = {}
  /\ DirectRejected["O1"] \intersect Active["O1"] = {}

Inv_RetryRestoresExactEffect == SourceEffects \subseteq Active["R"]

Inv_FailedSettlementSurvivesAcceptedMerges ==
  \A block \in {"Q1", "Q2", "N1", "N2", "N3", "R"} :
    "failed-settlement:1" \in Active[block]

Inv_StateSupportRefinesCausalSupport ==
  \A node \in Nodes :
    StateSupporting(node, "A") \subseteq CausalSupporting(node, "A")

Inv_AllValidatorTipsPreserveSource ==
  /\ DagDescendant("A", Tip["v1"]) /\ Preserves("A", Tip["v1"])
  /\ DagDescendant("A", Tip["v2"]) /\ Preserves("A", Tip["v2"])
  /\ DagDescendant("A", Tip["v3"]) /\ Preserves("A", Tip["v3"])

Inv_MajoritySummaryIsExact ==
  \A node \in Nodes :
    StateCertified(node, "A") = ExactStateCertified(node, "A")

Inv_DeliveredQuorumCertifiesSource ==
  \A node \in Nodes :
    known[node] = Validators => StateCertified(node, "A")

Inv_ConsensusNoDisagreement ==
  known["n1"] = Validators /\ known["n2"] = Validators
    => StateCertified("n1", "A") = StateCertified("n2", "A")

Live_AllNodesPromoteSource == <>(\A node \in Nodes : lfb[node] = "A")
=============================================================================

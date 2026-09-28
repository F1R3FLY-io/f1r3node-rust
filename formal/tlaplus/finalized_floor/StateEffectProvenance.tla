---------------------- MODULE StateEffectProvenance ----------------------
EXTENDS Naturals, FiniteSets, Sequences

CONSTANT
  \* @type: Bool;
  UseSingleBase

ASSUME UseSingleBase \in BOOLEAN

Effects == {"a:0"}
Blocks == {"G", "A", "B", "C", "M1", "M2", "M3", "N1", "N2", "N3", "J", "R"}
BaseBlocks == {"G", "A", "B", "C"}
FirstRound == {"M1", "M2", "M3", "J"}
SecondRound == {"N1", "N2", "N3", "R"}
Nodes == {"n1", "n2"}
Validators == {"v1", "v2", "v3"}

ParentOrder == [block \in Blocks |->
  CASE block = "G" -> <<>>
    [] block \in {"A", "B", "C"} -> <<"G">>
    [] block = "M1" -> <<"A", "B", "C">>
    [] block = "M2" -> <<"B", "C", "A">>
    [] block = "M3" -> <<"C", "A", "B">>
    [] block = "N1" -> <<"M1", "M2", "M3">>
    [] block = "N2" -> <<"M2", "M3", "M1">>
    [] block = "N3" -> <<"M3", "M1", "M2">>
    [] block = "J" -> <<"A", "B", "C">>
    [] OTHER -> <<"J">>]

Parents(block) ==
  {ParentOrder[block][index] : index \in DOMAIN ParentOrder[block]}

Floor == [block \in Blocks |-> IF block = "R" THEN "A" ELSE "G"]

Own == [block \in Blocks |-> IF block = "A" THEN {"a:0"} ELSE {}]

DirectRejected ==
  [block \in Blocks |-> IF block = "J" THEN {"a:0"} ELSE {}]

Inputs(block) ==
  IF UseSingleBase /\ Cardinality(Parents(block)) > 1
  THEN {Floor[block]}
  ELSE Parents(block) \union {Floor[block]}

MergeActive(previous, block) ==
  (Own[block] \union UNION {previous[input] : input \in Inputs(block)})
    \ DirectRejected[block]

Active0 == [block \in Blocks |-> Own[block]]

Active1 == [block \in Blocks |->
  IF block \in FirstRound
  THEN MergeActive(Active0, block)
  ELSE Active0[block]]

Active == [block \in Blocks |->
  IF block \in SecondRound
  THEN MergeActive(Active1, block)
  ELSE Active1[block]]

DagPast == [block \in Blocks |->
  CASE block = "G" -> {"G"}
    [] block = "A" -> {"G", "A"}
    [] block = "B" -> {"G", "B"}
    [] block = "C" -> {"G", "C"}
    [] block \in {"M1", "M2", "M3"} -> {"G", "A", "B", "C", block}
    [] block \in {"N1", "N2", "N3"} ->
         {"G", "A", "B", "C", "M1", "M2", "M3", block}
    [] block = "J" -> {"G", "A", "B", "C", "J"}
    [] OTHER -> {"G", "A", "B", "C", "J", "R"}]

DagDescendant(ancestor, descendant) == ancestor \in DagPast[descendant]
EffectPreserves(ancestor, descendant) == Active[ancestor] \subseteq Active[descendant]

CarriesSourceEffect == [block \in Blocks |->
  CASE block \in {"A", "R"} -> TRUE
    [] block \in {"M1", "M2", "M3", "N1", "N2", "N3"} -> ~UseSingleBase
    [] OTHER -> FALSE]

Preserves(ancestor, descendant) ==
  ~CarriesSourceEffect[ancestor] \/ CarriesSourceEffect[descendant]

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

Inv_ActiveIsExactMergeRecurrence ==
  /\ \A block \in FirstRound : Active1[block] = MergeActive(Active0, block)
  /\ \A block \in SecondRound : Active[block] = MergeActive(Active1, block)

Inv_ParentOrderInvariant ==
  /\ Parents("M1") = Parents("M2")
  /\ Parents("M2") = Parents("M3")
  /\ Active["M1"] = Active["M2"]
  /\ Active["M2"] = Active["M3"]
  /\ Active["N1"] = Active["N2"]
  /\ Active["N2"] = Active["N3"]

Inv_PreservationSummaryEqualsEffectRecurrence ==
  \A ancestor \in Blocks, descendant \in Blocks :
    Preserves(ancestor, descendant) = EffectPreserves(ancestor, descendant)

Inv_AcceptedThreeWayEffectsSurvive ==
  \A block \in {"M1", "M2", "M3", "N1", "N2", "N3"} :
    "a:0" \in Active[block]

Inv_DirectRejectionRemovesOnlyNamedEffect ==
  /\ "a:0" \notin Active["J"]
  /\ DirectRejected["J"] = {"a:0"}

Inv_FinalizedFloorRestoresEffect == "a:0" \in Active["R"]

Inv_StateSupportRefinesCausalSupport ==
  \A node \in Nodes :
    StateSupporting(node, "A") \subseteq CausalSupporting(node, "A")

Inv_EveryDeliveredTipPreservesSource ==
  \A node \in Nodes :
    \A validator \in known[node] :
      DagDescendant("A", Tip[validator]) /\ Preserves("A", Tip[validator])

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

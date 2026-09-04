----------------- MODULE StateEffectProvenanceApalache -----------------
EXTENDS FiniteSets

CONSTANT
  \* @type: Bool;
  UseUnionParents

ASSUME UseUnionParents \in BOOLEAN

Nodes == {"n1", "n2"}
Parents == {"A", "B", "C"}
Effects == {"source:0", "failed-settlement:1", "sibling-b:0", "sibling-c:0"}
SourceEffects == {"source:0", "failed-settlement:1"}
OmittedApplied == {"sibling-c:0"}

ParentEffects ==
  [parent \in Parents |->
    CASE parent = "A" -> SourceEffects
      [] parent = "B" -> {"sibling-b:0"}
      [] OTHER -> {"sibling-c:0"}]

StateParentEffects == ParentEffects["B"]

AppliedEffects(accept_source) ==
  OmittedApplied \union IF accept_source THEN SourceEffects ELSE {}

Construct(received, accept_source) ==
  AppliedEffects(accept_source) \union
    IF UseUnionParents
    THEN UNION {ParentEffects[parent] : parent \in received}
    ELSE StateParentEffects

VARIABLES
  \* @type: Str -> Set(Str);
  inbox,
  \* @type: Str -> Bool;
  merged,
  \* @type: Str -> Bool;
  accepted,
  \* @type: Str -> Set(Str);
  active,
  \* @type: Str -> Str;
  lfb

vars == <<inbox, merged, accepted, active, lfb>>

Init ==
  /\ inbox = [node \in Nodes |-> {}]
  /\ merged = [node \in Nodes |-> FALSE]
  /\ accepted = [node \in Nodes |-> FALSE]
  /\ active = [node \in Nodes |-> {}]
  /\ lfb = [node \in Nodes |-> "G"]

Deliver(node, parent) ==
  /\ ~merged[node]
  /\ parent \notin inbox[node]
  /\ inbox' = [inbox EXCEPT ![node] = @ \union {parent}]
  /\ UNCHANGED <<merged, accepted, active, lfb>>

Settle(node, accept_source) ==
  /\ ~merged[node]
  /\ inbox[node] = Parents
  /\ merged' = [merged EXCEPT ![node] = TRUE]
  /\ accepted' = [accepted EXCEPT ![node] = accept_source]
  /\ active' = [active EXCEPT ![node] = Construct(inbox[node], accept_source)]
  /\ UNCHANGED <<inbox, lfb>>

Promote(node) ==
  /\ merged[node]
  /\ SourceEffects \subseteq active[node]
  /\ lfb[node] = "G"
  /\ lfb' = [lfb EXCEPT ![node] = "A"]
  /\ UNCHANGED <<inbox, merged, accepted, active>>

Idle == UNCHANGED vars

Next ==
  \/ \E node \in Nodes, parent \in Parents : Deliver(node, parent)
  \/ \E node \in Nodes, accept_source \in BOOLEAN : Settle(node, accept_source)
  \/ \E node \in Nodes : Promote(node)
  \/ Idle

TypeOK ==
  /\ inbox \in [Nodes -> SUBSET Parents]
  /\ merged \in [Nodes -> BOOLEAN]
  /\ accepted \in [Nodes -> BOOLEAN]
  /\ active \in [Nodes -> SUBSET Effects]
  /\ lfb \in [Nodes -> {"G", "A"}]

Inv_SettlementIsExact ==
  \A node \in Nodes :
    merged[node] => active[node] = Construct(inbox[node], accepted[node])

Inv_AcceptedMergePreservesSource ==
  \A node \in Nodes :
    merged[node] /\ accepted[node] => SourceEffects \subseteq active[node]

Inv_OmittedParentEffectAbsent ==
  \A node \in Nodes :
    merged[node] /\ ~accepted[node] => SourceEffects \intersect active[node] = {}

Inv_PromotionRequiresAcceptedEffect ==
  \A node \in Nodes :
    lfb[node] = "A" => merged[node] /\ accepted[node]

Inv_FailedSettlementPreserved ==
  \A node \in Nodes :
    merged[node] /\ accepted[node] =>
      "failed-settlement:1" \in active[node]

Inv_SettledNodesConverge ==
  merged["n1"] /\ merged["n2"] /\ accepted["n1"] = accepted["n2"]
    => active["n1"] = active["n2"]

=============================================================================

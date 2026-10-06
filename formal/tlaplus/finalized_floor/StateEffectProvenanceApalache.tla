----------------- MODULE StateEffectProvenanceApalache -----------------
EXTENDS FiniteSets

CONSTANT
  \* @type: Bool;
  UseSingleBase

ASSUME UseSingleBase \in BOOLEAN

Nodes == {"n1", "n2"}
Parents == {"A", "B", "C"}
Effects == {"source:0", "sibling-b:0", "sibling-c:0"}
SourceEffect == "source:0"

ParentEffects ==
  [parent \in Parents |->
    CASE parent = "A" -> {SourceEffect}
      [] parent = "B" -> {"sibling-b:0"}
      [] OTHER -> {"sibling-c:0"}]

SelectedInputs(received) == IF UseSingleBase THEN {"B"} ELSE received

SettledEffects(received) ==
  UNION {ParentEffects[parent] : parent \in SelectedInputs(received)}

VARIABLES
  \* @type: Str -> Set(Str);
  inbox,
  \* @type: Str -> Bool;
  merged,
  \* @type: Str -> Set(Str);
  active,
  \* @type: Str -> Str;
  lfb

vars == <<inbox, merged, active, lfb>>

Init ==
  /\ inbox = [node \in Nodes |-> {}]
  /\ merged = [node \in Nodes |-> FALSE]
  /\ active = [node \in Nodes |-> {}]
  /\ lfb = [node \in Nodes |-> "G"]

Deliver(node, parent) ==
  /\ ~merged[node]
  /\ parent \notin inbox[node]
  /\ inbox' = [inbox EXCEPT ![node] = @ \union {parent}]
  /\ UNCHANGED <<merged, active, lfb>>

Settle(node) ==
  /\ ~merged[node]
  /\ inbox[node] = Parents
  /\ merged' = [merged EXCEPT ![node] = TRUE]
  /\ active' = [active EXCEPT ![node] = SettledEffects(inbox[node])]
  /\ UNCHANGED <<inbox, lfb>>

Promote(node) ==
  /\ merged[node]
  /\ SourceEffect \in active[node]
  /\ lfb[node] = "G"
  /\ lfb' = [lfb EXCEPT ![node] = "A"]
  /\ UNCHANGED <<inbox, merged, active>>

Idle == UNCHANGED vars

Next ==
  \/ \E node \in Nodes, parent \in Parents : Deliver(node, parent)
  \/ \E node \in Nodes : Settle(node)
  \/ \E node \in Nodes : Promote(node)
  \/ Idle

TypeOK ==
  /\ inbox \in [Nodes -> SUBSET Parents]
  /\ merged \in [Nodes -> BOOLEAN]
  /\ active \in [Nodes -> SUBSET Effects]
  /\ lfb \in [Nodes -> {"G", "A"}]

Inv_SettlementIsExact ==
  \A node \in Nodes :
    merged[node] => active[node] = SettledEffects(inbox[node])

Inv_AcceptedMergePreservesSource ==
  \A node \in Nodes : merged[node] => SourceEffect \in active[node]

Inv_PromotionRequiresPreservedSource ==
  \A node \in Nodes :
    lfb[node] = "A" => merged[node] /\ SourceEffect \in active[node]

Inv_SettledNodesConverge ==
  merged["n1"] /\ merged["n2"] => active["n1"] = active["n2"]

=============================================================================

------------------- MODULE StatePreservingForkChoice -------------------
EXTENDS Naturals, FiniteSets

CONSTANT
  \* @type: Bool;
  ProtectFloor,
  \* @type: Set(Str);
  ModelNodes

ASSUME ProtectFloor \in BOOLEAN
ASSUME /\ ModelNodes \subseteq {"n1", "n2"}
       /\ ModelNodes # {}

Nodes == ModelNodes
Validators == {"v1", "v2", "v3"}
Blocks == {"G", "F", "P1", "P2", "D"}
Effects == {"funding", "stale"}

Active == [block \in Blocks |->
  CASE block = "F" -> {"funding"}
    [] block = "D" -> {"stale"}
    [] OTHER -> {}]

Tip == [validator \in Validators |->
  CASE validator = "v1" -> "P1"
    [] validator = "v2" -> "P2"
    [] OTHER -> "D"]

VARIABLES
  \* @type: Str -> Str;
  lfb,
  \* @type: Str -> Set(Str);
  certificateKnown,
  \* @type: Str -> Set(Str);
  latestKnown,
  \* @type: Str -> (Str -> Str);
  latest,
  \* @type: Str -> Bool;
  hasProposal,
  \* @type: Str -> Str;
  proposalFloor,
  \* @type: Str -> Set(Str);
  proposalParents,
  \* @type: Str -> Set(Str);
  proposalValidTips,
  \* @type: Str -> Set(Str);
  proposalCausalInputs,
  \* @type: Str -> Set(Str);
  proposalState,
  \* @type: Str -> Bool;
  proposedAtFinalizedFloor

vars == <<lfb, certificateKnown, latestKnown, latest, hasProposal,
  proposalFloor, proposalParents, proposalValidTips, proposalCausalInputs,
  proposalState,
  proposedAtFinalizedFloor>>

CandidateTips(node) ==
  {latest[node][validator] : validator \in latestKnown[node]}

Covers(parent, tip) ==
  parent = tip \/ (parent = "P1" /\ tip = "P2")

DirectParents(node) ==
  {tip \in CandidateTips(node) :
    ~\E parent \in CandidateTips(node) :
      parent # tip /\ Covers(parent, tip)}

ChosenParents(node) ==
  IF CandidateTips(node) = {}
  THEN {lfb[node]}
  ELSE DirectParents(node)

ChosenCausalInputs(node) ==
  IF CandidateTips(node) = {}
  THEN {lfb[node]}
  ELSE CandidateTips(node)

ParentState(node) ==
  UNION {Active[parent] : parent \in ChosenCausalInputs(node)}

RebasedState(node) ==
  ParentState(node) \union (IF ProtectFloor THEN Active[lfb[node]] ELSE {})

ProposalSnapshotChanged(node) ==
  \/ ~hasProposal[node]
  \/ proposalFloor[node] # lfb[node]
  \/ proposalParents[node] # ChosenParents(node)
  \/ proposalValidTips[node] # CandidateTips(node)
  \/ proposalCausalInputs[node] # ChosenCausalInputs(node)
  \/ proposalState[node] # RebasedState(node)
  \/ ~proposedAtFinalizedFloor[node] /\ lfb[node] = "F"

HasCertificate(node) ==
  {"v1", "v2"} \subseteq certificateKnown[node]

Init ==
  /\ lfb = [node \in Nodes |-> "G"]
  /\ certificateKnown = [node \in Nodes |-> {}]
  /\ latestKnown = [node \in Nodes |-> {}]
  /\ latest = [node \in Nodes |-> [validator \in Validators |-> "G"]]
  /\ hasProposal = [node \in Nodes |-> FALSE]
  /\ proposalFloor = [node \in Nodes |-> "G"]
  /\ proposalParents = [node \in Nodes |-> {}]
  /\ proposalValidTips = [node \in Nodes |-> {}]
  /\ proposalCausalInputs = [node \in Nodes |-> {}]
  /\ proposalState = [node \in Nodes |-> {}]
  /\ proposedAtFinalizedFloor = [node \in Nodes |-> FALSE]

DeliverCertificate(node, validator) ==
  /\ validator \notin certificateKnown[node]
  /\ certificateKnown' =
       [certificateKnown EXCEPT ![node] = @ \union {validator}]
  /\ UNCHANGED <<lfb, latestKnown, latest, hasProposal, proposalFloor,
       proposalParents, proposalValidTips, proposalCausalInputs, proposalState,
       proposedAtFinalizedFloor>>

DeliverLatest(node, validator) ==
  /\ validator \notin latestKnown[node]
  /\ latestKnown' = [latestKnown EXCEPT ![node] = @ \union {validator}]
  /\ latest' = [latest EXCEPT ![node][validator] = Tip[validator]]
  /\ UNCHANGED <<lfb, certificateKnown, hasProposal, proposalFloor,
       proposalParents, proposalValidTips, proposalCausalInputs, proposalState,
       proposedAtFinalizedFloor>>

Promote(node) ==
  /\ lfb[node] = "G"
  /\ HasCertificate(node)
  /\ lfb' = [lfb EXCEPT ![node] = "F"]
  /\ UNCHANGED <<certificateKnown, latestKnown, latest, hasProposal,
       proposalFloor, proposalParents, proposalValidTips, proposalCausalInputs,
       proposalState,
       proposedAtFinalizedFloor>>

Propose(node) ==
  /\ ProposalSnapshotChanged(node)
  /\ hasProposal' = [hasProposal EXCEPT ![node] = TRUE]
  /\ proposalFloor' = [proposalFloor EXCEPT ![node] = lfb[node]]
  /\ proposalParents' = [proposalParents EXCEPT ![node] = ChosenParents(node)]
  /\ proposalValidTips' = [proposalValidTips EXCEPT ![node] = CandidateTips(node)]
  /\ proposalCausalInputs' =
       [proposalCausalInputs EXCEPT ![node] = ChosenCausalInputs(node)]
  /\ proposalState' = [proposalState EXCEPT ![node] = RebasedState(node)]
  /\ proposedAtFinalizedFloor' =
       [proposedAtFinalizedFloor EXCEPT ![node] = @ \/ lfb[node] = "F"]
  /\ UNCHANGED <<lfb, certificateKnown, latestKnown, latest>>

Next ==
  \/ \E node \in Nodes, validator \in Validators :
       DeliverCertificate(node, validator)
  \/ \E node \in Nodes, validator \in Validators :
       DeliverLatest(node, validator)
  \/ \E node \in Nodes : Promote(node)
  \/ \E node \in Nodes : Propose(node)

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A node \in Nodes, validator \in Validators :
       WF_vars(DeliverCertificate(node, validator))
  /\ \A node \in Nodes, validator \in Validators :
       WF_vars(DeliverLatest(node, validator))
  /\ \A node \in Nodes : WF_vars(Promote(node))
  /\ \A node \in Nodes : WF_vars(Propose(node) /\ lfb[node] = "F")

TypeOK ==
  /\ lfb \in [Nodes -> {"G", "F"}]
  /\ certificateKnown \in [Nodes -> SUBSET Validators]
  /\ latestKnown \in [Nodes -> SUBSET Validators]
  /\ latest \in [Nodes -> [Validators -> Blocks]]
  /\ hasProposal \in [Nodes -> BOOLEAN]
  /\ proposalFloor \in [Nodes -> {"G", "F"}]
  /\ proposalParents \in [Nodes -> SUBSET Blocks]
  /\ proposalValidTips \in [Nodes -> SUBSET Blocks]
  /\ proposalCausalInputs \in [Nodes -> SUBSET Blocks]
  /\ proposalState \in [Nodes -> SUBSET Effects]
  /\ proposedAtFinalizedFloor \in [Nodes -> BOOLEAN]

Inv_ProposalHasParent ==
  \A node \in Nodes : hasProposal[node] => proposalParents[node] # {}

Inv_FallbackUsesSnapshotFloor ==
  \A node \in Nodes :
    hasProposal[node] /\ proposalValidTips[node] = {} =>
      proposalParents[node] = {proposalFloor[node]}

Inv_AllValidLatestTipsCoveredByParents ==
  \A node \in Nodes :
    hasProposal[node] /\ proposalValidTips[node] # {} =>
      \A tip \in proposalValidTips[node] :
        \E parent \in proposalParents[node] : Covers(parent, tip)

Inv_AllValidLatestTipsRemainCausalInputs ==
  \A node \in Nodes :
    hasProposal[node] /\ proposalValidTips[node] # {} =>
      proposalCausalInputs[node] = proposalValidTips[node]

Inv_ProposalStateIsFloorRebased ==
  \A node \in Nodes :
    hasProposal[node] =>
      proposalState[node] =
        UNION {Active[input] : input \in proposalCausalInputs[node]}
          \union (IF ProtectFloor THEN Active[proposalFloor[node]] ELSE {})

Inv_ProposalPreservesSnapshotFloor ==
  \A node \in Nodes :
    hasProposal[node] => Active[proposalFloor[node]] \subseteq proposalState[node]

Inv_FinalizedFundingCannotBeDropped ==
  \A node \in Nodes :
    hasProposal[node] /\ proposalFloor[node] = "F" =>
      "funding" \in proposalState[node]

Live_AllNodesPromote == <>(\A node \in Nodes : lfb[node] = "F")

Live_AllNodesProposeAtFinalizedFloor ==
  <>(\A node \in Nodes : proposedAtFinalizedFloor[node])
=============================================================================

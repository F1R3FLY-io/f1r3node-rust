------------------- MODULE StatePreservingForkChoice -------------------
EXTENDS Naturals, FiniteSets

CONSTANT
  \* @type: Bool;
  ProtectFloor,
  \* @type: Bool;
  ParentUsesVoteProjection,
  \* @type: Bool;
  AdmitInvalidStale,
  \* @type: Bool;
  StaleTipIsValid,
  \* @type: Bool;
  PromoteDeployParent,
  \* @type: Bool;
  OmitFloorEvidenceRoot,
  \* @type: Bool;
  SkipAntichainCompaction,
  \* @type: Bool;
  RecoveryIgnoresFloorAncestry,
  \* @type: Bool;
  ApplyCausalExpiry,
  \* @type: Int;
  ParentCap,
  \* @type: Int;
  MaxParentDepth,
  \* @type: Set(Str);
  ModelNodes

ASSUME ProtectFloor \in BOOLEAN
ASSUME ParentUsesVoteProjection \in BOOLEAN
ASSUME AdmitInvalidStale \in BOOLEAN
ASSUME StaleTipIsValid \in BOOLEAN
ASSUME PromoteDeployParent \in BOOLEAN
ASSUME OmitFloorEvidenceRoot \in BOOLEAN
ASSUME SkipAntichainCompaction \in BOOLEAN
ASSUME RecoveryIgnoresFloorAncestry \in BOOLEAN
ASSUME ApplyCausalExpiry \in BOOLEAN
ASSUME ParentCap \in Nat \ {0}
ASSUME MaxParentDepth \in Nat
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

Height == [block \in Blocks |->
  CASE block = "G" -> 0
    [] block \in {"F", "D"} -> 1
    [] OTHER -> 2]

Tip == [validator \in Validators |->
  CASE validator = "v1" -> "P1"
    [] validator = "v2" -> "P2"
    [] OTHER -> "D"]

BaseAdmissible(block) ==
  block # "D" \/ StaleTipIsValid \/ AdmitInvalidStale

Descends(floor, block) ==
  CASE floor = "G" -> TRUE
    [] floor = "F" -> block \in {"F", "P1", "P2"}
    [] OTHER -> FALSE

Covers(parent, tip) ==
  parent = tip \/ (parent = "P1" /\ tip = "P2")

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
  recoveryBacklog,
  \* @type: Str -> Bool;
  hasProposal,
  \* @type: Str -> Str;
  proposalFloor,
  \* @type: Str -> Set(Str);
  proposalParents,
  \* @type: Str -> Str;
  proposalMain,
  \* @type: Str -> Set(Str);
  proposalExactTips,
  \* @type: Str -> Set(Str);
  proposalValidTips,
  \* @type: Str -> Set(Str);
  proposalVoteTips,
  \* @type: Str -> Set(Str);
  proposalCausalInputs,
  \* @type: Str -> Set(Str);
  proposalEvidenceRoots,
  \* @type: Str -> Set(Str);
  proposalState,
  \* @type: Str -> Bool;
  proposalRecoveryNarrowed,
  \* @type: Str -> Bool;
  proposedAtFinalizedFloor

vars == <<lfb, certificateKnown, latestKnown, latest, recoveryBacklog,
  hasProposal, proposalFloor, proposalParents, proposalMain,
  proposalExactTips, proposalValidTips, proposalVoteTips, proposalCausalInputs,
  proposalEvidenceRoots, proposalState, proposalRecoveryNarrowed,
  proposedAtFinalizedFloor>>

ExactTips(node) ==
  {latest[node][validator] : validator \in latestKnown[node]}

CandidateTips(node) ==
  {latest[node][validator] :
    validator \in
      {candidate \in latestKnown[node] :
        BaseAdmissible(latest[node][candidate])}}

VoteTips(node) ==
  {tip \in CandidateTips(node) : Descends(lfb[node], tip)}

ProjectionTips(node) ==
  IF ParentUsesVoteProjection THEN VoteTips(node) ELSE CandidateTips(node)

NeedsFloorBackstop(node) ==
  ~\E tip \in ProjectionTips(node) : Descends(lfb[node], tip)

ParentCandidates(node) ==
  ProjectionTips(node)
    \union (IF NeedsFloorBackstop(node) THEN {lfb[node]} ELSE {})

ReachabilityMaximalParents(node) ==
  {tip \in ParentCandidates(node) :
    ~\E parent \in ParentCandidates(node) :
      parent # tip /\ Covers(parent, tip)}

CompactedParents(node) ==
  IF SkipAntichainCompaction
  THEN ParentCandidates(node)
  ELSE ReachabilityMaximalParents(node)

GhostHead(node) ==
  IF VoteTips(node) = {}
  THEN lfb[node]
  ELSE IF "P1" \in VoteTips(node)
       THEN "P1"
       ELSE IF "P2" \in VoteTips(node)
            THEN "P2"
            ELSE "D"

SelectedMain(node) ==
  IF PromoteDeployParent /\ "D" \in CompactedParents(node)
  THEN "D"
  ELSE GhostHead(node)

HighestCompactedHeight(node) ==
  IF "P1" \in CompactedParents(node) \/ "P2" \in CompactedParents(node)
  THEN 2
  ELSE IF "F" \in CompactedParents(node) \/ "D" \in CompactedParents(node)
       THEN 1
       ELSE 0

DepthRetainedParents(node) ==
  IF ApplyCausalExpiry
  THEN {parent \in CompactedParents(node) :
    parent = SelectedMain(node)
      \/ HighestCompactedHeight(node) - Height[parent] <= MaxParentDepth}
  ELSE CompactedParents(node)

LiveCausalTips(node) ==
  {tip \in ProjectionTips(node) :
    \E parent \in DepthRetainedParents(node) : Covers(parent, tip)}

MainCoversAllCausalTips(node) ==
  \A tip \in LiveCausalTips(node) : Covers(SelectedMain(node), tip)

MainDescendsFromFloor(node) ==
  Descends(lfb[node], SelectedMain(node))

ShouldNarrowRecovery(node) ==
  /\ recoveryBacklog[node]
  /\ Cardinality(DepthRetainedParents(node)) > 1
  /\ MainCoversAllCausalTips(node)
  /\ (RecoveryIgnoresFloorAncestry \/ MainDescendsFromFloor(node))

ChosenParents(node) ==
  IF ShouldNarrowRecovery(node)
  THEN {SelectedMain(node)}
  ELSE DepthRetainedParents(node)

SemanticCausalInputs(node) ==
  LiveCausalTips(node) \union (IF ProtectFloor THEN {lfb[node]} ELSE {})

EvidenceRoots(node) ==
  ExactTips(node) \union (IF OmitFloorEvidenceRoot THEN {} ELSE {lfb[node]})

HighestParentHeight(node) ==
  IF "P1" \in ChosenParents(node) \/ "P2" \in ChosenParents(node)
  THEN 2
  ELSE IF "F" \in ChosenParents(node) \/ "D" \in ChosenParents(node)
       THEN 1
       ELSE 0

WithinDepth(node, parent) ==
  parent = SelectedMain(node)
    \/ HighestParentHeight(node) - Height[parent] <= MaxParentDepth

BoundsAdmit(node) ==
  /\ Cardinality(ChosenParents(node)) <= ParentCap
  /\ (ApplyCausalExpiry
       \/ \A parent \in ChosenParents(node) : WithinDepth(node, parent))

ParentSelectionReady(node) ==
  /\ ChosenParents(node) # {}
  /\ SelectedMain(node) \in ChosenParents(node)
  /\ BoundsAdmit(node)

ParentState(node) ==
  UNION {Active[input] : input \in SemanticCausalInputs(node)}

RebasedState(node) == ParentState(node)

ProposalSnapshotChanged(node) ==
  \/ ~hasProposal[node]
  \/ proposalFloor[node] # lfb[node]
  \/ proposalParents[node] # ChosenParents(node)
  \/ proposalMain[node] # SelectedMain(node)
  \/ proposalExactTips[node] # ExactTips(node)
  \/ proposalValidTips[node] # CandidateTips(node)
  \/ proposalVoteTips[node] # VoteTips(node)
  \/ proposalCausalInputs[node] # SemanticCausalInputs(node)
  \/ proposalEvidenceRoots[node] # EvidenceRoots(node)
  \/ proposalState[node] # RebasedState(node)
  \/ proposalRecoveryNarrowed[node] # ShouldNarrowRecovery(node)
  \/ ~proposedAtFinalizedFloor[node] /\ lfb[node] = "F"

HasCertificate(node) == {"v1", "v2"} \subseteq certificateKnown[node]

Init ==
  /\ lfb = [node \in Nodes |-> "G"]
  /\ certificateKnown = [node \in Nodes |-> {}]
  /\ latestKnown = [node \in Nodes |-> {}]
  /\ latest = [node \in Nodes |-> [validator \in Validators |-> "G"]]
  /\ recoveryBacklog = [node \in Nodes |-> FALSE]
  /\ hasProposal = [node \in Nodes |-> FALSE]
  /\ proposalFloor = [node \in Nodes |-> "G"]
  /\ proposalParents = [node \in Nodes |-> {}]
  /\ proposalMain = [node \in Nodes |-> "G"]
  /\ proposalExactTips = [node \in Nodes |-> {}]
  /\ proposalValidTips = [node \in Nodes |-> {}]
  /\ proposalVoteTips = [node \in Nodes |-> {}]
  /\ proposalCausalInputs = [node \in Nodes |-> {}]
  /\ proposalEvidenceRoots = [node \in Nodes |-> {}]
  /\ proposalState = [node \in Nodes |-> {}]
  /\ proposalRecoveryNarrowed = [node \in Nodes |-> FALSE]
  /\ proposedAtFinalizedFloor = [node \in Nodes |-> FALSE]

ApalacheInit ==
  /\ lfb = [node \in Nodes |-> "F"]
  /\ certificateKnown = [node \in Nodes |-> {"v1", "v2"}]
  /\ latestKnown = [node \in Nodes |-> {}]
  /\ latest = [node \in Nodes |-> [validator \in Validators |-> "G"]]
  /\ recoveryBacklog = [node \in Nodes |-> FALSE]
  /\ hasProposal = [node \in Nodes |-> FALSE]
  /\ proposalFloor = [node \in Nodes |-> "F"]
  /\ proposalParents = [node \in Nodes |-> {}]
  /\ proposalMain = [node \in Nodes |-> "F"]
  /\ proposalExactTips = [node \in Nodes |-> {}]
  /\ proposalValidTips = [node \in Nodes |-> {}]
  /\ proposalVoteTips = [node \in Nodes |-> {}]
  /\ proposalCausalInputs = [node \in Nodes |-> {}]
  /\ proposalEvidenceRoots = [node \in Nodes |-> {}]
  /\ proposalState = [node \in Nodes |-> {}]
  /\ proposalRecoveryNarrowed = [node \in Nodes |-> FALSE]
  /\ proposedAtFinalizedFloor = [node \in Nodes |-> FALSE]

DeliverCertificate(node, validator) ==
  /\ validator \notin certificateKnown[node]
  /\ certificateKnown' =
       [certificateKnown EXCEPT ![node] = @ \union {validator}]
  /\ UNCHANGED <<lfb, latestKnown, latest, recoveryBacklog, hasProposal,
       proposalFloor, proposalParents, proposalMain, proposalExactTips, proposalValidTips,
       proposalVoteTips, proposalCausalInputs, proposalEvidenceRoots,
       proposalState, proposalRecoveryNarrowed, proposedAtFinalizedFloor>>

DeliverLatest(node, validator) ==
  /\ validator \notin latestKnown[node]
  /\ latestKnown' = [latestKnown EXCEPT ![node] = @ \union {validator}]
  /\ latest' = [latest EXCEPT ![node][validator] = Tip[validator]]
  /\ UNCHANGED <<lfb, certificateKnown, recoveryBacklog, hasProposal,
       proposalFloor, proposalParents, proposalMain, proposalExactTips, proposalValidTips,
       proposalVoteTips, proposalCausalInputs, proposalEvidenceRoots,
       proposalState, proposalRecoveryNarrowed, proposedAtFinalizedFloor>>

ObserveRecoveryBacklog(node) ==
  /\ ~recoveryBacklog[node]
  /\ recoveryBacklog' = [recoveryBacklog EXCEPT ![node] = TRUE]
  /\ UNCHANGED <<lfb, certificateKnown, latestKnown, latest, hasProposal,
       proposalFloor, proposalParents, proposalMain, proposalExactTips, proposalValidTips,
       proposalVoteTips, proposalCausalInputs, proposalEvidenceRoots,
       proposalState, proposalRecoveryNarrowed, proposedAtFinalizedFloor>>

Promote(node) ==
  /\ lfb[node] = "G"
  /\ HasCertificate(node)
  /\ lfb' = [lfb EXCEPT ![node] = "F"]
  /\ UNCHANGED <<certificateKnown, latestKnown, latest, recoveryBacklog,
       hasProposal, proposalFloor, proposalParents, proposalMain,
       proposalExactTips, proposalValidTips, proposalVoteTips, proposalCausalInputs,
       proposalEvidenceRoots, proposalState, proposalRecoveryNarrowed,
       proposedAtFinalizedFloor>>

Propose(node) ==
  /\ ProposalSnapshotChanged(node)
  /\ ParentSelectionReady(node)
  /\ hasProposal' = [hasProposal EXCEPT ![node] = TRUE]
  /\ proposalFloor' = [proposalFloor EXCEPT ![node] = lfb[node]]
  /\ proposalParents' = [proposalParents EXCEPT ![node] = ChosenParents(node)]
  /\ proposalMain' = [proposalMain EXCEPT ![node] = SelectedMain(node)]
  /\ proposalExactTips' = [proposalExactTips EXCEPT ![node] = ExactTips(node)]
  /\ proposalValidTips' = [proposalValidTips EXCEPT ![node] = CandidateTips(node)]
  /\ proposalVoteTips' = [proposalVoteTips EXCEPT ![node] = VoteTips(node)]
  /\ proposalCausalInputs' =
       [proposalCausalInputs EXCEPT ![node] = SemanticCausalInputs(node)]
  /\ proposalEvidenceRoots' =
       [proposalEvidenceRoots EXCEPT ![node] = EvidenceRoots(node)]
  /\ proposalState' = [proposalState EXCEPT ![node] = RebasedState(node)]
  /\ proposalRecoveryNarrowed' =
       [proposalRecoveryNarrowed EXCEPT ![node] = ShouldNarrowRecovery(node)]
  /\ proposedAtFinalizedFloor' =
       [proposedAtFinalizedFloor EXCEPT ![node] = @ \/ lfb[node] = "F"]
  /\ UNCHANGED <<lfb, certificateKnown, latestKnown, latest, recoveryBacklog>>

Next ==
  \/ \E node \in Nodes, validator \in Validators :
       DeliverCertificate(node, validator)
  \/ \E node \in Nodes, validator \in Validators :
       DeliverLatest(node, validator)
  \/ \E node \in Nodes : ObserveRecoveryBacklog(node)
  \/ \E node \in Nodes : Promote(node)
  \/ \E node \in Nodes : Propose(node)

ApalacheNext ==
  \/ \E node \in Nodes, validator \in Validators :
       DeliverLatest(node, validator)
  \/ \E node \in Nodes : ObserveRecoveryBacklog(node)
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
  /\ recoveryBacklog \in [Nodes -> BOOLEAN]
  /\ hasProposal \in [Nodes -> BOOLEAN]
  /\ proposalFloor \in [Nodes -> {"G", "F"}]
  /\ proposalParents \in [Nodes -> SUBSET Blocks]
  /\ proposalMain \in [Nodes -> Blocks]
  /\ proposalExactTips \in [Nodes -> SUBSET Blocks]
  /\ proposalValidTips \in [Nodes -> SUBSET Blocks]
  /\ proposalVoteTips \in [Nodes -> SUBSET Blocks]
  /\ proposalCausalInputs \in [Nodes -> SUBSET Blocks]
  /\ proposalEvidenceRoots \in [Nodes -> SUBSET Blocks]
  /\ proposalState \in [Nodes -> SUBSET Effects]
  /\ proposalRecoveryNarrowed \in [Nodes -> BOOLEAN]
  /\ proposedAtFinalizedFloor \in [Nodes -> BOOLEAN]

Inv_ProposalHasParent ==
  \A node \in Nodes : hasProposal[node] => proposalParents[node] # {}

Inv_FloorBackstopUsesSnapshotFloor ==
  \A node \in Nodes :
    hasProposal[node] =>
      \E parent \in proposalParents[node] : Descends(proposalFloor[node], parent)

Inv_AllValidLatestTipsCoveredByParents ==
  \A node \in Nodes :
    hasProposal[node] =>
      \A tip \in proposalCausalInputs[node] \ {proposalFloor[node]} :
        \E parent \in proposalParents[node] : Covers(parent, tip)

Inv_ParentsFormReachabilityAntichain ==
  \A node \in Nodes :
    hasProposal[node] =>
      \A left \in proposalParents[node], right \in proposalParents[node] :
        left # right => ~Covers(left, right)

Inv_AllValidLatestTipsRemainCausalInputs ==
  \A node \in Nodes :
    hasProposal[node] => proposalValidTips[node] \subseteq proposalCausalInputs[node]

Inv_VoteTipsAreCausalTips ==
  \A node \in Nodes :
    hasProposal[node] => proposalVoteTips[node] \subseteq proposalValidTips[node]

Inv_StaleAcceptedTipRemainsCausalButCannotVote ==
  \A node \in Nodes :
    hasProposal[node] /\ proposalFloor[node] = "F" /\
      "D" \in proposalValidTips[node] =>
      /\ "D" \in proposalCausalInputs[node]
      /\ "D" \notin proposalVoteTips[node]

Inv_IntrinsicallyInvalidTipIsNeverCausal ==
  \A node \in Nodes :
    hasProposal[node] /\ ~StaleTipIsValid =>
      "D" \notin proposalValidTips[node]

Inv_GhostHeadIsMainParent ==
  \A node \in Nodes :
    hasProposal[node] =>
      proposalMain[node] =
        IF proposalVoteTips[node] = {}
        THEN proposalFloor[node]
        ELSE IF "P1" \in proposalVoteTips[node]
             THEN "P1"
             ELSE IF "P2" \in proposalVoteTips[node]
                  THEN "P2"
                  ELSE "D"

Inv_MainParentIsDeclared ==
  \A node \in Nodes :
    hasProposal[node] => proposalMain[node] \in proposalParents[node]

Inv_EvidenceRootsIncludeSnapshotFloor ==
  \A node \in Nodes :
    hasProposal[node] => proposalFloor[node] \in proposalEvidenceRoots[node]

Inv_ExactLatestMessagesAreEvidenceRoots ==
  \A node \in Nodes :
    hasProposal[node] => proposalExactTips[node] \subseteq proposalEvidenceRoots[node]

Inv_ProposalStateIsFloorRebased ==
  \A node \in Nodes :
    hasProposal[node] =>
      proposalState[node] =
        UNION {Active[input] : input \in proposalCausalInputs[node]}

Inv_ProposalPreservesSnapshotFloor ==
  \A node \in Nodes :
    hasProposal[node] => Active[proposalFloor[node]] \subseteq proposalState[node]

Inv_FinalizedFundingCannotBeDropped ==
  \A node \in Nodes :
    hasProposal[node] /\ proposalFloor[node] = "F" =>
      "funding" \in proposalState[node]

Inv_RecoveryNarrowingRequiresCoverageAndFloorAncestry ==
  \A node \in Nodes :
    hasProposal[node] /\ proposalRecoveryNarrowed[node] =>
      /\ Descends(proposalFloor[node], proposalMain[node])
      /\ \A tip \in proposalCausalInputs[node] \ {proposalFloor[node]} :
           Covers(proposalMain[node], tip)

Live_AllNodesPromote == <>(\A node \in Nodes : lfb[node] = "F")

Live_AllNodesProposeAtFinalizedFloor ==
  <>(\A node \in Nodes : proposedAtFinalizedFloor[node])
=============================================================================

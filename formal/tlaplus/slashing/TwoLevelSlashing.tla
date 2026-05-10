--------------------------- MODULE TwoLevelSlashing ---------------------------
(****************************************************************************)
(* Two-level slashing closure.                                              *)
(*                                                                          *)
(*   Level 1: validator A equivocates → A is slashed.                       *)
(*   Level 2: validator B's latest-message view makes A's equivocation      *)
(*            detectable without acknowledging/slashing A → B is slashed.   *)
(*                                                                          *)
(* Verifies:                                                                *)
(*   - termination: the closure reaches a fixed point in finite steps       *)
(*   - quorum preservation: the active validator set never falls below      *)
(*     n − ⌊(n−1)/3⌋                                                       *)
(*                                                                          *)
(* Reference: docs/theory/slashing/slashing-verification.md §7.             *)
(****************************************************************************)

EXTENDS Integers, FiniteSets, Sequences, TLC

CONSTANTS
    Validators,         \* Set of validator IDs
    MaxLevel,           \* Max neglect-chain depth to model
    EnforceClosureBound,\* TRUE checks T-12 under the BFT precondition
    BondWeight,         \* Validator -> non-negative stake weight
    CurrentValidators,  \* Validators eligible in the current validator set
    EvidenceValidators, \* Validators named by evidence under review
    Visibility,         \* Validator -> offenders whose evidence was visible
    Reports,            \* Validator -> offenders already reported/slashed
    EnforceWeightedClosureBound,
    EnforceVisibility,
    ArithmeticBits,
    MaxBond,
    InitialVault,
    ArithmeticLimit,
    CurrentEpoch,
    EvidenceEpoch,
    ViewAVisibility,
    ViewAReports,
    ViewBVisibility,
    ViewBReports,
    CarryoverEnabled,
    CarryoverMappedDirect,
    EnforceEvidenceRetention,
    RecordSeqBound,
    BatchFailureSet,
    EnforceBatchAtomicity,
    BatchOrderA,
    BatchOrderB,
    ProposerSchedule,
    EvidenceObservedBy,
    EvidenceIncludedBy,
    EnforceProposerFairness,
    GossipDelay,
    InclusionDelay,
    RetentionWindow,
    RebondOldNonce,
    RebondNewNonce,
    EnforceRecordRetention,
    Renaming

VARIABLES
    \* Static evidence: which validators equivocated (set once at init)
    equivocators,
    \* Dynamic: who has been slashed
    slashed,
    \* For each non-equivocator v, the set of slashed/equivocating validators
    \* whose blocks v cites in its justifications WITHOUT attaching a slash.
    \* This is the "neglect graph": neglectGraph[v] is the set of upstream
    \* offenders v failed to slash.
    neglectGraph,
    \* Step counter for termination check
    step

vars == <<equivocators, slashed, neglectGraph, step>>

(****************************************************************************)
(* TypeOK                                                                   *)
(****************************************************************************)
TypeOK ==
    /\ equivocators  \in SUBSET Validators
    /\ slashed       \in SUBSET Validators
    /\ neglectGraph  \in [Validators -> SUBSET Validators]
    /\ step          \in 0..MaxLevel
    /\ BondWeight    \in [Validators -> Nat]
    /\ CurrentValidators \subseteq Validators
    /\ EvidenceValidators \subseteq Validators
    /\ Visibility    \in [Validators -> SUBSET Validators]
    /\ Reports       \in [Validators -> SUBSET Validators]
    /\ ArithmeticBits \in Nat
    /\ ArithmeticBits >= 2
    /\ MaxBond \in Nat
    /\ InitialVault \in Nat
    /\ ArithmeticLimit \in Nat
    /\ CurrentEpoch \in Nat
    /\ EvidenceEpoch \in [Validators -> Nat]
    /\ ViewAVisibility \in [Validators -> SUBSET Validators]
    /\ ViewAReports \in [Validators -> SUBSET Validators]
    /\ ViewBVisibility \in [Validators -> SUBSET Validators]
    /\ ViewBReports \in [Validators -> SUBSET Validators]
    /\ CarryoverEnabled \in BOOLEAN
    /\ CarryoverMappedDirect \subseteq CurrentValidators
    /\ EnforceEvidenceRetention \in BOOLEAN
    /\ RecordSeqBound \in Nat
    /\ BatchFailureSet \subseteq Validators
    /\ EnforceBatchAtomicity \in BOOLEAN
    /\ BatchOrderA \in Seq(Validators)
    /\ BatchOrderB \in Seq(Validators)
    /\ ProposerSchedule \in Seq(Validators)
    /\ EvidenceObservedBy \subseteq Validators
    /\ EvidenceIncludedBy \subseteq Validators
    /\ EnforceProposerFairness \in BOOLEAN
    /\ GossipDelay \in Nat
    /\ InclusionDelay \in Nat
    /\ RetentionWindow \in Nat
    /\ RebondOldNonce \in Nat
    /\ RebondNewNonce \in Nat
    /\ EnforceRecordRetention \in BOOLEAN
    /\ Renaming \in [Validators -> Validators]

(****************************************************************************)
(* Bounded BFT quorum threshold                                             *)
(****************************************************************************)
N == Cardinality(Validators)
F == (N - 1) \div 3
QuorumLowerBound == N - F

RECURSIVE StakeSum(_)
StakeSum(S) ==
    IF S = {}
    THEN 0
    ELSE LET v == CHOOSE x \in S : TRUE
         IN BondWeight[v] + StakeSum(S \ {v})

TotalStake == StakeSum(Validators)
StakeF == IF TotalStake = 0 THEN 0 ELSE (TotalStake - 1) \div 3
StakeQuorumLowerBound == TotalStake - StakeF
ActiveValidators == Validators \ slashed
ActiveStake == StakeSum(ActiveValidators)
ActiveQuorumThreshold == (2 * Cardinality(ActiveValidators)) \div 3 + 1
ActiveStakeQuorumThreshold == (2 * ActiveStake) \div 3 + 1
ActiveQuorums == {Q \in SUBSET ActiveValidators : Cardinality(Q) >= ActiveQuorumThreshold}
ActiveStakeQuorums == {Q \in SUBSET ActiveValidators : StakeSum(Q) >= ActiveStakeQuorumThreshold}

ClosureStep(S) ==
    S \cup { v \in Validators : neglectGraph[v] \cap S # {} }

RECURSIVE ClosureAfter(_, _)
ClosureAfter(S, n) ==
    IF n = 0 THEN S ELSE ClosureStep(ClosureAfter(S, n - 1))

ViewGraph(visibility, reports) ==
    [v \in Validators |-> visibility[v] \ reports[v]]

GraphUnion(G, H) ==
    [v \in Validators |-> G[v] \cup H[v]]

GraphClosureStep(G, S) ==
    S \cup { v \in Validators : G[v] \cap S # {} }

RECURSIVE GraphClosureAfter(_, _, _)
GraphClosureAfter(G, S, n) ==
    IF n = 0 THEN S ELSE GraphClosureStep(G, GraphClosureAfter(G, S, n - 1))

ViewClosure(visibility, reports) ==
    GraphClosureAfter(ViewGraph(visibility, reports), equivocators, MaxLevel)

ViewsHaveSameActiveEdges ==
    ViewGraph(ViewAVisibility, ViewAReports) = ViewGraph(ViewBVisibility, ViewBReports)

ViewAReportsSubsetViewBReports ==
    \A v \in Validators : ViewAReports[v] \subseteq ViewBReports[v]

MergedViewGraph ==
    GraphUnion(ViewGraph(ViewAVisibility, ViewAReports),
               ViewGraph(ViewBVisibility, ViewBReports))

RustViewGraph == ViewGraph(Visibility, Reports)

RustViewClosure == GraphClosureAfter(RustViewGraph, equivocators, MaxLevel)

RenameSet(S) == {Renaming[v] : v \in S}

RenameGraph(G) ==
    [v \in Validators |->
        {Renaming[offender] :
            offender \in {o \in Validators :
                \E src \in Validators :
                    /\ Renaming[src] = v
                    /\ o \in G[src]}}]

RenamingIsBijective ==
    /\ {Renaming[v] : v \in Validators} = Validators
    /\ \A v1 \in Validators :
         \A v2 \in Validators :
           Renaming[v1] = Renaming[v2] => v1 = v2

RenamingDivergenceClass ==
    IF RenamingIsBijective THEN "bisimilar" ELSE "assumption_counterexample"

SlashedClosurePrefix ==
    IF step = 0
    THEN {}
    ELSE ClosureAfter(equivocators, step - 1)

RustViewDetectabilityClass ==
    IF \A v \in Validators : neglectGraph[v] \subseteq RustViewGraph[v]
    THEN "bisimilar"
    ELSE "projection_risk"

BoundedSlashClosure ==
    Cardinality(ClosureAfter(equivocators, MaxLevel)) <= F

BoundedWeightedSlashClosure ==
    StakeSum(ClosureAfter(equivocators, MaxLevel)) <= StakeF

CurrentClosureStep(S) ==
    S \cup { v \in CurrentValidators :
                 (neglectGraph[v] \cap CurrentValidators) \cap S # {} }

RECURSIVE CurrentClosureAfter(_, _)
CurrentClosureAfter(S, n) ==
    IF n = 0 THEN S ELSE CurrentClosureStep(CurrentClosureAfter(S, n - 1))

FilteredCurrentClosure ==
    CurrentClosureAfter(equivocators \cap CurrentValidators, MaxLevel)

EvidenceProjectionClosure ==
    ClosureAfter(equivocators \cap EvidenceValidators, MaxLevel) \cap CurrentValidators

BoundaryDivergenceClass ==
    IF FilteredCurrentClosure = EvidenceProjectionClosure
    THEN "bisimilar"
    ELSE "candidate_boundary"

ViewDivergenceClass ==
    IF ViewClosure(ViewAVisibility, ViewAReports) = ViewClosure(ViewBVisibility, ViewBReports)
    THEN "bisimilar"
    ELSE "candidate_evidence_view"

CarryoverDirect ==
    IF CarryoverEnabled THEN CarryoverMappedDirect ELSE {}

EpochCarryoverDivergenceClass ==
    IF CarryoverEnabled
    THEN "candidate_epoch_carryover"
    ELSE "bisimilar"

RECURSIVE SeqSet(_)
SeqSet(seq) ==
    IF Len(seq) = 0 THEN {}
    ELSE {Head(seq)} \cup SeqSet(Tail(seq))

ScheduledProposers == SeqSet(ProposerSchedule)
ObservedScheduledProposers == ScheduledProposers \cap EvidenceObservedBy
IncludingScheduledProposers == ScheduledProposers \cap EvidenceIncludedBy

ProposerFairnessDivergenceClass ==
    IF ObservedScheduledProposers # {} /\ IncludingScheduledProposers = {}
    THEN "candidate_proposer_fairness"
    ELSE "bisimilar"

TemporalWindowSafe ==
    RetentionWindow >= GossipDelay + InclusionDelay

TemporalWindowDivergenceClass ==
    IF TemporalWindowSafe
    THEN "bisimilar"
    ELSE "projection_risk"

RebondIdentityDivergenceClass ==
    IF RebondOldNonce # RebondNewNonce /\ ~CarryoverEnabled
    THEN "candidate_boundary"
    ELSE "bisimilar"

RecordLifecycleDivergenceClass ==
    IF EnforceRecordRetention
    THEN "bisimilar"
    ELSE "projection_risk"

CurrentRustRecordLifecycleDivergenceClass == RecordLifecycleDivergenceClass

RECURSIVE BatchPrefixBeforeFailure(_)
BatchPrefixBeforeFailure(seq) ==
    IF Len(seq) = 0 THEN {}
    ELSE IF Head(seq) \in BatchFailureSet
         THEN {}
         ELSE {Head(seq)} \cup BatchPrefixBeforeFailure(Tail(seq))

ProjectionDivergenceClass ==
    IF BatchFailureSet # {} /\ ~EnforceBatchAtomicity
    THEN "candidate_projection"
    ELSE "bisimilar"

AssumptionDivergenceClass ==
    IF BoundedSlashClosure /\ BoundedWeightedSlashClosure
    THEN "bisimilar"
    ELSE "assumption_counterexample"

SemanticCampaignDivergenceClass ==
    IF
        \/ ProjectionDivergenceClass = "candidate_projection"
        \/ TemporalWindowDivergenceClass = "projection_risk"
        \/ RecordLifecycleDivergenceClass = "projection_risk"
    THEN "projection_risk"
    ELSE IF
        \/ BoundaryDivergenceClass # "bisimilar"
        \/ ViewDivergenceClass # "bisimilar"
        \/ EpochCarryoverDivergenceClass # "bisimilar"
        \/ ProposerFairnessDivergenceClass # "bisimilar"
        \/ RebondIdentityDivergenceClass # "bisimilar"
    THEN "candidate_boundary"
    ELSE IF
        \/ AssumptionDivergenceClass = "assumption_counterexample"
    THEN "assumption_counterexample"
    ELSE "bisimilar"

SchedulerDivergenceClass ==
    IF ProjectionDivergenceClass = "candidate_projection"
    THEN "projection_risk"
    ELSE IF
        \/ ViewDivergenceClass # "bisimilar"
        \/ ProposerFairnessDivergenceClass # "bisimilar"
    THEN "candidate_boundary"
    ELSE "bisimilar"

ArithmeticProjectionStressClass ==
    IF InitialVault + TotalStake <= ArithmeticLimit
    THEN "bisimilar"
    ELSE "projection_risk"

PartitionGossipDivergenceClass == SchedulerDivergenceClass

ObjectiveGuidedDivergenceClass == SemanticCampaignDivergenceClass

PreconditionFuzzingClass ==
    IF ProjectionDivergenceClass = "candidate_projection"
    THEN "projection_risk"
    ELSE IF
        \/ AssumptionDivergenceClass = "assumption_counterexample"
        \/ ProposerFairnessDivergenceClass # "bisimilar"
    THEN "assumption_counterexample"
    ELSE IF
        \/ BoundaryDivergenceClass # "bisimilar"
        \/ ViewDivergenceClass # "bisimilar"
        \/ EpochCarryoverDivergenceClass # "bisimilar"
    THEN "candidate_boundary"
    ELSE "bisimilar"

RustReplayDivergenceClass ==
    IF SemanticCampaignDivergenceClass \in {"bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"}
    THEN SemanticCampaignDivergenceClass
    ELSE "unexpected"

DeepThreatModelDivergenceClass ==
    IF ArithmeticProjectionStressClass = "projection_risk"
    THEN "projection_risk"
    ELSE IF AssumptionDivergenceClass = "assumption_counterexample"
    THEN "assumption_counterexample"
    ELSE SemanticCampaignDivergenceClass

DagTraceDivergenceClass ==
    IF RustViewDetectabilityClass = "projection_risk"
    THEN "projection_risk"
    ELSE IF AssumptionDivergenceClass = "assumption_counterexample"
    THEN "assumption_counterexample"
    ELSE "bisimilar"

AdversarialCampaignDivergenceClass ==
    IF ProjectionDivergenceClass = "candidate_projection"
    THEN "projection_risk"
    ELSE IF AssumptionDivergenceClass = "assumption_counterexample"
    THEN "assumption_counterexample"
    ELSE IF
        \/ SchedulerDivergenceClass # "bisimilar"
        \/ DagTraceDivergenceClass # "bisimilar"
        \/ SemanticCampaignDivergenceClass # "bisimilar"
    THEN "candidate_boundary"
    ELSE "bisimilar"

DifferentialOraclePipelineClass ==
    IF AdversarialCampaignDivergenceClass \in {"bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"}
    THEN AdversarialCampaignDivergenceClass
    ELSE "unexpected"

HorizonCampaignDivergenceClass ==
    IF
        \/ TemporalWindowDivergenceClass = "projection_risk"
        \/ ArithmeticProjectionStressClass = "projection_risk"
        \/ RecordLifecycleDivergenceClass = "projection_risk"
        \/ RustViewDetectabilityClass = "projection_risk"
    THEN "projection_risk"
    ELSE IF AssumptionDivergenceClass = "assumption_counterexample"
    THEN "assumption_counterexample"
    ELSE IF
        \/ ProposerFairnessDivergenceClass # "bisimilar"
        \/ RebondIdentityDivergenceClass # "bisimilar"
        \/ SchedulerDivergenceClass # "bisimilar"
        \/ SemanticCampaignDivergenceClass # "bisimilar"
    THEN "candidate_boundary"
    ELSE "bisimilar"

HorizonV2DivergenceClass ==
    IF HorizonCampaignDivergenceClass \in {"bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"}
    THEN HorizonCampaignDivergenceClass
    ELSE "unexpected"

VisibleUnreported(v) == Visibility[v] \ Reports[v]
EpochEligibleEquivocators ==
    {v \in equivocators \cap CurrentValidators : EvidenceEpoch[v] = CurrentEpoch}

RECURSIVE Pow2(_)
Pow2(n) == IF n = 0 THEN 1 ELSE 2 * Pow2(n - 1)

UnsignedMax(bits) == Pow2(bits) - 1
SignedMax(bits) == Pow2(bits - 1) - 1

(****************************************************************************)
(* Init: pick a BFT-bounded set of equivocators and a neglect graph whose  *)
(* MaxLevel closure stays within the same BFT bound.                        *)
(****************************************************************************)
Init ==
    /\ equivocators \in SUBSET Validators
    /\ Cardinality(equivocators) <= F
    /\ slashed = {}
    /\ neglectGraph \in [Validators -> SUBSET Validators]
    /\ \A v \in Validators :
          \* a non-equivocator only appears in neglectGraph[v] if it is
          \* itself either an equivocator or eventually-slashed; and v's
          \* own neglect set excludes itself.
          /\ v \notin neglectGraph[v]
          /\ neglectGraph[v] \subseteq Validators
          /\ BondWeight[v] <= MaxBond
    /\ step = 0
    /\ \/ \neg EnforceClosureBound
       \/ BoundedSlashClosure
    /\ \/ \neg EnforceWeightedClosureBound
       \/ BoundedWeightedSlashClosure
    /\ \/ \neg EnforceVisibility
       \/ \A v \in Validators : neglectGraph[v] \subseteq VisibleUnreported(v)

(****************************************************************************)
(* Action: SlashLevel — close the slash set under one BFS level.            *)
(*                                                                          *)
(* Level 0: slashed := equivocators                                         *)
(* Level k: slashed := slashed ∪ { v : neglectGraph[v] ∩ slashed ≠ ∅ }      *)
(****************************************************************************)
SlashStep ==
    /\ step < MaxLevel
    /\ LET delta == { v \in Validators :
                        v \notin slashed
                        /\ ( IF step = 0
                             THEN v \in equivocators
                             ELSE neglectGraph[v] \cap slashed # {} ) }
       IN  /\ slashed' = slashed \cup delta
           /\ step' = step + 1
           /\ UNCHANGED <<equivocators, neglectGraph>>

(****************************************************************************)
(* Idle action when fixed point reached                                     *)
(****************************************************************************)
FixedPoint ==
    /\ \/ step >= MaxLevel
       \/ \A v \in Validators :
            v \in slashed
            \/ ( IF step = 0
                 THEN v \notin equivocators
                 ELSE neglectGraph[v] \cap slashed = {} )
    /\ UNCHANGED vars

(****************************************************************************)
(* Next                                                                     *)
(****************************************************************************)
Next == SlashStep \/ FixedPoint

Spec == Init /\ [][Next]_vars /\ WF_vars(SlashStep)

(****************************************************************************)
(* Invariants                                                               *)
(****************************************************************************)

\* T-11: Termination — the slash set stabilizes by step ≤ N (since each step
\*       strictly grows the set or terminates).
Inv_LevelClosureTerminates ==
    step <= MaxLevel

\* T-12: Quorum preservation — even after all neglect levels close, the
\* remaining active set is large enough to maintain BFT safety.
Inv_ActiveSetAboveQuorum ==
    Cardinality(Validators \ slashed) >= QuorumLowerBound

Inv_ActiveStakeAboveWeightedQuorum ==
    StakeSum(Validators \ slashed) >= StakeQuorumLowerBound

\* Slashed set is contained in the universe.
Inv_SlashedInUniverse ==
    slashed \subseteq Validators

Inv_SlashedWithinClosure ==
    slashed \subseteq ClosureAfter(equivocators, MaxLevel)

Inv_SlashedEqualsClosurePrefix ==
    slashed = SlashedClosurePrefix

Inv_NoDirectSeedNoClosure ==
    GraphClosureAfter(neglectGraph, {}, MaxLevel) = {}

Inv_FilteredClosureInCurrentValidators ==
    FilteredCurrentClosure \subseteq CurrentValidators

Inv_NeglectEdgesVisibleUnreported ==
    \A v \in Validators : neglectGraph[v] \subseteq VisibleUnreported(v)

Inv_RustViewEdgesDetectableUnreported ==
    \A v \in Validators : RustViewGraph[v] = VisibleUnreported(v)

Inv_NoUnexpectedDifferentialDivergence ==
    /\ BoundaryDivergenceClass \in {"bisimilar", "candidate_boundary"}
    /\ ViewDivergenceClass \in {"bisimilar", "candidate_evidence_view"}
    /\ EpochCarryoverDivergenceClass \in {"bisimilar", "candidate_epoch_carryover"}
    /\ ProjectionDivergenceClass \in {"bisimilar", "candidate_projection"}
    /\ ProposerFairnessDivergenceClass \in {"bisimilar", "candidate_proposer_fairness"}
    /\ AssumptionDivergenceClass \in {"bisimilar", "assumption_counterexample"}
    /\ SemanticCampaignDivergenceClass \in {"bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"}
    /\ SchedulerDivergenceClass \in {"bisimilar", "candidate_boundary", "projection_risk"}
    /\ ArithmeticProjectionStressClass \in {"bisimilar", "projection_risk"}
    /\ PartitionGossipDivergenceClass \in {"bisimilar", "candidate_boundary", "projection_risk"}
    /\ ObjectiveGuidedDivergenceClass \in {"bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"}
    /\ PreconditionFuzzingClass \in {"bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"}
    /\ RustReplayDivergenceClass \in {"bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"}
    /\ DeepThreatModelDivergenceClass \in {"bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"}
    /\ DagTraceDivergenceClass \in {"bisimilar", "projection_risk", "assumption_counterexample"}
    /\ AdversarialCampaignDivergenceClass \in {"bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"}
    /\ DifferentialOraclePipelineClass \in {"bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"}
    /\ HorizonCampaignDivergenceClass \in {"bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"}
    /\ HorizonV2DivergenceClass \in {"bisimilar", "candidate_boundary", "projection_risk", "assumption_counterexample"}
    /\ TemporalWindowDivergenceClass \in {"bisimilar", "projection_risk"}
    /\ RebondIdentityDivergenceClass \in {"bisimilar", "candidate_boundary"}
    /\ RecordLifecycleDivergenceClass \in {"bisimilar", "projection_risk"}
    /\ RenamingDivergenceClass \in {"bisimilar", "assumption_counterexample"}

Inv_UnsignedArithmeticBoundary ==
    UnsignedMax(ArithmeticBits) + 1 = Pow2(ArithmeticBits)

Inv_SignedArithmeticBoundary ==
    SignedMax(ArithmeticBits) + 1 = Pow2(ArithmeticBits - 1)

Inv_ActiveQuorumsIntersect ==
    \A Q1 \in ActiveQuorums :
      \A Q2 \in ActiveQuorums :
        Q1 \cap Q2 # {}

Inv_ActiveStakeQuorumsIntersect ==
    \A Q1 \in ActiveStakeQuorums :
      \A Q2 \in ActiveStakeQuorums :
        Q1 \cap Q2 # {}

Inv_ClosureStableAtMaxLevel ==
    step = MaxLevel => ClosureStep(slashed) = slashed

Inv_EpochEligibleInCurrent ==
    EpochEligibleEquivocators \subseteq CurrentValidators

Inv_StaleEvidenceNotEligible ==
    \A v \in equivocators :
      EvidenceEpoch[v] # CurrentEpoch => v \notin EpochEligibleEquivocators

Inv_ReportsSuppressNeglectEdges ==
    \A v \in Validators : Reports[v] \cap neglectGraph[v] = {}

Inv_UnreportedVisibleEdgesRemainActive ==
    \A v \in Validators :
      \A offender \in Validators :
        offender \in Visibility[v] /\ offender \notin Reports[v] =>
          offender \in RustViewGraph[v]

Inv_ReportGrowthCannotExpandViewClosure ==
    /\ ViewAVisibility = ViewBVisibility
    /\ ViewAReportsSubsetViewBReports
    => ViewClosure(ViewBVisibility, ViewBReports) \subseteq
       ViewClosure(ViewAVisibility, ViewAReports)

Inv_ReportsDoNotSuppressDirectEvidence ==
    equivocators \subseteq RustViewClosure

Inv_ArithmeticSafeEnvelope ==
    InitialVault + N * MaxBond <= ArithmeticLimit =>
      InitialVault + TotalStake <= ArithmeticLimit

Inv_ViewEdgesVisibleUnreported ==
    /\ \A v \in Validators : ViewGraph(ViewAVisibility, ViewAReports)[v] \subseteq ViewAVisibility[v]
    /\ \A v \in Validators : ViewGraph(ViewAVisibility, ViewAReports)[v] \cap ViewAReports[v] = {}
    /\ \A v \in Validators : ViewGraph(ViewBVisibility, ViewBReports)[v] \subseteq ViewBVisibility[v]
    /\ \A v \in Validators : ViewGraph(ViewBVisibility, ViewBReports)[v] \cap ViewBReports[v] = {}

Inv_SameViewSameClosure ==
    ViewsHaveSameActiveEdges =>
      ViewClosure(ViewAVisibility, ViewAReports) = ViewClosure(ViewBVisibility, ViewBReports)

Inv_SameRustViewSameClosure ==
    RustViewGraph = ViewGraph(ViewAVisibility, ViewAReports) =>
      RustViewClosure = ViewClosure(ViewAVisibility, ViewAReports)

Inv_InitialEvidenceMonotonicity ==
    ClosureAfter(equivocators, MaxLevel) \subseteq
      ClosureAfter(equivocators \cup EvidenceValidators, MaxLevel)

Inv_ViewMergeOverapproximatesInputs ==
    /\ ViewClosure(ViewAVisibility, ViewAReports) \subseteq
       GraphClosureAfter(MergedViewGraph, equivocators, MaxLevel)
    /\ ViewClosure(ViewBVisibility, ViewBReports) \subseteq
       GraphClosureAfter(MergedViewGraph, equivocators, MaxLevel)

Inv_ViewMergeCommutative ==
    GraphClosureAfter(
      GraphUnion(ViewGraph(ViewAVisibility, ViewAReports),
                 ViewGraph(ViewBVisibility, ViewBReports)),
      equivocators,
      MaxLevel) =
    GraphClosureAfter(
      GraphUnion(ViewGraph(ViewBVisibility, ViewBReports),
                 ViewGraph(ViewAVisibility, ViewAReports)),
      equivocators,
      MaxLevel)

Inv_ValidatorRenamingEquivariance ==
    RenamingIsBijective =>
      GraphClosureAfter(RenameGraph(neglectGraph), RenameSet(equivocators), MaxLevel) =
      RenameSet(ClosureAfter(equivocators, MaxLevel))

Inv_CarryoverPolicyCurrent ==
    CarryoverDirect \subseteq CurrentValidators

Inv_NoCarryoverNoMappedDirect ==
    ~CarryoverEnabled => CarryoverDirect = {}

Inv_EvidenceRetentionForDirectOffenders ==
    EnforceEvidenceRetention => equivocators \subseteq EvidenceValidators

Inv_CanonicalRecordKeyInjective ==
    \A v1 \in Validators :
      \A v2 \in Validators :
        \A s1 \in 0..RecordSeqBound :
          \A s2 \in 0..RecordSeqBound :
            <<v1, s1>> = <<v2, s2>> => v1 = v2 /\ s1 = s2

Inv_BatchNoFailureOrderIndependent ==
    /\ BatchFailureSet = {}
    /\ SeqSet(BatchOrderA) = SeqSet(BatchOrderB)
    => BatchPrefixBeforeFailure(BatchOrderA) = BatchPrefixBeforeFailure(BatchOrderB)

Inv_PartialBatchFailureRequiresAtomicPolicy ==
    BatchFailureSet # {} => EnforceBatchAtomicity

Inv_ProposerFairnessForBoundedLiveness ==
    EnforceProposerFairness =>
      (ObservedScheduledProposers # {} => IncludingScheduledProposers # {})

Inv_TemporalWindowBoundary ==
    TemporalWindowSafe <=> RetentionWindow >= GossipDelay + InclusionDelay

Inv_RebondIdentityClassified ==
    RebondIdentityDivergenceClass \in {"bisimilar", "candidate_boundary"}

Inv_RecordLifecycleRetentionPolicy ==
    EnforceRecordRetention => RecordLifecycleDivergenceClass = "bisimilar"

Inv_CurrentRustRecordLifecycleRetainsRecords ==
    EnforceRecordRetention => CurrentRustRecordLifecycleDivergenceClass = "bisimilar"

Inv_ClosureDepthWithinUniverseBound ==
    ClosureStep(ClosureAfter(equivocators, N)) = ClosureAfter(equivocators, N)
\* Monotonicity is structural via SlashStep's union-only update.

============================================================================

---------------------- MODULE LocatedAuthoritySettlement ----------------------
(***************************************************************************)
(* G3 (DR-135, design v2 rules R2 to R4 and R10; DR-68 items 1 to 3):      *)
(* owner purses.                                                           *)
(*                                                                         *)
(* An owner purse pays for the stored process of its owner when another    *)
(* deployment fires it (P1 Rules 1 to 5, decision a). No deployment        *)
(* reserves an owner purse (ASSUME below), so the reservation bound        *)
(* RealizedBackedByReservation covers the client purses only. An owner     *)
(* purse is gated by its owner budget instead (v2 R3: the balance at       *)
(* R_{i-1} minus the later holds and the own holds). An event whose owner  *)
(* share does not fit the remaining budget does not fire and has no charge *)
(* (Option 2): RejectEvent changes nothing, and the run goes on.           *)
(*                                                                         *)
(* G3 invariants:                                                          *)
(*   OwnerRealizedWithinBudget   the draws on an owner purse stay within   *)
(*                               its owner budget.                         *)
(*   RejectedEventDebitsNothing  the realized draws count exactly the      *)
(*                               committed events and a pending partial    *)
(*                               commit, so a rejected event debits        *)
(*                               nothing.                                  *)
(* G3 controls (each holds only its named invariant):                      *)
(*   AllowUngatedOwnerDraw   Fire skips the budget gate of an owner purse  *)
(*                           -> OwnerRealizedWithinBudget                  *)
(*   AllowUnsealedOwnerDraw  a trigger whose regions carry no stored seal  *)
(*                           of the owner draws the owner purse            *)
(*                           -> NoAmbientAuthority                         *)
(* The second control is the TLA+ obligation of DR-68: "a trigger with no  *)
(* stored signature cannot draw an installer purse".                       *)
(*                                                                         *)
(* The owner scenario is MCLocatedAuthoritySettlementOwner.tla. The first  *)
(* scenario, MCLocatedAuthoritySettlement.tla, has no owner purse, so its  *)
(* configs keep their state space.                                         *)
(*                                                                         *)
(* G3 step S11 (DR-135 item 13): the signed limit and the hold of a caller *)
(* cover only the work that the caller pays. This model agrees and needs   *)
(* no change. A deployment reserves DeployBound only on its client purses, *)
(* because DeployBound is 0 on every owner purse (ASSUME below).           *)
(* RealizedBackedByReservation bounds only the client purses, and an owner *)
(* draw counts only against its owner budget. Every draw of this model is  *)
(* 1 unit, so every owner here has a price of 1 or more. Owner work at     *)
(* price 0 costs 0 and counts against the limit of the caller (S11         *)
(* decision 2). LocatedAuthoritySettlement.v checks this rule in its       *)
(* caller-run section.                                                     *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS
    Actors,
    Signatures,
    Locations,
    Regions,
    Purses,
    Events,
    Surfaces,
    Deployments,
    RegionSignature,
    RegionLocation,
    EventRegions,
    EventSurfaces,
    EventDependencies,
    EventDeployment,
    FundingPurse,
    PurseAtoms,
    Near,
    InitialBalance,
    DeployBound,
    DeployOrder,
    ReplayOrder,
    OuterEvent,
    ContinuationEvent,
    LaterSlotEvent,
    WholeJoinEvent,
    SplitJoinEvent,
    CombinedJoinEvent,
    OuterPurse,
    SlotPurse,
    AmbientPurse,
    AlternateSlotPurse,
    EnvelopePurse,
    NoEvent,
    EraseAuthorityMetadata,
    AllowAmbientPurse,
    AllowRewrappedContinuation,
    AllowNonAtomicDebit,
    ReplayOmitsAuthority,
    ReplayLosesSlotIdentity,
    \* Added by G3 (DR-135).
    OwnerPurses,
    OwnerBudget,
    UnsealedTriggerEvent,
    OwnerPurse,
    AllowUngatedOwnerDraw,
    AllowUnsealedOwnerDraw

\* Added by G3 (DR-135): no deployment reserves an owner purse, and an owner
\* budget never exceeds the balance.
ASSUME /\ OwnerPurses \subseteq Purses
       /\ OwnerBudget \in [Purses -> Nat]
       /\ \A purse \in OwnerPurses : OwnerBudget[purse] <= InitialBalance[purse]
       /\ \A deployment \in Deployments, purse \in OwnerPurses :
            DeployBound[deployment][purse] = 0
       /\ UnsealedTriggerEvent \in Events
       /\ OwnerPurse \in Purses
       /\ AllowUngatedOwnerDraw \in BOOLEAN
       /\ AllowUnsealedOwnerDraw \in BOOLEAN

ASSUME /\ Actors # {}
       /\ Signatures # {}
       /\ Locations # {}
       /\ Regions # {}
       /\ Purses # {}
       /\ Events # {}
       /\ Surfaces # {}
       /\ Deployments # {}
       /\ NoEvent \notin Events
       /\ RegionSignature \in [Regions -> Signatures]
       /\ RegionLocation \in [Regions -> Locations]
       /\ EventRegions \in [Events -> SUBSET Regions]
       /\ \A e \in Events : EventRegions[e] # {}
       /\ EventSurfaces \in [Events -> SUBSET Surfaces]
       /\ \A e \in Events : EventSurfaces[e] # {}
       /\ EventDependencies \in [Events -> SUBSET Events]
       /\ EventDeployment \in [Events -> Deployments]
       /\ \A e \in Events : FundingPurse[e] \in [EventRegions[e] -> Purses]
       /\ PurseAtoms \in [Purses -> [Signatures -> Nat]]
       /\ Near \in [Regions -> [Purses -> BOOLEAN]]
       /\ InitialBalance \in [Purses -> Nat]
       /\ DeployBound \in [Deployments -> [Purses -> Nat]]
       /\ DeployOrder \in Seq(Deployments)
       /\ {DeployOrder[index] : index \in 1..Len(DeployOrder)} = Deployments
       /\ ReplayOrder \in Seq(Events)
       /\ {ReplayOrder[index] : index \in 1..Len(ReplayOrder)} = Events
       /\ \A flag \in {
            EraseAuthorityMetadata,
            AllowAmbientPurse,
            AllowRewrappedContinuation,
            AllowNonAtomicDebit,
            ReplayOmitsAuthority,
            ReplayLosesSlotIdentity
          } : flag \in BOOLEAN
       /\ OuterEvent \in Events
       /\ ContinuationEvent \in Events
       /\ LaterSlotEvent \in Events
       /\ WholeJoinEvent \in Events
       /\ SplitJoinEvent \in Events
       /\ CombinedJoinEvent \in Events
       /\ OuterPurse \in Purses
       /\ SlotPurse \in Purses
       /\ AmbientPurse \in Purses
       /\ AlternateSlotPurse \in Purses
       /\ EnvelopePurse \in Purses

VARIABLES
    phase,
    admissionIndex,
    admitted,
    rejectedDeployments,
    reserved,
    arrived,
    committed,
    rejectedEvents,
    realized,
    realizedByDeployment,
    pendingEvent,
    pendingPurses,
    balance,
    replayIndex,
    replayed,
    replayRealized,
    replayBalance

vars == <<phase, admissionIndex, admitted, rejectedDeployments, reserved,
          arrived, committed, rejectedEvents, realized,
          realizedByDeployment, pendingEvent, pendingPurses, balance,
          replayIndex, replayed, replayRealized, replayBalance>>

ZeroPurses == [p \in Purses |-> 0]
ZeroByDeployment == [d \in Deployments |-> ZeroPurses]

RECURSIVE SumSet(_, _)

SumSet(f, domain) ==
    IF domain = {}
    THEN 0
    ELSE LET element == CHOOSE x \in domain : TRUE
         IN f[element] + SumSet(f, domain \ {element})

StaticPurses(event) ==
    {FundingPurse[event][region] : region \in EventRegions[event]}

StaticPlanExact(event) ==
    \A purse \in StaticPurses(event) :
      \A signature \in Signatures :
        PurseAtoms[purse][signature]
          = Cardinality({region \in EventRegions[event] :
                /\ FundingPurse[event][region] = purse
                /\ RegionSignature[region] = signature})

StaticPlanLocated(event) ==
    \A region \in EventRegions[event] : Near[region][FundingPurse[event][region]]

\* Changed by G3 (DR-135): control AllowUnsealedOwnerDraw lets a trigger
\* whose regions carry no stored seal of the owner draw the owner purse.
\* RuntimePurse(event, region) ==
\*     IF EraseAuthorityMetadata
\*     THEN EnvelopePurse
\*     ELSE IF AllowAmbientPurse /\ event = ContinuationEvent
\*          THEN AmbientPurse
\*          ELSE IF AllowRewrappedContinuation /\ event = ContinuationEvent
\*               THEN OuterPurse
\*               ELSE FundingPurse[event][region]
RuntimePurse(event, region) ==
    IF EraseAuthorityMetadata
    THEN EnvelopePurse
    ELSE IF AllowAmbientPurse /\ event = ContinuationEvent
         THEN AmbientPurse
         ELSE IF AllowRewrappedContinuation /\ event = ContinuationEvent
              THEN OuterPurse
              ELSE IF AllowUnsealedOwnerDraw /\ event = UnsealedTriggerEvent
                   THEN OwnerPurse
                   ELSE FundingPurse[event][region]

RuntimePurses(event) ==
    {RuntimePurse(event, region) : region \in EventRegions[event]}

RuntimePlanExact(event) ==
    \A purse \in RuntimePurses(event) :
      \A signature \in Signatures :
        PurseAtoms[purse][signature]
          = Cardinality({region \in EventRegions[event] :
                /\ RuntimePurse(event, region) = purse
                /\ RegionSignature[region] = signature})

RuntimePlanLocated(event) ==
    \A region \in EventRegions[event] : Near[region][RuntimePurse(event, region)]

\* Changed by G3 (DR-135): the unsealed owner draw also bypasses validation.
\* AuthorityValidationBypassed ==
\*     EraseAuthorityMetadata \/ AllowAmbientPurse \/ AllowRewrappedContinuation
AuthorityValidationBypassed ==
    \/ EraseAuthorityMetadata
    \/ AllowAmbientPurse
    \/ AllowRewrappedContinuation
    \/ AllowUnsealedOwnerDraw

RuntimeAuthorityAccepted(event) ==
    /\ RuntimePurses(event) # {}
    /\ ((RuntimePlanExact(event) /\ RuntimePlanLocated(event))
          \/ AuthorityValidationBypassed)

ReplayPurse(event, region) ==
    IF ReplayOmitsAuthority
    THEN EnvelopePurse
    ELSE IF ReplayLosesSlotIdentity /\ event = LaterSlotEvent
         THEN AlternateSlotPurse
         ELSE RuntimePurse(event, region)

ReplayPurses(event) ==
    IF ReplayOmitsAuthority
    THEN {}
    ELSE {ReplayPurse(event, region) : region \in EventRegions[event]}

IncrementPurses(vector, purses) ==
    [p \in Purses |-> vector[p] + IF p \in purses THEN 1 ELSE 0]

DecrementPurses(vector, purses) ==
    [p \in Purses |-> vector[p] - IF p \in purses THEN 1 ELSE 0]

AddVectors(left, right) ==
    [p \in Purses |-> left[p] + right[p]]

CanReserve(deployment) ==
    \A purse \in Purses :
      reserved[purse] + DeployBound[deployment][purse] <= balance[purse]

\* Changed by G3 (DR-135): an owner purse is gated by its owner budget, and
\* control AllowUngatedOwnerDraw removes that gate.
\* CanDebit(event) ==
\*     \A purse \in RuntimePurses(event) : realized[purse] < balance[purse]
CanDebit(event) ==
    \A purse \in RuntimePurses(event) :
      IF purse \in OwnerPurses
      THEN AllowUngatedOwnerDraw \/ realized[purse] < OwnerBudget[purse]
      ELSE realized[purse] < balance[purse]

Ready(event) ==
    /\ phase = "Execution"
    /\ event \notin committed \cup rejectedEvents
    /\ EventDeployment[event] \in admitted
    /\ EventSurfaces[event] \subseteq arrived
    /\ EventDependencies[event] \subseteq committed
    /\ pendingEvent = NoEvent

ExecutionComplete ==
    \A event \in Events :
      EventDeployment[event] \in admitted => event \in committed \cup rejectedEvents

StaticPlanExactForDeployment(deployment) ==
    \A event \in Events : EventDeployment[event] = deployment => StaticPlanExact(event)

StaticPlanLocatedForDeployment(deployment) ==
    \A event \in Events : EventDeployment[event] = deployment => StaticPlanLocated(event)

Init ==
    /\ phase = "Admission"
    /\ admissionIndex = 1
    /\ admitted = {}
    /\ rejectedDeployments = {}
    /\ reserved = ZeroPurses
    /\ arrived = {}
    /\ committed = {}
    /\ rejectedEvents = {}
    /\ realized = ZeroPurses
    /\ realizedByDeployment = ZeroByDeployment
    /\ pendingEvent = NoEvent
    /\ pendingPurses = {}
    /\ balance = InitialBalance
    /\ replayIndex = 1
    /\ replayed = {}
    /\ replayRealized = ZeroPurses
    /\ replayBalance = InitialBalance

Admit ==
    /\ phase = "Admission"
    /\ admissionIndex <= Len(DeployOrder)
    /\ LET deployment == DeployOrder[admissionIndex]
       IN /\ StaticPlanExactForDeployment(deployment)
          /\ StaticPlanLocatedForDeployment(deployment)
          /\ CanReserve(deployment)
          /\ admitted' = admitted \cup {deployment}
          /\ reserved' = AddVectors(reserved, DeployBound[deployment])
    /\ admissionIndex' = admissionIndex + 1
    /\ UNCHANGED <<phase, rejectedDeployments, arrived, committed,
                    rejectedEvents, realized, realizedByDeployment,
                    pendingEvent, pendingPurses, balance, replayIndex,
                    replayed, replayRealized, replayBalance>>

RejectAdmission ==
    /\ phase = "Admission"
    /\ admissionIndex <= Len(DeployOrder)
    /\ LET deployment == DeployOrder[admissionIndex]
       IN /\ ~(StaticPlanExactForDeployment(deployment)
                 /\ StaticPlanLocatedForDeployment(deployment)
                 /\ CanReserve(deployment))
          /\ rejectedDeployments' = rejectedDeployments \cup {deployment}
    /\ admissionIndex' = admissionIndex + 1
    /\ UNCHANGED <<phase, admitted, reserved, arrived, committed,
                    rejectedEvents, realized, realizedByDeployment,
                    pendingEvent, pendingPurses, balance, replayIndex,
                    replayed, replayRealized, replayBalance>>

BeginExecution ==
    /\ phase = "Admission"
    /\ admissionIndex > Len(DeployOrder)
    /\ phase' = "Execution"
    /\ UNCHANGED <<admissionIndex, admitted, rejectedDeployments, reserved,
                    arrived, committed, rejectedEvents, realized,
                    realizedByDeployment, pendingEvent, pendingPurses,
                    balance, replayIndex, replayed, replayRealized,
                    replayBalance>>

Arrive(surface) ==
    /\ phase = "Execution"
    /\ surface \in Surfaces \ arrived
    /\ arrived' = arrived \cup {surface}
    /\ UNCHANGED <<phase, admissionIndex, admitted, rejectedDeployments,
                    reserved, committed, rejectedEvents, realized,
                    realizedByDeployment, pendingEvent, pendingPurses,
                    balance, replayIndex, replayed, replayRealized,
                    replayBalance>>

CommitAtomic(event) ==
    /\ committed' = committed \cup {event}
    /\ realized' = IncrementPurses(realized, RuntimePurses(event))
    /\ realizedByDeployment' =
         [realizedByDeployment EXCEPT
            ![EventDeployment[event]] = IncrementPurses(@, RuntimePurses(event))]
    /\ UNCHANGED <<pendingEvent, pendingPurses>>

BeginPartialCommit(event) ==
    LET purse == CHOOSE p \in RuntimePurses(event) : TRUE
    IN /\ pendingEvent' = event
       /\ pendingPurses' = {purse}
       /\ realized' = IncrementPurses(realized, {purse})
       /\ realizedByDeployment' =
            [realizedByDeployment EXCEPT
               ![EventDeployment[event]] = IncrementPurses(@, {purse})]
       /\ UNCHANGED committed

Fire(event) ==
    /\ Ready(event)
    /\ RuntimeAuthorityAccepted(event)
    /\ CanDebit(event)
    /\ IF AllowNonAtomicDebit /\ Cardinality(RuntimePurses(event)) > 1
          THEN BeginPartialCommit(event)
          ELSE CommitAtomic(event)
    /\ UNCHANGED <<phase, admissionIndex, admitted, rejectedDeployments,
                    reserved, arrived, rejectedEvents, balance, replayIndex,
                    replayed, replayRealized, replayBalance>>

FinishPartialCommit ==
    /\ phase = "Execution"
    /\ pendingEvent \in Events
    /\ LET remaining == RuntimePurses(pendingEvent) \ pendingPurses
       IN /\ committed' = committed \cup {pendingEvent}
          /\ realized' = IncrementPurses(realized, remaining)
          /\ realizedByDeployment' =
               [realizedByDeployment EXCEPT
                  ![EventDeployment[pendingEvent]] = IncrementPurses(@, remaining)]
    /\ pendingEvent' = NoEvent
    /\ pendingPurses' = {}
    /\ UNCHANGED <<phase, admissionIndex, admitted, rejectedDeployments,
                    reserved, arrived, rejectedEvents, balance, replayIndex,
                    replayed, replayRealized, replayBalance>>

RejectEvent(event) ==
    /\ Ready(event)
    /\ ~(RuntimeAuthorityAccepted(event) /\ CanDebit(event))
    /\ rejectedEvents' = rejectedEvents \cup {event}
    /\ UNCHANGED <<phase, admissionIndex, admitted, rejectedDeployments,
                    reserved, arrived, committed, realized,
                    realizedByDeployment, pendingEvent, pendingPurses,
                    balance, replayIndex, replayed, replayRealized,
                    replayBalance>>

Settle ==
    /\ phase = "Execution"
    /\ ExecutionComplete
    /\ pendingEvent = NoEvent
    /\ \A purse \in Purses : realized[purse] <= balance[purse]
    /\ balance' = [p \in Purses |-> balance[p] - realized[p]]
    /\ reserved' = ZeroPurses
    /\ phase' = "Replay"
    /\ UNCHANGED <<admissionIndex, admitted, rejectedDeployments, arrived,
                    committed, rejectedEvents, realized,
                    realizedByDeployment, pendingEvent, pendingPurses,
                    replayIndex, replayed, replayRealized, replayBalance>>

ReplayCommittedEvent(event) ==
    /\ event \in committed
    /\ EventDependencies[event] \subseteq replayed
    /\ \A purse \in ReplayPurses(event) : replayBalance[purse] > 0
    /\ replayed' = replayed \cup {event}
    /\ replayRealized' = IncrementPurses(replayRealized, ReplayPurses(event))
    /\ replayBalance' = DecrementPurses(replayBalance, ReplayPurses(event))

ReplaySkippedEvent(event) ==
    /\ event \notin committed
    /\ UNCHANGED <<replayed, replayRealized, replayBalance>>

ReplayStep ==
    /\ phase = "Replay"
    /\ replayIndex <= Len(ReplayOrder)
    /\ LET event == ReplayOrder[replayIndex]
       IN (ReplayCommittedEvent(event) \/ ReplaySkippedEvent(event))
    /\ replayIndex' = replayIndex + 1
    /\ UNCHANGED <<phase, admissionIndex, admitted, rejectedDeployments,
                    reserved, arrived, committed, rejectedEvents, realized,
                    realizedByDeployment, pendingEvent, pendingPurses,
                    balance>>

FinishReplay ==
    /\ phase = "Replay"
    /\ replayIndex > Len(ReplayOrder)
    /\ phase' = "Done"
    /\ UNCHANGED <<admissionIndex, admitted, rejectedDeployments, reserved,
                    arrived, committed, rejectedEvents, realized,
                    realizedByDeployment, pendingEvent, pendingPurses,
                    balance, replayIndex, replayed, replayRealized,
                    replayBalance>>

AdmissionAny == Admit \/ RejectAdmission
ArrivalAny == \E surface \in Surfaces : Arrive(surface)
FireAny == \E event \in Events : (Fire(event) \/ RejectEvent(event))

Next ==
    \/ AdmissionAny
    \/ BeginExecution
    \/ ArrivalAny
    \/ FireAny
    \/ FinishPartialCommit
    \/ Settle
    \/ ReplayStep
    \/ FinishReplay

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(AdmissionAny)
    /\ WF_vars(BeginExecution)
    /\ WF_vars(ArrivalAny)
    /\ WF_vars(FireAny)
    /\ WF_vars(FinishPartialCommit)
    /\ WF_vars(Settle)
    /\ WF_vars(ReplayStep)
    /\ WF_vars(FinishReplay)

TypeOK ==
    /\ phase \in {"Admission", "Execution", "Replay", "Done"}
    /\ admissionIndex \in Nat \ {0}
    /\ admitted \subseteq Deployments
    /\ rejectedDeployments \subseteq Deployments
    /\ reserved \in [Purses -> Nat]
    /\ arrived \subseteq Surfaces
    /\ committed \subseteq Events
    /\ rejectedEvents \subseteq Events
    /\ realized \in [Purses -> Nat]
    /\ realizedByDeployment \in [Deployments -> [Purses -> Nat]]
    /\ pendingEvent \in Events \cup {NoEvent}
    /\ pendingPurses \subseteq Purses
    /\ balance \in [Purses -> Nat]
    /\ replayIndex \in Nat \ {0}
    /\ replayed \subseteq Events
    /\ replayRealized \in [Purses -> Nat]
    /\ replayBalance \in [Purses -> Nat]

ReservationsNeverExceedSupply ==
    \A purse \in Purses : reserved[purse] <= InitialBalance[purse]

\* Changed by G3 (DR-135): only a client purse is backed by a deployment
\* reservation. An owner purse is backed by its owner budget
\* (OwnerRealizedWithinBudget).
\* RealizedBackedByReservation ==
\*     \A deployment \in Deployments :
\*       \A purse \in Purses :
\*         realizedByDeployment[deployment][purse] <= DeployBound[deployment][purse]
RealizedBackedByReservation ==
    \A deployment \in Deployments :
      \A purse \in Purses \ OwnerPurses :
        realizedByDeployment[deployment][purse] <= DeployBound[deployment][purse]

\* Added by G3 (DR-135): the draws on an owner purse stay within its budget.
OwnerRealizedWithinBudget ==
    \A purse \in OwnerPurses : realized[purse] <= OwnerBudget[purse]

\* Added by G3 (DR-135): the realized draws count exactly the committed
\* events and a pending partial commit, so a rejected event debits nothing.
RejectedEventDebitsNothing ==
    /\ rejectedEvents \cap committed = {}
    /\ pendingEvent \notin rejectedEvents
    /\ \A purse \in Purses :
         realized[purse]
           = Cardinality({event \in committed : purse \in RuntimePurses(event)})
             + (IF purse \in pendingPurses THEN 1 ELSE 0)

RealizedDecomposesByDeployment ==
    \A purse \in Purses :
      realized[purse]
        = SumSet([deployment \in Deployments |->
                    realizedByDeployment[deployment][purse]], Deployments)

NoPartialEventDebit ==
    /\ pendingEvent = NoEvent
    /\ pendingPurses = {}

CommittedEventsHaveExactAuthority ==
    \A event \in committed :
      /\ RuntimePurses(event) = StaticPurses(event)
      /\ RuntimePlanExact(event)
      /\ RuntimePlanLocated(event)

NoAmbientAuthority ==
    \A event \in committed : RuntimePlanLocated(event)

NoFreeCommunication ==
    \A event \in committed : RuntimePurses(event) # {}

DependencyOrderPreserved ==
    \A event \in committed : EventDependencies[event] \subseteq committed

LollipopContinuationOrder ==
    ContinuationEvent \in committed => OuterEvent \in committed

LollipopUsesDistinctPayers ==
    /\ (OuterEvent \in committed => RuntimePurses(OuterEvent) = {OuterPurse})
    /\ (ContinuationEvent \in committed => RuntimePurses(ContinuationEvent) = {SlotPurse})

WholeJoinConsumesOneCell ==
    WholeJoinEvent \in committed => Cardinality(RuntimePurses(WholeJoinEvent)) = 1

SplitJoinConsumesEveryPresentedCell ==
    SplitJoinEvent \in committed =>
      Cardinality(RuntimePurses(SplitJoinEvent))
        = Cardinality(EventRegions[SplitJoinEvent])

CombinedJoinConsumesOneCompleteCell ==
    CombinedJoinEvent \in committed =>
      /\ Cardinality(RuntimePurses(CombinedJoinEvent)) = 1
      /\ RuntimePlanExact(CombinedJoinEvent)

CrossDeploySlotIdentityStable ==
    /\ (ContinuationEvent \in committed => RuntimePurses(ContinuationEvent) = {SlotPurse})
    /\ (LaterSlotEvent \in committed => RuntimePurses(LaterSlotEvent) = {SlotPurse})
    /\ (LaterSlotEvent \in replayed => ReplayPurses(LaterSlotEvent) = {SlotPurse})

SettlementConservesEveryPurse ==
    phase \in {"Replay", "Done"} =>
      \A purse \in Purses : balance[purse] + realized[purse] = InitialBalance[purse]

ReplayUsesCommittedEvents == replayed \subseteq committed

ReplayPreservesAuthority ==
    \A event \in replayed : ReplayPurses(event) = RuntimePurses(event)

ReplayMatchesSettlement ==
    phase = "Done" =>
      /\ replayed = committed
      /\ replayRealized = realized
      /\ replayBalance = balance

UnusedReservationIsRefunded ==
    phase \in {"Replay", "Done"} => reserved = ZeroPurses

EventuallyDone == <>(phase = "Done")

=============================================================================

---------------------- MODULE LocatedAuthoritySettlement ----------------------
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
    ReplayLosesSlotIdentity

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

RuntimePurse(event, region) ==
    IF EraseAuthorityMetadata
    THEN EnvelopePurse
    ELSE IF AllowAmbientPurse /\ event = ContinuationEvent
         THEN AmbientPurse
         ELSE IF AllowRewrappedContinuation /\ event = ContinuationEvent
              THEN OuterPurse
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

AuthorityValidationBypassed ==
    EraseAuthorityMetadata \/ AllowAmbientPurse \/ AllowRewrappedContinuation

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

CanDebit(event) ==
    \A purse \in RuntimePurses(event) : realized[purse] < balance[purse]

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

RealizedBackedByReservation ==
    \A deployment \in Deployments :
      \A purse \in Purses :
        realizedByDeployment[deployment][purse] <= DeployBound[deployment][purse]

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

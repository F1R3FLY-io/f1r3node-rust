--------------------- MODULE RecoveryFrontierCoverage ---------------------
EXTENDS FiniteSets, Naturals

CONSTANTS
    \* @type: Bool;
    UseCollectiveCoverage,
    \* @type: Int;
    LeaseLimit,
    \* @type: Bool;
    GateOpen,
    \* @type: Int;
    Owner,
    \* @type: Int;
    OrdinaryLeader,
    \* @type: Int;
    NumValidators

ASSUME /\ UseCollectiveCoverage \in BOOLEAN
       /\ LeaseLimit \in Nat \ {0}
       /\ GateOpen \in BOOLEAN
       /\ NumValidators \in Nat \ {0}
       /\ Owner \in 1..NumValidators
       /\ OrdinaryLeader \in 1..NumValidators

Validators == 1..NumValidators
Parents == {"left", "right", "cover"}
Latest == {"left", "right"}

Covers(parent) ==
    CASE parent = "left" -> {"left"}
      [] parent = "right" -> {"right"}
      [] OTHER -> Latest

VARIABLES
    \* @type: Bool;
    configured,
    \* @type: Set(Str);
    selectedParents,
    \* @type: Set(Str);
    invalidLatest,
    \* @type: Int;
    leaseAge,
    \* @type: Bool;
    retryPublished,
    \* @type: Int;
    retryPublisher,
    \* @type: Bool;
    ordinaryPublished,
    \* @type: Int;
    ordinaryPublisher

vars ==
    <<configured,
      selectedParents,
      invalidLatest,
      leaseAge,
      retryPublished,
      retryPublisher,
      ordinaryPublished,
      ordinaryPublisher>>

ValidLatest == Latest \ invalidLatest

CollectivelyCovered ==
    \A latest \in ValidLatest :
        \E parent \in selectedParents : latest \in Covers(parent)

SingleParentCovered ==
    \E parent \in selectedParents : ValidLatest \subseteq Covers(parent)

FrontierReady ==
    IF UseCollectiveCoverage THEN CollectivelyCovered ELSE SingleParentCovered

RetryReady == configured /\ GateOpen /\ (FrontierReady \/ leaseAge > LeaseLimit)

Init ==
    /\ configured = FALSE
    /\ selectedParents = {}
    /\ invalidLatest = {}
    /\ leaseAge = 0
    /\ retryPublished = FALSE
    /\ retryPublisher = 0
    /\ ordinaryPublished = FALSE
    /\ ordinaryPublisher = 0

Configure(parents, invalid) ==
    /\ ~configured
    /\ parents \in SUBSET Parents
    /\ invalid \in SUBSET Latest
    /\ configured' = TRUE
    /\ selectedParents' = parents
    /\ invalidLatest' = invalid
    /\ UNCHANGED
       <<leaseAge,
         retryPublished,
         retryPublisher,
         ordinaryPublished,
         ordinaryPublisher>>

TickLease ==
    /\ configured
    /\ ~retryPublished
    /\ leaseAge <= LeaseLimit
    /\ leaseAge' = leaseAge + 1
    /\ UNCHANGED
       <<configured,
         selectedParents,
         invalidLatest,
         retryPublished,
         retryPublisher,
         ordinaryPublished,
         ordinaryPublisher>>

PublishRetry ==
    /\ RetryReady
    /\ ~retryPublished
    /\ retryPublished' = TRUE
    /\ retryPublisher' = Owner
    /\ UNCHANGED
       <<configured,
         selectedParents,
         invalidLatest,
         leaseAge,
         ordinaryPublished,
         ordinaryPublisher>>

PublishOrdinary ==
    /\ configured
    /\ ~ordinaryPublished
    /\ ordinaryPublished' = TRUE
    /\ ordinaryPublisher' = OrdinaryLeader
    /\ UNCHANGED
       <<configured,
         selectedParents,
         invalidLatest,
         leaseAge,
         retryPublished,
         retryPublisher>>

ConfigureSome ==
    \E parents \in SUBSET Parents, invalid \in SUBSET Latest : Configure(parents, invalid)

Next == ConfigureSome \/ TickLease \/ PublishRetry \/ PublishOrdinary

Spec ==
    Init
    /\ [][Next]_vars
    /\ WF_vars(ConfigureSome)
    /\ WF_vars(TickLease)
    /\ WF_vars(PublishRetry)
    /\ WF_vars(PublishOrdinary)

TypeOK ==
    /\ configured \in BOOLEAN
    /\ selectedParents \subseteq Parents
    /\ invalidLatest \subseteq Latest
    /\ leaseAge \in 0..(LeaseLimit + 1)
    /\ retryPublished \in BOOLEAN
    /\ retryPublisher \in 0..NumValidators
    /\ ordinaryPublished \in BOOLEAN
    /\ ordinaryPublisher \in 0..NumValidators

CollectiveCoverageReadiesRetry ==
    configured /\ GateOpen /\ CollectivelyCovered => FrontierReady

IncompleteCoverageDefersBeforeLease ==
    configured /\ ~CollectivelyCovered /\ leaseAge <= LeaseLimit => ~FrontierReady

RetryUsesCarrierOwner == retryPublished => retryPublisher = Owner

RetryRequiresGateAndReadiness ==
    retryPublished => GateOpen /\ (FrontierReady \/ leaseAge > LeaseLimit)

OrdinaryUsesIndependentLeader ==
    ordinaryPublished => ordinaryPublisher = OrdinaryLeader

RecoveryEventuallyPublishes == configured ~> retryPublished

OrdinaryProgressesIndependently == configured ~> ordinaryPublished

=============================================================================

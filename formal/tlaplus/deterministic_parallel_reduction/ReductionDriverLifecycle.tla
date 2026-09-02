---------------- MODULE ReductionDriverLifecycle ----------------
EXTENDS FiniteSets, Naturals, Sequences

CONSTANTS
    \* @type: Bool;
    AtomicSubmitClaim,
    \* @type: Bool;
    InlineWorkReady,
    \* @type: Bool;
    TransferPendingDriver,
    \* @type: Bool;
    BypassInternalSubmission,
    \* @type: Int;
    MaxRounds

P1 == "participant-1"
P2 == "participant-2"
P3 == "participant-3"
Participants == {P1, P2, P3}
Phases == {"running", "waiting", "done"}
Drivers == {"none", "inline", "spawned"}
NoOwner == "none"
Owners == Participants \union {NoOwner}
Ops == [participant : Participants, round : 1..MaxRounds]

\* @type: Str => Int;
ParticipantRank(participant) ==
    CASE participant = P1 -> 1
      [] participant = P2 -> 2
      [] OTHER -> 3

\* @type: {participant: Str, round: Int} => Int;
OpRank(op) == (op.round - 1) * Cardinality(Participants) + ParticipantRank(op.participant)

\* @type: Set({participant: Str, round: Int}) => {participant: Str, round: Int};
CanonicalFirst(ops) ==
    CHOOSE op \in ops : \A other \in ops : OpRank(op) <= OpRank(other)

\* @type: (Str, Set({participant: Str, round: Int}), Set({participant: Str, round: Int}), Set({participant: Str, round: Int})) => Set({participant: Str, round: Int});
OutstandingFor(participant, pendingOps, inFlightOps, awaitingOps) ==
    {op \in pendingOps \union inFlightOps \union awaitingOps :
        op.participant = participant}

\* @type: (Set(Str), Str -> Str, Set({participant: Str, round: Int})) => Bool;
Ready(liveParticipants, participantPhase, pendingOps) ==
    /\ pendingOps /= {}
    /\ \A participant \in liveParticipants : participantPhase[participant] = "waiting"

VARIABLES
    \* @type: Set(Str);
    live,
    \* @type: Str -> Str;
    phase,
    \* @type: Str -> Int;
    nextRound,
    \* @type: Set({participant: Str, round: Int});
    pending,
    \* @type: Set({participant: Str, round: Int});
    inFlight,
    \* @type: Set({participant: Str, round: Int});
    awaitingDelivery,
    \* @type: Set({participant: Str, round: Int});
    delivered,
    \* @type: Set({participant: Str, round: Int});
    cancelled,
    \* @type: Str;
    driver,
    \* @type: Str;
    owner,
    \* @type: Seq({participant: Str, round: Int});
    trace,
    \* @type: Int;
    internalReentries

vars == <<
    live,
    phase,
    nextRound,
    pending,
    inFlight,
    awaitingDelivery,
    delivered,
    cancelled,
    driver,
    owner,
    trace,
    internalReentries
>>

Init ==
    /\ live = Participants
    /\ phase = [participant \in Participants |-> "running"]
    /\ nextRound = [participant \in Participants |-> 1]
    /\ pending = {}
    /\ inFlight = {}
    /\ awaitingDelivery = {}
    /\ delivered = {}
    /\ cancelled = {}
    /\ driver = "none"
    /\ owner = NoOwner
    /\ trace = <<>>
    /\ internalReentries = 0

Submit(participant) ==
    /\ driver /= "inline"
    /\ participant \in live
    /\ phase[participant] = "running"
    /\ nextRound[participant] <= MaxRounds
    /\ LET op == [participant |-> participant, round |-> nextRound[participant]]
           nextPhase == [phase EXCEPT ![participant] = "waiting"]
           nextPending == pending \union {op}
           claim ==
               AtomicSubmitClaim /\ driver = "none" /\ Ready(live, nextPhase, nextPending)
       IN /\ phase' = nextPhase
          /\ nextRound' = [nextRound EXCEPT ![participant] = @ + 1]
          /\ IF claim
                THEN /\ pending' = {}
                     /\ inFlight' = nextPending
                     /\ driver' = "inline"
                     /\ owner' = participant
                ELSE /\ pending' = nextPending
                     /\ UNCHANGED <<inFlight, driver, owner>>
    /\ UNCHANGED <<live, awaitingDelivery, delivered, cancelled, trace,
                    internalReentries>>

Complete(participant) ==
    /\ driver /= "inline"
    /\ participant \in live
    /\ phase[participant] = "running"
    /\ LET nextLive == live \ {participant}
           nextPhase == [phase EXCEPT ![participant] = "done"]
           claim == driver = "none" /\ Ready(nextLive, nextPhase, pending)
       IN /\ live' = nextLive
          /\ phase' = nextPhase
          /\ IF claim
                THEN /\ driver' = "spawned"
                     /\ owner' = NoOwner
                ELSE UNCHANGED <<driver, owner>>
    /\ UNCHANGED <<nextRound, pending, inFlight, awaitingDelivery, delivered,
                    cancelled, trace, internalReentries>>

CancelWaiting(participant) ==
    /\ driver /= "inline"
    /\ participant \in live
    /\ phase[participant] = "waiting"
    /\ LET cancelledPending == {op \in pending : op.participant = participant}
           nextPending == pending \ cancelledPending
           nextLive == live \ {participant}
           nextPhase == [phase EXCEPT ![participant] = "done"]
           claim == driver = "none" /\ Ready(nextLive, nextPhase, nextPending)
       IN /\ live' = nextLive
          /\ phase' = nextPhase
          /\ pending' = nextPending
          /\ cancelled' = cancelled \union cancelledPending
          /\ IF claim
                THEN /\ driver' = "spawned"
                     /\ owner' = NoOwner
                ELSE UNCHANGED <<driver, owner>>
    /\ UNCHANGED <<nextRound, inFlight, awaitingDelivery, delivered, trace,
                    internalReentries>>

TransferInline ==
    /\ driver = "inline"
    /\ inFlight /= {}
    /\ ~InlineWorkReady
    /\ TransferPendingDriver
    /\ driver' = "spawned"
    /\ owner' = NoOwner
    /\ UNCHANGED <<live, phase, nextRound, pending, inFlight,
                    awaitingDelivery, delivered, cancelled, trace,
                    internalReentries>>

InlineCommit ==
    /\ driver = "inline"
    /\ inFlight /= {}
    /\ InlineWorkReady
    /\ LET op == CanonicalFirst(inFlight)
       IN /\ inFlight' = inFlight \ {op}
          /\ awaitingDelivery' = awaitingDelivery \union {op}
          /\ delivered' = delivered \union {op}
          /\ trace' = Append(trace, op)
          /\ internalReentries' =
              internalReentries + IF BypassInternalSubmission THEN 0 ELSE 1
    /\ UNCHANGED <<live, phase, nextRound, pending, cancelled, driver, owner>>

FinishInline ==
    /\ driver = "inline"
    /\ inFlight = {}
    /\ driver' = "none"
    /\ owner' = NoOwner
    /\ phase' =
        [participant \in Participants |->
            IF participant \in live /\
               \E op \in awaitingDelivery : op.participant = participant
            THEN "running"
            ELSE phase[participant]]
    /\ awaitingDelivery' = {}
    /\ UNCHANGED <<live, nextRound, pending, inFlight, delivered, cancelled,
                    trace, internalReentries>>

StartSpawnedBatch ==
    /\ driver = "spawned"
    /\ inFlight = {}
    /\ awaitingDelivery = {}
    /\ Ready(live, phase, pending)
    /\ inFlight' = pending
    /\ pending' = {}
    /\ UNCHANGED <<live, phase, nextRound, awaitingDelivery, delivered,
                    cancelled, driver, owner, trace, internalReentries>>

SpawnedCommit ==
    /\ driver = "spawned"
    /\ inFlight /= {}
    /\ LET op == CanonicalFirst(inFlight)
       IN /\ inFlight' = inFlight \ {op}
          /\ awaitingDelivery' = awaitingDelivery \union {op}
          /\ delivered' = delivered \union {op}
          /\ trace' = Append(trace, op)
          /\ internalReentries' =
              internalReentries + IF BypassInternalSubmission THEN 0 ELSE 1
    /\ UNCHANGED <<live, phase, nextRound, pending, cancelled, driver, owner>>

FinishSpawned ==
    /\ driver = "spawned"
    /\ inFlight = {}
    /\ (awaitingDelivery /= {} \/ pending = {})
    /\ driver' = "none"
    /\ owner' = NoOwner
    /\ phase' =
        [participant \in Participants |->
            IF participant \in live /\
               \E op \in awaitingDelivery : op.participant = participant
            THEN "running"
            ELSE phase[participant]]
    /\ awaitingDelivery' = {}
    /\ UNCHANGED <<live, nextRound, pending, inFlight, delivered, cancelled,
                    trace, internalReentries>>

ParticipantProgress ==
    \E participant \in Participants :
        Submit(participant) \/ Complete(participant) \/ CancelWaiting(participant)

DriverProgress ==
    TransferInline \/ InlineCommit \/ FinishInline \/
    StartSpawnedBatch \/ SpawnedCommit \/ FinishSpawned

Next == ParticipantProgress \/ DriverProgress

Spec == Init /\ [][Next]_vars

LiveSpec ==
    Spec /\ WF_vars(ParticipantProgress) /\ WF_vars(DriverProgress)

TypeOK ==
    /\ live \subseteq Participants
    /\ phase \in [Participants -> Phases]
    /\ nextRound \in [Participants -> 1..(MaxRounds + 1)]
    /\ pending \subseteq Ops
    /\ inFlight \subseteq Ops
    /\ awaitingDelivery \subseteq Ops
    /\ delivered \subseteq Ops
    /\ cancelled \subseteq Ops
    /\ driver \in Drivers
    /\ owner \in Owners
    /\ Len(trace) <= Cardinality(Ops)
    /\ internalReentries \in 0..Cardinality(Ops)

Inv_ExactlyOneDriverOwner ==
    /\ (driver = "inline") = (owner \in Participants)
    /\ (driver /= "inline") = (owner = NoOwner)

Inv_ReadyHasDriver ==
    Ready(live, phase, pending) => driver /= "none"

Inv_InlineBatchIsFrozen ==
    driver = "inline" => pending = {}

Inv_PendingInlineIsTransferable ==
    driver = "inline" /\ inFlight /= {} => InlineWorkReady \/ TransferPendingDriver

Inv_OperationConservation ==
    LET submitted ==
        {op \in Ops : op.round < nextRound[op.participant]}
    IN /\ pending \intersect inFlight = {}
       /\ pending \intersect awaitingDelivery = {}
       /\ pending \intersect delivered = {}
       /\ pending \intersect cancelled = {}
       /\ inFlight \intersect awaitingDelivery = {}
       /\ inFlight \intersect delivered = {}
       /\ inFlight \intersect cancelled = {}
       /\ awaitingDelivery \subseteq delivered
       /\ awaitingDelivery \intersect cancelled = {}
       /\ delivered \intersect cancelled = {}
       /\ pending \union inFlight \union delivered \union cancelled = submitted

Inv_WaiterOwnsOneOperation ==
    \A participant \in live :
        phase[participant] = "waiting" =>
            Cardinality(
                OutstandingFor(participant, pending, inFlight, awaitingDelivery)) = 1

Inv_RunnerOwnsNoOperation ==
    \A participant \in live :
        phase[participant] = "running" =>
            OutstandingFor(participant, pending, inFlight, awaitingDelivery) = {}

Inv_TraceIsExact ==
    Len(trace) = Cardinality(delivered)

Inv_InternalExecutionNeverResubmits ==
    internalReentries = 0

Inv_PerParticipantOrder ==
    \A op \in delivered :
        op.round > 1 =>
            [participant |-> op.participant, round |-> op.round - 1] \in delivered

Inv_IncompleteFrontierRetainsConcurrency ==
    driver = "none" /\ pending /= {} =>
        \E participant \in live : phase[participant] = "running"

Quiescent ==
    /\ live = {}
    /\ pending = {}
    /\ inFlight = {}
    /\ awaitingDelivery = {}
    /\ driver = "none"

EventuallyQuiescent == <>Quiescent

=============================================================================

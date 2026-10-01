-------------------- MODULE VaultBackedByteAccounting --------------------
EXTENDS Integers, FiniteSets

CONSTANTS
    \* @type: Set(Str);
    Events,
    \* @type: Set(Str);
    Introductions,
    \* @type: Set(Str);
    Communications,
    \* @type: Set(Str);
    PersistentIntroductions,
    \* @type: Str -> Int;
    ExecutionUnits,
    \* @type: Str -> Int;
    PhysicalAuthorityUnits,
    \* @type: Str -> Int;
    IntroductionBytes,
    \* @type: Str -> Int;
    TransferBytes,
    \* @type: Str -> Int;
    TraceBytes,
    \* @type: Str -> Set(Str);
    Dependencies,
    \* @type: Str -> Set(Str);
    Removals,
    \* @type: Int;
    ByteRate,
    \* @type: Int;
    ReservationBound,
    \* @type: Int;
    InitialPurse,
    \* @type: Int;
    InitialSender,
    \* @type: Int;
    TopUpAmount,
    \* @type: Int;
    MaxU64,
    \* @type: Str;
    Defect,
    \* @type: Str;
    NoEvent,
    \* @type: Str;
    PersistentEvent,
    \* @type: Str;
    RepeatEvent,
    \* @type: Str;
    JoinEvent,
    \* @type: Int;
    JoinOmittedBytes,
    \* @type: Str;
    introduceProducer,
    \* @type: Str;
    introduceConsumer,
    \* @type: Str;
    binaryComm,
    \* @type: Str;
    joinComm,
    \* @type: Str;
    repeatComm

ASSUME Events # {}
ASSUME Introductions \subseteq Events
ASSUME Communications \subseteq Events
ASSUME Introductions \cap Communications = {}
ASSUME Events = Introductions \cup Communications
ASSUME PersistentIntroductions \subseteq Introductions
ASSUME ExecutionUnits \in [Events -> Nat]
ASSUME PhysicalAuthorityUnits \in [Events -> Nat]
ASSUME IntroductionBytes \in [Events -> Nat]
ASSUME TransferBytes \in [Events -> Nat]
ASSUME TraceBytes \in [Events -> Nat]
ASSUME Dependencies \in [Events -> SUBSET Events]
ASSUME Removals \in [Events -> SUBSET Introductions]
ASSUME \A event \in Events : event \notin Dependencies[event]
ASSUME \A event \in Introductions : ExecutionUnits[event] = 0
ASSUME \A event \in Communications : ExecutionUnits[event] = 1
ASSUME \A event \in Introductions : PhysicalAuthorityUnits[event] = 0
ASSUME \A event \in Communications : PhysicalAuthorityUnits[event] >= 1
ASSUME \A event \in Introductions : TransferBytes[event] = 0 /\ TraceBytes[event] = 0
ASSUME \A event \in Communications : IntroductionBytes[event] = 0
ASSUME \A event \in Events : Removals[event] \cap PersistentIntroductions = {}
ASSUME ByteRate \in Nat \ {0}
ASSUME ReservationBound \in Nat
ASSUME InitialPurse \in Nat
ASSUME InitialSender \in Nat
ASSUME TopUpAmount \in Nat
ASSUME InitialPurse >= ReservationBound
ASSUME InitialSender >= TopUpAmount
ASSUME MaxU64 \in Nat
ASSUME ReservationBound <= MaxU64
ASSUME NoEvent \notin Events
ASSUME PersistentEvent \in PersistentIntroductions
ASSUME RepeatEvent \in Communications
ASSUME JoinEvent \in Communications
ASSUME JoinOmittedBytes \in Nat
ASSUME JoinOmittedBytes <= TransferBytes[JoinEvent]
ASSUME Defect \in {
    "None",
    "ChargeAfterMutation",
    "TriggerDependent",
    "OmitJoinParticipant",
    "RechargePersistent",
    "PeekCredit",
    "ReplayOmission",
    "TopUpExpandsBound",
    "OverflowWrap"
}

CanonicalByteCharge(event) ==
    ByteRate * (IntroductionBytes[event] + TransferBytes[event] + TraceBytes[event])

CanonicalProcessedCharge(event) == ExecutionUnits[event] + CanonicalByteCharge(event)

CanonicalSettlementCharge(event) ==
    PhysicalAuthorityUnits[event] + CanonicalByteCharge(event)

CanonicalProcessedChargeSum(events) ==
    (IF introduceProducer \in events THEN CanonicalProcessedCharge(introduceProducer) ELSE 0)
    + (IF introduceConsumer \in events THEN CanonicalProcessedCharge(introduceConsumer) ELSE 0)
    + (IF binaryComm \in events THEN CanonicalProcessedCharge(binaryComm) ELSE 0)
    + (IF joinComm \in events THEN CanonicalProcessedCharge(joinComm) ELSE 0)
    + (IF repeatComm \in events THEN CanonicalProcessedCharge(repeatComm) ELSE 0)

CanonicalSettlementChargeSum(events) ==
    (IF introduceProducer \in events THEN CanonicalSettlementCharge(introduceProducer) ELSE 0)
    + (IF introduceConsumer \in events THEN CanonicalSettlementCharge(introduceConsumer) ELSE 0)
    + (IF binaryComm \in events THEN CanonicalSettlementCharge(binaryComm) ELSE 0)
    + (IF joinComm \in events THEN CanonicalSettlementCharge(joinComm) ELSE 0)
    + (IF repeatComm \in events THEN CanonicalSettlementCharge(repeatComm) ELSE 0)

CanonicalByteSum(events) ==
    (IF introduceProducer \in events THEN CanonicalByteCharge(introduceProducer) ELSE 0)
    + (IF introduceConsumer \in events THEN CanonicalByteCharge(introduceConsumer) ELSE 0)
    + (IF binaryComm \in events THEN CanonicalByteCharge(binaryComm) ELSE 0)
    + (IF joinComm \in events THEN CanonicalByteCharge(joinComm) ELSE 0)
    + (IF repeatComm \in events THEN CanonicalByteCharge(repeatComm) ELSE 0)

ActualByteCharge(event, committed) ==
    CASE Defect = "TriggerDependent"
              /\ event \in Introductions
              /\ committed \cap Introductions # {}
         -> 0
      [] Defect = "OmitJoinParticipant" /\ event = JoinEvent
         -> CanonicalByteCharge(event) - ByteRate * JoinOmittedBytes
      [] Defect = "RechargePersistent" /\ event = RepeatEvent
         -> CanonicalByteCharge(event) + ByteRate * IntroductionBytes[PersistentEvent]
      [] Defect = "OverflowWrap" /\ event = JoinEvent
         -> 0
      [] OTHER -> CanonicalByteCharge(event)

ReplayByteCharge(event) ==
    IF Defect = "ReplayOmission" /\ event = JoinEvent
    THEN CanonicalByteCharge(event) - ByteRate * TraceBytes[event]
    ELSE CanonicalByteCharge(event)

VARIABLES
    \* @type: Str;
    phase,
    \* @type: Int;
    purseLiquid,
    \* @type: Int;
    senderLiquid,
    \* @type: Int;
    held,
    \* @type: Int;
    reservationSnapshot,
    \* @type: Int;
    burned,
    \* @type: Set(Str);
    processed,
    \* @type: Set(Str);
    stored,
    \* @type: Int;
    executionSpent,
    \* @type: Int;
    physicalSpent,
    \* @type: Int;
    byteSpent,
    \* @type: Int;
    processedCost,
    \* @type: Int;
    spent,
    \* @type: Int;
    stateVersion,
    \* @type: Bool;
    oversizedAttempted,
    \* @type: Int;
    rejectionBeforeVersion,
    \* @type: Int;
    rejectionAfterVersion,
    \* @type: Bool;
    peeked,
    \* @type: Int;
    removalCredit,
    \* @type: Bool;
    topUpDone,
    \* @type: Set(Str);
    replayProcessed,
    \* @type: Set(Str);
    replayStored,
    \* @type: Int;
    replayExecutionSpent,
    \* @type: Int;
    replayPhysicalSpent,
    \* @type: Int;
    replayByteSpent,
    \* @type: Int;
    replayProcessedCost,
    \* @type: Int;
    replaySpent,
    \* @type: Int;
    replayStateVersion

vars == <<
    phase,
    purseLiquid,
    senderLiquid,
    held,
    reservationSnapshot,
    burned,
    processed,
    stored,
    executionSpent,
    physicalSpent,
    byteSpent,
    processedCost,
    spent,
    stateVersion,
    oversizedAttempted,
    rejectionBeforeVersion,
    rejectionAfterVersion,
    peeked,
    removalCredit,
    topUpDone,
    replayProcessed,
    replayStored,
    replayExecutionSpent,
    replayPhysicalSpent,
    replayByteSpent,
    replayProcessedCost,
    replaySpent,
    replayStateVersion
>>

Init ==
    /\ phase = "admission"
    /\ purseLiquid = InitialPurse
    /\ senderLiquid = InitialSender
    /\ held = 0
    /\ reservationSnapshot = 0
    /\ burned = 0
    /\ processed = {}
    /\ stored = {}
    /\ executionSpent = 0
    /\ physicalSpent = 0
    /\ byteSpent = 0
    /\ processedCost = 0
    /\ spent = 0
    /\ stateVersion = 0
    /\ oversizedAttempted = FALSE
    /\ rejectionBeforeVersion = 0
    /\ rejectionAfterVersion = 0
    /\ peeked = FALSE
    /\ removalCredit = 0
    /\ topUpDone = FALSE
    /\ replayProcessed = {}
    /\ replayStored = {}
    /\ replayExecutionSpent = 0
    /\ replayPhysicalSpent = 0
    /\ replayByteSpent = 0
    /\ replayProcessedCost = 0
    /\ replaySpent = 0
    /\ replayStateVersion = 0

Reserve ==
    /\ phase = "admission"
    /\ purseLiquid >= ReservationBound
    /\ purseLiquid' = purseLiquid - ReservationBound
    /\ held' = ReservationBound
    /\ reservationSnapshot' = ReservationBound
    /\ phase' = "execution"
    /\ UNCHANGED <<
        senderLiquid, burned, processed, stored, executionSpent, physicalSpent,
        byteSpent, processedCost, spent, stateVersion, oversizedAttempted, rejectionBeforeVersion,
        rejectionAfterVersion, peeked, removalCredit, topUpDone,
        replayProcessed, replayStored, replayExecutionSpent, replayPhysicalSpent,
        replayByteSpent, replayProcessedCost, replaySpent, replayStateVersion
        >>

Ready(event) ==
    /\ event \in Events \ processed
    /\ Dependencies[event] \subseteq processed

NextStored(event, current) ==
    (current \ Removals[event])
        \cup IF event \in Introductions THEN {event} ELSE {}

Execute(event) ==
    LET bytes == ActualByteCharge(event, processed)
        processedCharge == ExecutionUnits[event] + bytes
        settlementCharge == PhysicalAuthorityUnits[event] + bytes
    IN /\ phase = "execution"
       /\ Ready(event)
       /\ processedCharge <= MaxU64
       /\ settlementCharge <= MaxU64
       /\ processedCost + processedCharge <= reservationSnapshot
       /\ spent + settlementCharge <= reservationSnapshot
       /\ processed' = processed \cup {event}
       /\ stored' = NextStored(event, stored)
       /\ executionSpent' = executionSpent + ExecutionUnits[event]
       /\ physicalSpent' = physicalSpent + PhysicalAuthorityUnits[event]
       /\ byteSpent' = byteSpent + bytes
       /\ processedCost' = processedCost + processedCharge
       /\ spent' = spent + settlementCharge
       /\ stateVersion' = stateVersion + 1
       /\ UNCHANGED <<
            phase, purseLiquid, senderLiquid, held, reservationSnapshot,
            burned, oversizedAttempted, rejectionBeforeVersion,
            rejectionAfterVersion, peeked, removalCredit, topUpDone,
            replayProcessed, replayStored, replayExecutionSpent,
            replayPhysicalSpent, replayByteSpent, replayProcessedCost,
            replaySpent, replayStateVersion
          >>

AttemptOversized ==
    /\ phase = "execution"
    /\ ~oversizedAttempted
    /\ oversizedAttempted' = TRUE
    /\ rejectionBeforeVersion' = stateVersion
    /\ IF Defect = "ChargeAfterMutation"
       THEN /\ stateVersion' = stateVersion + 1
            /\ rejectionAfterVersion' = stateVersion + 1
       ELSE /\ UNCHANGED stateVersion
            /\ rejectionAfterVersion' = stateVersion
    /\ UNCHANGED <<
        phase, purseLiquid, senderLiquid, held, reservationSnapshot,
        burned, processed, stored, executionSpent, physicalSpent, byteSpent,
        processedCost, spent,
        peeked, removalCredit, topUpDone, replayProcessed, replayStored,
        replayExecutionSpent, replayPhysicalSpent, replayByteSpent,
        replayProcessedCost, replaySpent, replayStateVersion
        >>

Peek ==
    /\ phase = "execution"
    /\ ~peeked
    /\ peeked' = TRUE
    /\ IF Defect = "PeekCredit" /\ spent > 0
       THEN /\ spent' = spent - 1
            /\ removalCredit' = 1
       ELSE /\ UNCHANGED spent
            /\ removalCredit' = 0
    /\ UNCHANGED <<
        phase, purseLiquid, senderLiquid, held, reservationSnapshot,
        burned, processed, stored, executionSpent, physicalSpent, byteSpent,
        processedCost, stateVersion,
        oversizedAttempted, rejectionBeforeVersion, rejectionAfterVersion,
        topUpDone, replayProcessed, replayStored, replayExecutionSpent,
        replayPhysicalSpent, replayByteSpent, replayProcessedCost,
        replaySpent, replayStateVersion
        >>

TopUp ==
    /\ phase \in {"execution", "replay"}
    /\ ~topUpDone
    /\ senderLiquid >= TopUpAmount
    /\ topUpDone' = TRUE
    /\ senderLiquid' = senderLiquid - TopUpAmount
    /\ IF Defect = "TopUpExpandsBound"
       THEN /\ held' = held + TopUpAmount
            /\ reservationSnapshot' = reservationSnapshot + TopUpAmount
            /\ UNCHANGED purseLiquid
       ELSE /\ purseLiquid' = purseLiquid + TopUpAmount
            /\ UNCHANGED <<held, reservationSnapshot>>
    /\ UNCHANGED <<
        phase, burned, processed, stored, executionSpent, physicalSpent,
        byteSpent, processedCost, spent,
        stateVersion, oversizedAttempted, rejectionBeforeVersion,
        rejectionAfterVersion, peeked, removalCredit, replayProcessed,
        replayStored, replayExecutionSpent, replayPhysicalSpent,
        replayByteSpent, replayProcessedCost, replaySpent, replayStateVersion
        >>

Settle ==
    /\ phase = "execution"
    /\ processed = Events
    /\ oversizedAttempted
    /\ held = reservationSnapshot
    /\ spent <= reservationSnapshot
    /\ purseLiquid' = purseLiquid + (reservationSnapshot - spent)
    /\ held' = 0
    /\ burned' = burned + spent
    /\ phase' = "replay"
    /\ UNCHANGED <<
        senderLiquid, reservationSnapshot, processed, stored, executionSpent,
        physicalSpent, byteSpent, processedCost, spent, stateVersion, oversizedAttempted,
        rejectionBeforeVersion, rejectionAfterVersion, peeked, removalCredit,
        topUpDone, replayProcessed, replayStored, replayExecutionSpent,
        replayPhysicalSpent, replayByteSpent, replayProcessedCost,
        replaySpent, replayStateVersion
        >>

ReplayReady(event) ==
    /\ event \in processed \ replayProcessed
    /\ Dependencies[event] \subseteq replayProcessed

Replay(event) ==
    LET bytes == ReplayByteCharge(event)
        processedCharge == ExecutionUnits[event] + bytes
        settlementCharge == PhysicalAuthorityUnits[event] + bytes
    IN /\ phase = "replay"
       /\ ReplayReady(event)
       /\ replayProcessed' = replayProcessed \cup {event}
       /\ replayStored' = NextStored(event, replayStored)
       /\ replayExecutionSpent' = replayExecutionSpent + ExecutionUnits[event]
       /\ replayPhysicalSpent' = replayPhysicalSpent + PhysicalAuthorityUnits[event]
       /\ replayByteSpent' = replayByteSpent + bytes
       /\ replayProcessedCost' = replayProcessedCost + processedCharge
       /\ replaySpent' = replaySpent + settlementCharge
       /\ replayStateVersion' = replayStateVersion + 1
       /\ UNCHANGED <<
            phase, purseLiquid, senderLiquid, held, reservationSnapshot,
            burned, processed, stored, executionSpent, physicalSpent, byteSpent,
            processedCost, spent,
            stateVersion, oversizedAttempted, rejectionBeforeVersion,
            rejectionAfterVersion, peeked, removalCredit, topUpDone
          >>

Finish ==
    /\ phase = "replay"
    /\ replayProcessed = processed
    /\ phase' = "done"
    /\ UNCHANGED <<
        purseLiquid, senderLiquid, held, reservationSnapshot, burned,
        processed, stored, executionSpent, physicalSpent, byteSpent,
        processedCost, spent, stateVersion,
        oversizedAttempted, rejectionBeforeVersion, rejectionAfterVersion,
        peeked, removalCredit, topUpDone, replayProcessed, replayStored,
        replayExecutionSpent, replayPhysicalSpent, replayByteSpent,
        replayProcessedCost, replaySpent, replayStateVersion
        >>

Next ==
    \/ Reserve
    \/ \E event \in Events : Execute(event)
    \/ AttemptOversized
    \/ Peek
    \/ TopUp
    \/ Settle
    \/ \E event \in Events : Replay(event)
    \/ Finish

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(Reserve)
    /\ \A event \in Events : WF_vars(Execute(event))
    /\ WF_vars(AttemptOversized)
    /\ WF_vars(Settle)
    /\ \A event \in Events : WF_vars(Replay(event))
    /\ WF_vars(Finish)

TypeOK ==
    /\ phase \in {"admission", "execution", "replay", "done"}
    /\ purseLiquid \in Nat
    /\ senderLiquid \in Nat
    /\ held \in Nat
    /\ reservationSnapshot \in Nat
    /\ burned \in Nat
    /\ processed \subseteq Events
    /\ stored \subseteq Introductions
    /\ executionSpent \in Nat
    /\ physicalSpent \in Nat
    /\ byteSpent \in Nat
    /\ processedCost \in Nat
    /\ spent \in Nat
    /\ stateVersion \in Nat
    /\ oversizedAttempted \in BOOLEAN
    /\ rejectionBeforeVersion \in Nat
    /\ rejectionAfterVersion \in Nat
    /\ peeked \in BOOLEAN
    /\ removalCredit \in Nat
    /\ topUpDone \in BOOLEAN
    /\ replayProcessed \subseteq Events
    /\ replayStored \subseteq Introductions
    /\ replayExecutionSpent \in Nat
    /\ replayPhysicalSpent \in Nat
    /\ replayByteSpent \in Nat
    /\ replayProcessedCost \in Nat
    /\ replaySpent \in Nat
    /\ replayStateVersion \in Nat

OneExecutionUnitPerCommittedComm ==
    executionSpent = Cardinality(processed \cap Communications)

IntroductionsConsumeNoExecutionUnit ==
    processed \subseteq Introductions => executionSpent = 0

PhysicalAuthorityIsTrackedSeparately ==
    physicalSpent =
      (IF introduceProducer \in processed THEN PhysicalAuthorityUnits[introduceProducer] ELSE 0)
      + (IF introduceConsumer \in processed THEN PhysicalAuthorityUnits[introduceConsumer] ELSE 0)
      + (IF binaryComm \in processed THEN PhysicalAuthorityUnits[binaryComm] ELSE 0)
      + (IF joinComm \in processed THEN PhysicalAuthorityUnits[joinComm] ELSE 0)
      + (IF repeatComm \in processed THEN PhysicalAuthorityUnits[repeatComm] ELSE 0)

ExactCanonicalDebit ==
    /\ byteSpent = CanonicalByteSum(processed)
    /\ processedCost = CanonicalProcessedChargeSum(processed)
    /\ spent = CanonicalSettlementChargeSum(processed)

HardReservationCeiling ==
    /\ spent <= reservationSnapshot
    /\ reservationSnapshot <= MaxU64

RejectedAttemptIsAtomic ==
    oversizedAttempted => rejectionAfterVersion = rejectionBeforeVersion

NoRemovalCredit == removalCredit = 0

ReservationSnapshotImmutable ==
    phase # "admission" => reservationSnapshot = ReservationBound

TopUpCannotFundRunningDeploy ==
    topUpDone => reservationSnapshot = ReservationBound

CanonicalValueConserved ==
    purseLiquid + senderLiquid + held + burned = InitialPurse + InitialSender

PersistentIntroductionSurvivesDelivery ==
    RepeatEvent \in processed => PersistentEvent \in stored

ReplayPrefixExact ==
    /\ replayExecutionSpent = Cardinality(replayProcessed \cap Communications)
    /\ replayPhysicalSpent =
      (IF introduceProducer \in replayProcessed THEN PhysicalAuthorityUnits[introduceProducer] ELSE 0)
      + (IF introduceConsumer \in replayProcessed THEN PhysicalAuthorityUnits[introduceConsumer] ELSE 0)
      + (IF binaryComm \in replayProcessed THEN PhysicalAuthorityUnits[binaryComm] ELSE 0)
      + (IF joinComm \in replayProcessed THEN PhysicalAuthorityUnits[joinComm] ELSE 0)
      + (IF repeatComm \in replayProcessed THEN PhysicalAuthorityUnits[repeatComm] ELSE 0)
    /\ replayByteSpent = CanonicalByteSum(replayProcessed)
    /\ replayProcessedCost = CanonicalProcessedChargeSum(replayProcessed)
    /\ replaySpent = CanonicalSettlementChargeSum(replayProcessed)

ReplayMatchesPlay ==
    phase = "done" =>
        /\ replayProcessed = processed
        /\ replayStored = stored
        /\ replayExecutionSpent = executionSpent
        /\ replayPhysicalSpent = physicalSpent
        /\ replayByteSpent = byteSpent
        /\ replayProcessedCost = processedCost
        /\ replaySpent = spent
        /\ replayStateVersion = stateVersion

AllCommittedWorkIsFunded ==
    processed \subseteq Events /\ spent <= reservationSnapshot

EventuallyDone == <>(phase = "done")

EventsDef == {introduceProducer, introduceConsumer, binaryComm, joinComm, repeatComm}
IntroductionsDef == {introduceProducer, introduceConsumer}
CommunicationsDef == {binaryComm, joinComm, repeatComm}
PersistentIntroductionsDef == {introduceProducer}
ExecutionUnitsDef ==
    [event \in EventsDef |-> IF event \in CommunicationsDef THEN 1 ELSE 0]
PhysicalAuthorityUnitsDef ==
    [event \in EventsDef |->
        CASE event = joinComm -> 2
          [] event \in CommunicationsDef -> 1
          [] OTHER -> 0]
IntroductionBytesDef ==
    [event \in EventsDef |->
        CASE event = introduceProducer -> 2
          [] event = introduceConsumer -> 3
          [] OTHER -> 0]
TransferBytesDef ==
    [event \in EventsDef |->
        CASE event = binaryComm -> 4
          [] event = joinComm -> 7
          [] event = repeatComm -> 4
          [] OTHER -> 0]
TraceBytesDef ==
    [event \in EventsDef |-> IF event \in CommunicationsDef THEN 2 ELSE 0]
DependenciesDef ==
    [event \in EventsDef |->
        CASE event = binaryComm -> {introduceProducer, introduceConsumer}
          [] event = joinComm -> {introduceProducer, introduceConsumer}
          [] event = repeatComm -> {joinComm}
          [] OTHER -> {}]
RemovalsDef ==
    [event \in EventsDef |->
        IF event = binaryComm THEN {introduceConsumer} ELSE {}]

=============================================================================

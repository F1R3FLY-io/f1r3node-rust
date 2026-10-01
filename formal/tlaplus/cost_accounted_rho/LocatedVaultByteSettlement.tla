-------------------- MODULE LocatedVaultByteSettlement --------------------
EXTENDS Integers, FiniteSets, Sequences

CONSTANTS
    \* @type: Set(Str);
    Purses,
    \* @type: Set(Str);
    Events,
    \* @type: Str -> Set(Str);
    EventPurses,
    \* @type: Str -> Int;
    EventAmount,
    \* @type: Str -> Set(Str);
    Dependencies,
    \* @type: Str -> Int;
    ReservationBound,
    \* @type: Str -> Int;
    InitialLiquid,
    \* @type: Int;
    InitialSender,
    \* @type: Str;
    TopUpTarget,
    \* @type: Int;
    TopUpAmount,
    \* @type: Str;
    ProbePurse,
    \* @type: Str;
    RescuePurse,
    \* @type: Int;
    ProbeAmount,
    \* @type: Int;
    MaxEvents,
    \* @type: Str;
    EnvelopePurse,
    \* @type: Str;
    OuterPurse,
    \* @type: Str;
    ContinuationPurse,
    \* @type: Str;
    CompoundEvent,
    \* @type: Str;
    Defect,
    \* @type: Str;
    outerPurse,
    \* @type: Str;
    continuationPurse,
    \* @type: Str;
    envelopePurse,
    \* @type: Str;
    outerByte,
    \* @type: Str;
    continuationByte,
    \* @type: Str;
    compoundByte

ASSUME Purses = {OuterPurse, ContinuationPurse, EnvelopePurse}
ASSUME OuterPurse # ContinuationPurse
ASSUME OuterPurse # EnvelopePurse
ASSUME ContinuationPurse # EnvelopePurse
ASSUME Events # {}
ASSUME EventPurses \in [Events -> SUBSET Purses]
ASSUME EventAmount \in [Events -> Nat \ {0}]
ASSUME Dependencies \in [Events -> SUBSET Events]
ASSUME \A event \in Events : event \notin Dependencies[event]
ASSUME ReservationBound \in [Purses -> Nat]
ASSUME InitialLiquid \in [Purses -> Nat]
ASSUME \A purse \in Purses : InitialLiquid[purse] >= ReservationBound[purse]
ASSUME InitialSender \in Nat
ASSUME TopUpTarget \in Purses
ASSUME TopUpAmount \in Nat
ASSUME InitialSender >= TopUpAmount
ASSUME ProbePurse \in Purses
ASSUME RescuePurse \in Purses
ASSUME ProbePurse # RescuePurse
ASSUME ProbeAmount \in Nat \ {0}
ASSUME MaxEvents \in Nat \ {0}
ASSUME Cardinality(Events) = MaxEvents
ASSUME ProbeAmount > ReservationBound[ProbePurse]
ASSUME ProbeAmount <= ReservationBound[RescuePurse]
ASSUME EnvelopePurse \in Purses
ASSUME OuterPurse \in Purses
ASSUME ContinuationPurse \in Purses
ASSUME CompoundEvent \in Events
ASSUME {OuterPurse, ContinuationPurse} \subseteq EventPurses[CompoundEvent]
ASSUME Defect \in {
    "None",
    "EnvelopeCollapse",
    "CrossPurseRescue",
    "TopUpExpandsReservation",
    "ReplayUsesEnvelope"
}

ZeroVector == [purse \in Purses |-> 0]

CanonicalDebit(event, purse) ==
    IF purse \in EventPurses[event] THEN EventAmount[event] ELSE 0

ExecutionDebit(event, purse) ==
    IF Defect = "EnvelopeCollapse"
    THEN IF purse = EnvelopePurse THEN EventAmount[event] ELSE 0
    ELSE CanonicalDebit(event, purse)

ReplayDebit(event, purse) ==
    IF Defect = "ReplayUsesEnvelope"
    THEN IF purse = EnvelopePurse THEN EventAmount[event] ELSE 0
    ELSE CanonicalDebit(event, purse)

CanonicalDebitSum(processedEvents, purse) ==
    (IF outerByte \in processedEvents THEN CanonicalDebit(outerByte, purse) ELSE 0)
    + (IF continuationByte \in processedEvents
       THEN CanonicalDebit(continuationByte, purse)
       ELSE 0)
    + (IF compoundByte \in processedEvents
       THEN CanonicalDebit(compoundByte, purse)
       ELSE 0)

SequenceSet(sequence) ==
    {sequence[index] : index \in {candidate \in 1..MaxEvents : candidate <= Len(sequence)}}

PurseTotal(vector) ==
    vector[OuterPurse] + vector[ContinuationPurse] + vector[EnvelopePurse]

VARIABLES
    \* @type: Str;
    phase,
    \* @type: Str -> Int;
    liquid,
    \* @type: Str -> Int;
    held,
    \* @type: Str -> Int;
    reservationSnapshot,
    \* @type: Str -> Int;
    spent,
    \* @type: Str -> Int;
    burned,
    \* @type: Int;
    senderLiquid,
    \* @type: Set(Str);
    processed,
    \* @type: Seq(Str);
    trace,
    \* @type: Bool;
    topUpDone,
    \* @type: Bool;
    probeAttempted,
    \* @type: Bool;
    probeAccepted,
    \* @type: Int;
    replayIndex,
    \* @type: Set(Str);
    replayProcessed,
    \* @type: Seq(Str);
    replayTrace,
    \* @type: Str -> Int;
    replaySpent

vars == <<
    phase,
    liquid,
    held,
    reservationSnapshot,
    spent,
    burned,
    senderLiquid,
    processed,
    trace,
    topUpDone,
    probeAttempted,
    probeAccepted,
    replayIndex,
    replayProcessed,
    replayTrace,
    replaySpent
>>

Init ==
    /\ phase = "admission"
    /\ liquid = InitialLiquid
    /\ held = ZeroVector
    /\ reservationSnapshot = ZeroVector
    /\ spent = ZeroVector
    /\ burned = ZeroVector
    /\ senderLiquid = InitialSender
    /\ processed = {}
    /\ trace = <<>>
    /\ topUpDone = FALSE
    /\ probeAttempted = FALSE
    /\ probeAccepted = FALSE
    /\ replayIndex = 0
    /\ replayProcessed = {}
    /\ replayTrace = <<>>
    /\ replaySpent = ZeroVector

ReserveAll ==
    /\ phase = "admission"
    /\ \A purse \in Purses : liquid[purse] >= ReservationBound[purse]
    /\ liquid' = [purse \in Purses |-> liquid[purse] - ReservationBound[purse]]
    /\ held' = ReservationBound
    /\ reservationSnapshot' = ReservationBound
    /\ phase' = "execution"
    /\ UNCHANGED <<
        spent, burned, senderLiquid, processed, trace, topUpDone,
        probeAttempted, probeAccepted, replayIndex, replayProcessed,
        replayTrace, replaySpent
       >>

Ready(event) ==
    /\ event \in Events \ processed
    /\ Dependencies[event] \subseteq processed

Execute(event) ==
    /\ phase = "execution"
    /\ Ready(event)
    /\ \A purse \in Purses :
        spent[purse] + ExecutionDebit(event, purse) <= reservationSnapshot[purse]
    /\ spent' = [purse \in Purses |->
        spent[purse] + ExecutionDebit(event, purse)]
    /\ processed' = processed \cup {event}
    /\ trace' = Append(trace, event)
    /\ UNCHANGED <<
        phase, liquid, held, reservationSnapshot, burned, senderLiquid,
        topUpDone, probeAttempted, probeAccepted, replayIndex,
        replayProcessed, replayTrace, replaySpent
       >>

AttemptUnderfundedContinuation ==
    /\ phase = "execution"
    /\ ~probeAttempted
    /\ probeAttempted' = TRUE
    /\ IF Defect = "CrossPurseRescue"
          /\ spent[RescuePurse] + ProbeAmount <= reservationSnapshot[RescuePurse]
       THEN /\ probeAccepted' = TRUE
            /\ spent' = [purse \in Purses |->
                IF purse = RescuePurse
                THEN spent[purse] + ProbeAmount
                ELSE spent[purse]]
       ELSE /\ probeAccepted' = FALSE
            /\ UNCHANGED spent
    /\ UNCHANGED <<
        phase, liquid, held, reservationSnapshot, burned, senderLiquid,
        processed, trace, topUpDone, replayIndex, replayProcessed,
        replayTrace, replaySpent
       >>

TopUp ==
    /\ phase \in {"execution", "replay"}
    /\ ~topUpDone
    /\ senderLiquid >= TopUpAmount
    /\ topUpDone' = TRUE
    /\ senderLiquid' = senderLiquid - TopUpAmount
    /\ IF Defect = "TopUpExpandsReservation"
       THEN /\ liquid' = liquid
            /\ held' = [purse \in Purses |->
                IF purse = TopUpTarget
                THEN held[purse] + TopUpAmount
                ELSE held[purse]]
            /\ reservationSnapshot' = [purse \in Purses |->
                IF purse = TopUpTarget
                THEN reservationSnapshot[purse] + TopUpAmount
                ELSE reservationSnapshot[purse]]
       ELSE /\ liquid' = [purse \in Purses |->
                IF purse = TopUpTarget
                THEN liquid[purse] + TopUpAmount
                ELSE liquid[purse]]
            /\ UNCHANGED <<held, reservationSnapshot>>
    /\ UNCHANGED <<
        phase, spent, burned, processed, trace, probeAttempted,
        probeAccepted, replayIndex, replayProcessed, replayTrace, replaySpent
       >>

Settle ==
    /\ phase = "execution"
    /\ processed = Events
    /\ probeAttempted
    /\ \A purse \in Purses : spent[purse] <= reservationSnapshot[purse]
    /\ liquid' = [purse \in Purses |->
        liquid[purse] + reservationSnapshot[purse] - spent[purse]]
    /\ held' = ZeroVector
    /\ burned' = [purse \in Purses |-> burned[purse] + spent[purse]]
    /\ phase' = "replay"
    /\ UNCHANGED <<
        reservationSnapshot, spent, senderLiquid, processed, trace,
        topUpDone, probeAttempted, probeAccepted, replayIndex,
        replayProcessed, replayTrace, replaySpent
       >>

ReplayNext ==
    LET event == trace[replayIndex + 1]
    IN /\ phase = "replay"
       /\ replayIndex < Len(trace)
       /\ Dependencies[event] \subseteq replayProcessed
       /\ replayIndex' = replayIndex + 1
       /\ replayProcessed' = replayProcessed \cup {event}
       /\ replayTrace' = Append(replayTrace, event)
       /\ replaySpent' = [purse \in Purses |->
            replaySpent[purse] + ReplayDebit(event, purse)]
       /\ UNCHANGED <<
            phase, liquid, held, reservationSnapshot, spent, burned,
            senderLiquid, processed, trace, topUpDone, probeAttempted,
            probeAccepted
          >>

Finish ==
    /\ phase = "replay"
    /\ replayIndex = Len(trace)
    /\ phase' = "done"
    /\ UNCHANGED <<
        liquid, held, reservationSnapshot, spent, burned, senderLiquid,
        processed, trace, topUpDone, probeAttempted, probeAccepted,
        replayIndex, replayProcessed, replayTrace, replaySpent
       >>

StayDone ==
    /\ phase = "done"
    /\ UNCHANGED vars

Next ==
    \/ ReserveAll
    \/ \E event \in Events : Execute(event)
    \/ AttemptUnderfundedContinuation
    \/ TopUp
    \/ Settle
    \/ ReplayNext
    \/ Finish
    \/ StayDone

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(ReserveAll)
    /\ \A event \in Events : WF_vars(Execute(event))
    /\ WF_vars(AttemptUnderfundedContinuation)
    /\ WF_vars(Settle)
    /\ WF_vars(ReplayNext)
    /\ WF_vars(Finish)

TypeOK ==
    /\ phase \in {"admission", "execution", "replay", "done"}
    /\ liquid \in [Purses -> Nat]
    /\ held \in [Purses -> Nat]
    /\ reservationSnapshot \in [Purses -> Nat]
    /\ spent \in [Purses -> Nat]
    /\ burned \in [Purses -> Nat]
    /\ senderLiquid \in Nat
    /\ processed \subseteq Events
    /\ Len(trace) <= MaxEvents
    /\ \A index \in {candidate \in 1..MaxEvents : candidate <= Len(trace)} :
        trace[index] \in Events
    /\ topUpDone \in BOOLEAN
    /\ probeAttempted \in BOOLEAN
    /\ probeAccepted \in BOOLEAN
    /\ replayIndex \in 0..Len(trace)
    /\ replayProcessed \subseteq Events
    /\ Len(replayTrace) <= MaxEvents
    /\ \A index \in {candidate \in 1..MaxEvents : candidate <= Len(replayTrace)} :
        replayTrace[index] \in Events
    /\ replaySpent \in [Purses -> Nat]

TraceIsExactEvidence ==
    /\ SequenceSet(trace) = processed
    /\ Len(trace) = Cardinality(processed)

ExactLocatedDebit ==
    \A purse \in Purses : spent[purse] = CanonicalDebitSum(processed, purse)

CompoundDebitIsComponentwise ==
    CompoundEvent \in processed =>
        /\ spent[OuterPurse] >= EventAmount[CompoundEvent]
        /\ spent[ContinuationPurse] >= EventAmount[CompoundEvent]

HardCeilingIsPerPurse ==
    \A purse \in Purses : spent[purse] <= reservationSnapshot[purse]

UnderfundedContinuationCannotUseAnotherPurse ==
    probeAttempted => ~probeAccepted

ReservationSnapshotImmutable ==
    phase # "admission" => reservationSnapshot = ReservationBound

TopUpOnlyChangesUnreservedLiquidity ==
    topUpDone => reservationSnapshot = ReservationBound

CanonicalValueConserved ==
    senderLiquid + PurseTotal(liquid) + PurseTotal(held) + PurseTotal(burned) =
      InitialSender + PurseTotal(InitialLiquid)

ReplayTraceIsExactPrefix ==
    replayTrace = SubSeq(trace, 1, replayIndex)

ReplayAllocationIsExact ==
    \A purse \in Purses :
      replaySpent[purse] = CanonicalDebitSum(replayProcessed, purse)

ReplayMatchesPlay ==
    phase = "done" =>
        /\ replayTrace = trace
        /\ replayProcessed = processed
        /\ replaySpent = [purse \in Purses |-> CanonicalDebitSum(processed, purse)]

EventuallyDone == <>(phase = "done")

PursesDef == {outerPurse, continuationPurse, envelopePurse}
EventsDef == {outerByte, continuationByte, compoundByte}
EventPursesDef ==
    [event \in EventsDef |->
        CASE event = outerByte -> {outerPurse}
          [] event = continuationByte -> {continuationPurse}
          [] OTHER -> {outerPurse, continuationPurse}]
EventAmountDef ==
    [event \in EventsDef |->
        CASE event = outerByte -> 5
          [] event = continuationByte -> 7
          [] OTHER -> 2]
DependenciesDef ==
    [event \in EventsDef |->
        IF event = compoundByte THEN {outerByte, continuationByte} ELSE {}]
ReservationBoundDef ==
    [purse \in PursesDef |->
        CASE purse = outerPurse -> 20
          [] purse = continuationPurse -> 9
          [] OTHER -> 20]
InitialLiquidDef == ReservationBoundDef

=============================================================================

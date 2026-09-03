---------------------- MODULE FinalizedOccurrenceStatus ----------------------
EXTENDS FiniteSets

CONSTANT
    \* @type: Bool;
    UseAllCausalTombstones,
    \* @type: Bool;
    FreezeVisibleSurvivor,
    \* @type: Bool;
    RestrictToFinalizedClosure,
    \* @type: Bool;
    RequireFloorReadyBeforeSettle,
    \* @type: Bool;
    UseRejectionAsStateSubtraction

ASSUME UseAllCausalTombstones \in BOOLEAN
ASSUME FreezeVisibleSurvivor \in BOOLEAN
ASSUME RestrictToFinalizedClosure \in BOOLEAN
ASSUME RequireFloorReadyBeforeSettle \in BOOLEAN
ASSUME UseRejectionAsStateSubtraction \in BOOLEAN

MainSource == "main-source"
RejectedSource == "rejected-source"
SecondaryRecord == "secondary-record"
OffFloorSource == "off-floor-source"
OffFloorRecord == "off-floor-record"
NoSource == "no-source"

Occurrences == {MainSource, RejectedSource, OffFloorSource}
CausalRecords == {SecondaryRecord, OffFloorRecord}
FinalizedOccurrences == {MainSource, RejectedSource}
FinalizedRecords == {SecondaryRecord}
MainChainRecords == {}

RecordTarget(record) ==
    CASE record = SecondaryRecord -> RejectedSource
      [] record = OffFloorRecord -> MainSource
      [] OTHER -> OffFloorSource

Targets(records) == {RecordTarget(record) : record \in records}

ConstructState(stateParent, applied, own, rejected) ==
    IF UseRejectionAsStateSubtraction
    THEN (stateParent \union applied \union own) \ rejected
    ELSE stateParent \union applied \union own

StateParentActive == {MainSource}
AppliedActive == {}
OwnActive == {}
SiblingRejected == {RejectedSource}
CommittedActive ==
    ConstructState(
        StateParentActive,
        AppliedActive,
        OwnActive,
        SiblingRejected)

SameLineageParentActive == {MainSource}
SameLineageRejected == {MainSource}
SameLineageActive ==
    ConstructState(
        SameLineageParentActive,
        {},
        {},
        SameLineageRejected)

VARIABLES
    \* @type: Str;
    phase,
    \* @type: Set(Str);
    unseenOccurrences,
    \* @type: Set(Str);
    unseenRecords,
    \* @type: Set(Str);
    observedOccurrences,
    \* @type: Set(Str);
    observedRecords,
    \* @type: Set(Str);
    statusSources,
    \* @type: Str;
    frozenSource,
    \* @type: Bool;
    floorReady

vars ==
    <<phase,
      unseenOccurrences,
      unseenRecords,
      observedOccurrences,
      observedRecords,
      statusSources,
      frozenSource,
      floorReady>>

Init ==
    /\ phase = "collecting"
    /\ unseenOccurrences = Occurrences
    /\ unseenRecords = CausalRecords
    /\ observedOccurrences = {}
    /\ observedRecords = {}
    /\ statusSources = {}
    /\ frozenSource = NoSource
    /\ floorReady = FALSE

ObserveOccurrence(source) ==
    /\ phase = "collecting"
    /\ source \in unseenOccurrences
    /\ unseenOccurrences' = unseenOccurrences \ {source}
    /\ observedOccurrences' = observedOccurrences \union {source}
    /\ UNCHANGED <<phase, unseenRecords, observedRecords, statusSources, frozenSource, floorReady>>

ObserveRecord(record) ==
    /\ phase = "collecting"
    /\ record \in unseenRecords
    /\ unseenRecords' = unseenRecords \ {record}
    /\ observedRecords' = observedRecords \union {record}
    /\ UNCHANGED <<phase, unseenOccurrences, observedOccurrences, statusSources, frozenSource, floorReady>>

MaterializeFloor ==
    /\ phase = "collecting"
    /\ ~floorReady
    /\ floorReady' = TRUE
    /\ UNCHANGED
        <<phase,
          unseenOccurrences,
          unseenRecords,
          observedOccurrences,
          observedRecords,
          statusSources,
          frozenSource>>

VisibleTombstones ==
    LET closureRecords ==
        IF RestrictToFinalizedClosure
        THEN observedRecords \intersect FinalizedRecords
        ELSE observedRecords
    IN
    IF UseAllCausalTombstones
    THEN Targets(closureRecords)
    ELSE Targets(closureRecords \intersect MainChainRecords)

VisibleOccurrences ==
    IF RestrictToFinalizedClosure
    THEN observedOccurrences \intersect FinalizedOccurrences
    ELSE observedOccurrences

VisibleSurvivors == VisibleOccurrences \ VisibleTombstones

SelectedVisibleSurvivor ==
    IF VisibleSurvivors = {}
    THEN NoSource
    ELSE CHOOSE source \in VisibleSurvivors : TRUE

Settle ==
    /\ phase = "collecting"
    /\ unseenOccurrences = {}
    /\ unseenRecords = {}
    /\ (~RequireFloorReadyBeforeSettle \/ floorReady)
    /\ phase' = "settled"
    /\ statusSources' = VisibleSurvivors
    /\ frozenSource' =
        IF FreezeVisibleSurvivor
        THEN SelectedVisibleSurvivor
        ELSE RejectedSource
    /\ UNCHANGED
        <<unseenOccurrences,
          unseenRecords,
          observedOccurrences,
          observedRecords,
          floorReady>>

Next ==
    \/ \E source \in Occurrences : ObserveOccurrence(source)
    \/ \E record \in CausalRecords : ObserveRecord(record)
    \/ MaterializeFloor
    \/ Settle

Spec ==
    Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"collecting", "settled"}
    /\ unseenOccurrences \subseteq Occurrences
    /\ unseenRecords \subseteq CausalRecords
    /\ observedOccurrences \subseteq Occurrences
    /\ observedRecords \subseteq CausalRecords
    /\ statusSources \subseteq Occurrences
    /\ frozenSource \in Occurrences \union {NoSource}
    /\ floorReady \in BOOLEAN

Inv_ObservationPartitionsEvidence ==
    /\ unseenOccurrences \intersect observedOccurrences = {}
    /\ unseenOccurrences \union observedOccurrences = Occurrences
    /\ unseenRecords \intersect observedRecords = {}
    /\ unseenRecords \union observedRecords = CausalRecords

Inv_StatusMatchesCommittedState ==
    phase = "settled" => statusSources = CommittedActive

Inv_SecondaryRejectionRemovesExactSource ==
    phase = "settled" => RejectedSource \notin statusSources

Inv_DistinctSourceSurvives ==
    phase = "settled" => MainSource \in statusSources

Inv_RejectionDoesNotSubtractStateParent ==
    phase = "settled" => MainSource \in SameLineageActive

Inv_OneFinalizedOccurrence ==
    phase = "settled" => Cardinality(statusSources) = 1

Inv_FrozenSourceMatchesCommittedState ==
    phase = "settled" =>
        /\ (statusSources = {} => frozenSource = NoSource)
        /\ (statusSources # {} => frozenSource \in statusSources)

Inv_RejectedSourceIsNeverFrozen ==
    phase = "settled" => frozenSource # RejectedSource

Inv_OffFloorEvidenceCannotAffectTerminalStatus ==
    phase = "settled" =>
        /\ OffFloorSource \notin statusSources
        /\ frozenSource # OffFloorSource

Inv_TerminalStatusRequiresFloorReady ==
    phase = "settled" => floorReady

Live_EventuallySettled == <>(phase = "settled")

=============================================================================

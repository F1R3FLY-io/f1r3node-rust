---------------------- MODULE FinalizedOccurrenceStatus ----------------------
EXTENDS FiniteSets

CONSTANT
    \* @type: Bool;
    UseAllCausalTombstones

ASSUME UseAllCausalTombstones \in BOOLEAN

MainSource == "main-source"
RejectedSource == "rejected-source"
SecondaryRecord == "secondary-record"

Occurrences == {MainSource, RejectedSource}
CausalRecords == {SecondaryRecord}
MainChainRecords == {}

RecordTarget(record) ==
    IF record = SecondaryRecord THEN RejectedSource ELSE MainSource

Targets(records) == {RecordTarget(record) : record \in records}
CommittedActive == Occurrences \ Targets(CausalRecords)

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
    statusSources

vars ==
    <<phase,
      unseenOccurrences,
      unseenRecords,
      observedOccurrences,
      observedRecords,
      statusSources>>

Init ==
    /\ phase = "collecting"
    /\ unseenOccurrences = Occurrences
    /\ unseenRecords = CausalRecords
    /\ observedOccurrences = {}
    /\ observedRecords = {}
    /\ statusSources = {}

ObserveOccurrence(source) ==
    /\ phase = "collecting"
    /\ source \in unseenOccurrences
    /\ unseenOccurrences' = unseenOccurrences \ {source}
    /\ observedOccurrences' = observedOccurrences \union {source}
    /\ UNCHANGED <<phase, unseenRecords, observedRecords, statusSources>>

ObserveRecord(record) ==
    /\ phase = "collecting"
    /\ record \in unseenRecords
    /\ unseenRecords' = unseenRecords \ {record}
    /\ observedRecords' = observedRecords \union {record}
    /\ UNCHANGED <<phase, unseenOccurrences, observedOccurrences, statusSources>>

VisibleTombstones ==
    IF UseAllCausalTombstones
    THEN Targets(observedRecords)
    ELSE Targets(observedRecords \intersect MainChainRecords)

Settle ==
    /\ phase = "collecting"
    /\ unseenOccurrences = {}
    /\ unseenRecords = {}
    /\ phase' = "settled"
    /\ statusSources' = observedOccurrences \ VisibleTombstones
    /\ UNCHANGED
        <<unseenOccurrences,
          unseenRecords,
          observedOccurrences,
          observedRecords>>

Next ==
    \/ \E source \in Occurrences : ObserveOccurrence(source)
    \/ \E record \in CausalRecords : ObserveRecord(record)
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

Inv_OneFinalizedOccurrence ==
    phase = "settled" => Cardinality(statusSources) = 1

Live_EventuallySettled == <>(phase = "settled")

=============================================================================

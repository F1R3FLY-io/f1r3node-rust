--------------------------- MODULE CarrierIndex ---------------------------
\* Rust mapping:
\* RecordBlock -> CarrierIndex::record_once from BlockDagKeyValueStorage::insert
\* PublishBlock -> BlockDagKeyValueStorage::insert metadata publication
\* AdvanceWindow -> KeyValueDagRepresentation::prune_carriers_below
\* FailRead -> carrier_index_watermark or carrier_index_proves_absence failure
\* Crash -> interruption between carrier recording and DAG publication
\* IndexBeforeDag distinguishes ingest-first ordering from the regression.
\* FallbackOnReadFailure distinguishes refusal from false absence.
EXTENDS Naturals, FiniteSets

CONSTANTS
  Sigs,
  Blocks,
  Height,
  BlockSigs,
  Watermark,
  MaxScanStart,
  IndexBeforeDag,
  FallbackOnReadFailure

VARIABLES indexed, visible, scanStart, readable

vars == <<indexed, visible, scanStart, readable>>

CarrierPairs(block) == {<<sig, block>> : sig \in BlockSigs[block]}

Init ==
  /\ indexed = {}
  /\ visible = {}
  /\ scanStart = Watermark
  /\ readable = TRUE

RecordBlock(block) ==
  /\ block \in Blocks
  /\ indexed' = indexed \union CarrierPairs(block)
  /\ UNCHANGED <<visible, scanStart, readable>>

PublishBlock(block) ==
  /\ block \in Blocks
  /\ block \notin visible
  /\ (~IndexBeforeDag \/ CarrierPairs(block) \subseteq indexed)
  /\ visible' = visible \union {block}
  /\ UNCHANGED <<indexed, scanStart, readable>>

AdvanceWindow(nextStart) ==
  /\ nextStart \in scanStart..MaxScanStart
  /\ scanStart' = nextStart
  /\ indexed' = {
       pair \in indexed : Height[pair[2]] >= nextStart
     }
  /\ UNCHANGED <<visible, readable>>

FailRead ==
  /\ readable
  /\ readable' = FALSE
  /\ UNCHANGED <<indexed, visible, scanStart>>

RestoreRead ==
  /\ ~readable
  /\ readable' = TRUE
  /\ UNCHANGED <<indexed, visible, scanStart>>

Crash == UNCHANGED vars

Next ==
  \/ \E block \in Blocks : RecordBlock(block)
  \/ \E block \in Blocks : PublishBlock(block)
  \/ \E nextStart \in scanStart..MaxScanStart : AdvanceWindow(nextStart)
  \/ FailRead
  \/ RestoreRead
  \/ Crash

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ indexed \subseteq Sigs \X Blocks
  /\ visible \subseteq Blocks
  /\ scanStart \in Watermark..MaxScanStart
  /\ readable \in BOOLEAN

IndexCompleteForWindow ==
  \A block \in visible :
    (Height[block] >= scanStart /\ Height[block] >= Watermark)
      => CarrierPairs(block) \subseteq indexed

NoIndexedCarrier(sig) ==
  ~\E block \in Blocks :
    /\ Height[block] >= scanStart
    /\ <<sig, block>> \in indexed

NoVisibleCarrier(sig) ==
  ~\E block \in visible :
    /\ Height[block] >= scanStart
    /\ sig \in BlockSigs[block]

FastAbsence(sig) ==
  /\ Watermark <= scanStart
  /\ IF readable
        THEN NoIndexedCarrier(sig)
        ELSE ~FallbackOnReadFailure

AbsenceProofSound ==
  \A sig \in Sigs : FastAbsence(sig) => NoVisibleCarrier(sig)

=============================================================================

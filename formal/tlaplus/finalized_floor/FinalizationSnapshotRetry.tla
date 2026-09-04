---------------------- MODULE FinalizationSnapshotRetry ----------------------
EXTENDS Integers, TLC

CONSTANT
    \* @type: Int;
    MaxRevision,
    \* @type: Bool;
    UnsafePublishStaleReader

ASSUME /\ MaxRevision \in Nat \ {0}
       /\ UnsafePublishStaleReader \in BOOLEAN

Phases == {"Idle", "Projection", "Validate", "Done"}

VARIABLES
    \* @type: Int;
    ledgerHead,
    \* @type: Int;
    projectionCursor,
    \* @type: Int;
    dagFloor,
    \* @type: Set(Int);
    records,
    \* @type: Str;
    readerPhase,
    \* @type: Int;
    readerBeforeHead,
    \* @type: Int;
    readerBeforeCursor,
    \* @type: Int;
    readerDagFloor,
    \* @type: Int;
    resultRevision,
    \* @type: Int;
    resultFloor,
    \* @type: Int;
    resultCertificate,
    \* @type: Int;
    retries,
    \* @type: Bool;
    lastCaptureStale,
    \* @type: Bool;
    lastCaptureCorrupt

vars == <<ledgerHead, projectionCursor, dagFloor, records, readerPhase,
          readerBeforeHead, readerBeforeCursor, readerDagFloor, resultRevision,
          resultFloor, resultCertificate, retries, lastCaptureStale,
          lastCaptureCorrupt>>

Init ==
    /\ ledgerHead = 0
    /\ projectionCursor = 0
    /\ dagFloor = 0
    /\ records = {}
    /\ readerPhase = "Idle"
    /\ readerBeforeHead = 0
    /\ readerBeforeCursor = 0
    /\ readerDagFloor = 0
    /\ resultRevision = -1
    /\ resultFloor = -1
    /\ resultCertificate = -1
    /\ retries = 0
    /\ lastCaptureStale = FALSE
    /\ lastCaptureCorrupt = FALSE

AppendLedger ==
    /\ ledgerHead < MaxRevision
    /\ ledgerHead' = ledgerHead + 1
    /\ records' = records \cup {ledgerHead + 1}
    /\ UNCHANGED <<projectionCursor, dagFloor, readerPhase,
                    readerBeforeHead, readerBeforeCursor, readerDagFloor,
                    resultRevision, resultFloor, resultCertificate, retries,
                    lastCaptureStale, lastCaptureCorrupt>>

ProjectNextRecord ==
    /\ dagFloor = projectionCursor
    /\ dagFloor < ledgerHead
    /\ readerPhase # "Validate"
    /\ dagFloor' = dagFloor + 1
    /\ UNCHANGED <<ledgerHead, projectionCursor, records, readerPhase,
                    readerBeforeHead, readerBeforeCursor, readerDagFloor,
                    resultRevision, resultFloor, resultCertificate, retries,
                    lastCaptureStale, lastCaptureCorrupt>>

CompleteProjection ==
    /\ projectionCursor < dagFloor
    /\ projectionCursor' = projectionCursor + 1
    /\ UNCHANGED <<ledgerHead, dagFloor, records, readerPhase,
                    readerBeforeHead, readerBeforeCursor, readerDagFloor,
                    resultRevision, resultFloor, resultCertificate, retries,
                    lastCaptureStale, lastCaptureCorrupt>>

BeginCapture ==
    /\ readerPhase = "Idle"
    /\ readerPhase' = "Projection"
    /\ readerBeforeHead' = ledgerHead
    /\ readerBeforeCursor' = projectionCursor
    /\ resultRevision' = -1
    /\ resultFloor' = -1
    /\ resultCertificate' = -1
    /\ lastCaptureStale' = FALSE
    /\ lastCaptureCorrupt' = FALSE
    /\ UNCHANGED <<ledgerHead, projectionCursor, dagFloor, records,
                    readerDagFloor, retries>>

CaptureProjection ==
    /\ readerPhase = "Projection"
    /\ readerBeforeHead = readerBeforeCursor
    /\ readerPhase' = "Validate"
    /\ readerDagFloor' = dagFloor
    /\ UNCHANGED <<ledgerHead, projectionCursor, dagFloor, records,
                    readerBeforeHead, readerBeforeCursor, resultRevision,
                    resultFloor, resultCertificate, retries, lastCaptureStale,
                    lastCaptureCorrupt>>

NextRetryCount == IF retries <= MaxRevision THEN retries + 1 ELSE retries

RetryLaggingFirstEndpoint ==
    /\ readerPhase = "Projection"
    /\ readerBeforeHead # readerBeforeCursor
    /\ readerPhase' = "Idle"
    /\ retries' = NextRetryCount
    /\ resultRevision' = -1
    /\ resultFloor' = -1
    /\ resultCertificate' = -1
    /\ lastCaptureStale' = TRUE
    /\ lastCaptureCorrupt' = FALSE
    /\ UNCHANGED <<ledgerHead, projectionCursor, dagFloor, records,
                    readerBeforeHead, readerBeforeCursor, readerDagFloor>>

EndpointChangedOrLagging ==
    \/ readerBeforeHead # ledgerHead
    \/ readerBeforeCursor # projectionCursor
    \/ projectionCursor # ledgerHead

StableFullyProjectedMismatch ==
    /\ readerBeforeHead = ledgerHead
    /\ readerBeforeCursor = projectionCursor
    /\ projectionCursor = ledgerHead
    /\ readerDagFloor # ledgerHead

PublishCoherentCapture ==
    /\ readerPhase = "Validate"
    /\ readerBeforeHead = ledgerHead
    /\ readerBeforeCursor = projectionCursor
    /\ projectionCursor = ledgerHead
    /\ readerDagFloor = ledgerHead
    /\ readerPhase' = "Done"
    /\ resultRevision' = readerBeforeHead
    /\ resultFloor' = readerDagFloor
    /\ resultCertificate' = readerBeforeHead
    /\ lastCaptureStale' = FALSE
    /\ lastCaptureCorrupt' = FALSE
    /\ UNCHANGED <<ledgerHead, projectionCursor, dagFloor, records,
                    readerBeforeHead, readerBeforeCursor, readerDagFloor,
                    retries>>

RetryStaleCapture ==
    /\ readerPhase = "Validate"
    /\ EndpointChangedOrLagging
    /\ readerPhase' = "Idle"
    /\ retries' = NextRetryCount
    /\ resultRevision' = -1
    /\ resultFloor' = -1
    /\ resultCertificate' = -1
    /\ lastCaptureStale' = TRUE
    /\ lastCaptureCorrupt' = FALSE
    /\ UNCHANGED <<ledgerHead, projectionCursor, dagFloor, records,
                    readerBeforeHead, readerBeforeCursor, readerDagFloor>>

RejectCorruptCapture ==
    /\ readerPhase = "Validate"
    /\ StableFullyProjectedMismatch
    /\ readerPhase' = "Done"
    /\ resultRevision' = -1
    /\ resultFloor' = -1
    /\ resultCertificate' = -1
    /\ lastCaptureStale' = FALSE
    /\ lastCaptureCorrupt' = TRUE
    /\ UNCHANGED <<ledgerHead, projectionCursor, dagFloor, records,
                    readerBeforeHead, readerBeforeCursor, readerDagFloor,
                    retries>>

UnsafePublishStaleCapture ==
    /\ UnsafePublishStaleReader
    /\ \/ /\ readerPhase = "Projection"
           /\ readerBeforeHead # readerBeforeCursor
       \/ /\ readerPhase = "Validate"
           /\ EndpointChangedOrLagging
    /\ readerPhase' = "Done"
    /\ resultRevision' = readerBeforeHead
    /\ resultFloor' = readerDagFloor
    /\ resultCertificate' = ledgerHead
    /\ lastCaptureStale' = TRUE
    /\ lastCaptureCorrupt' = FALSE
    /\ UNCHANGED <<ledgerHead, projectionCursor, dagFloor, records,
                    readerBeforeHead, readerBeforeCursor, readerDagFloor,
                    retries>>

Next ==
    \/ AppendLedger
    \/ ProjectNextRecord
    \/ CompleteProjection
    \/ BeginCapture
    \/ CaptureProjection
    \/ RetryLaggingFirstEndpoint
    \/ PublishCoherentCapture
    \/ RetryStaleCapture
    \/ RejectCorruptCapture
    \/ UnsafePublishStaleCapture

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ ledgerHead \in 0..MaxRevision
    /\ projectionCursor \in 0..MaxRevision
    /\ dagFloor \in 0..MaxRevision
    /\ records \subseteq 1..MaxRevision
    /\ readerPhase \in Phases
    /\ readerBeforeHead \in 0..MaxRevision
    /\ readerBeforeCursor \in 0..MaxRevision
    /\ readerDagFloor \in 0..MaxRevision
    /\ resultRevision \in -1..MaxRevision
    /\ resultFloor \in -1..MaxRevision
    /\ resultCertificate \in -1..MaxRevision
    /\ retries \in 0..(MaxRevision + 1)
    /\ lastCaptureStale \in BOOLEAN
    /\ lastCaptureCorrupt \in BOOLEAN

Inv_ProjectionPrefix ==
    /\ projectionCursor <= dagFloor
    /\ dagFloor <= ledgerHead

Inv_RecordPrefix == records = {revision \in 1..MaxRevision : revision <= ledgerHead}

Inv_ReaderResultCoherent ==
    resultRevision = -1 \/
        /\ resultRevision = resultFloor
        /\ resultFloor = resultCertificate
        /\ resultRevision <= ledgerHead

Inv_ReaderResultRecorded ==
    resultRevision <= 0 \/ resultRevision \in records

Inv_CapturedRevisionRemainsDurablePrefix ==
    resultRevision = -1 \/
        /\ resultRevision <= ledgerHead
        /\ (resultRevision = 0 \/ resultRevision \in records)

Inv_UnsafeCapturedRevisionEqualsCurrentHead ==
    resultRevision = -1 \/ resultRevision = ledgerHead

Inv_StaleReaderHasNoResult ==
    lastCaptureStale => resultRevision = -1

Inv_CorruptReaderHasNoResult ==
    lastCaptureCorrupt => resultRevision = -1

Inv_CoherentResultUsesOneEndpoint ==
    resultRevision # -1 =>
        /\ readerBeforeHead = readerBeforeCursor
        /\ readerBeforeHead = readerDagFloor
        /\ resultRevision = readerBeforeHead

Safety ==
    /\ TypeOK
    /\ Inv_ProjectionPrefix
    /\ Inv_RecordPrefix
    /\ Inv_ReaderResultCoherent
    /\ Inv_ReaderResultRecorded
    /\ Inv_CapturedRevisionRemainsDurablePrefix
    /\ Inv_StaleReaderHasNoResult
    /\ Inv_CorruptReaderHasNoResult
    /\ Inv_CoherentResultUsesOneEndpoint

=============================================================================

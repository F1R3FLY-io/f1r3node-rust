---------------------- MODULE FinalizationSnapshotRetry ----------------------
EXTENDS Integers, TLC

CONSTANT
    MaxRevision,
    UnsafePublishStaleReader

ASSUME /\ MaxRevision \in Nat \ {0}
       /\ UnsafePublishStaleReader \in BOOLEAN

Phases == {"Idle", "Projection", "Validate", "Done"}

VARIABLES
    ledgerHead,
    projectedHead,
    records,
    readerPhase,
    readerBefore,
    readerProjection,
    resultRevision,
    resultFloor,
    resultCertificate,
    retries,
    lastCaptureStale

vars == <<ledgerHead, projectedHead, records, readerPhase, readerBefore,
          readerProjection, resultRevision, resultFloor, resultCertificate,
          retries, lastCaptureStale>>

Init ==
    /\ ledgerHead = 0
    /\ projectedHead = 0
    /\ records = {}
    /\ readerPhase = "Idle"
    /\ readerBefore = 0
    /\ readerProjection = 0
    /\ resultRevision = -1
    /\ resultFloor = -1
    /\ resultCertificate = -1
    /\ retries = 0
    /\ lastCaptureStale = FALSE

AppendLedger ==
    /\ ledgerHead < MaxRevision
    /\ ledgerHead' = ledgerHead + 1
    /\ records' = records \cup {ledgerHead + 1}
    /\ UNCHANGED <<projectedHead, readerPhase, readerBefore,
                    readerProjection, resultRevision, resultFloor,
                    resultCertificate, retries, lastCaptureStale>>

ProjectHead ==
    /\ projectedHead < ledgerHead
    /\ projectedHead' = ledgerHead
    /\ UNCHANGED <<ledgerHead, records, readerPhase, readerBefore,
                    readerProjection, resultRevision, resultFloor,
                    resultCertificate, retries, lastCaptureStale>>

BeginCapture ==
    /\ readerPhase = "Idle"
    /\ projectedHead = ledgerHead
    /\ readerPhase' = "Projection"
    /\ readerBefore' = ledgerHead
    /\ resultRevision' = -1
    /\ resultFloor' = -1
    /\ resultCertificate' = -1
    /\ lastCaptureStale' = FALSE
    /\ UNCHANGED <<ledgerHead, projectedHead, records, readerProjection, retries>>

CaptureProjection ==
    /\ readerPhase = "Projection"
    /\ readerPhase' = "Validate"
    /\ readerProjection' = projectedHead
    /\ UNCHANGED <<ledgerHead, projectedHead, records, readerBefore,
                    resultRevision, resultFloor, resultCertificate, retries,
                    lastCaptureStale>>

PublishCoherentCapture ==
    /\ readerPhase = "Validate"
    /\ readerBefore = ledgerHead
    /\ readerProjection = readerBefore
    /\ readerPhase' = "Done"
    /\ resultRevision' = readerBefore
    /\ resultFloor' = readerBefore
    /\ resultCertificate' = readerBefore
    /\ lastCaptureStale' = FALSE
    /\ UNCHANGED <<ledgerHead, projectedHead, records, readerBefore,
                    readerProjection, retries>>

RetryStaleCapture ==
    /\ readerPhase = "Validate"
    /\ \/ readerBefore # ledgerHead
       \/ readerProjection # readerBefore
    /\ readerPhase' = "Idle"
    /\ retries' = retries + 1
    /\ resultRevision' = -1
    /\ resultFloor' = -1
    /\ resultCertificate' = -1
    /\ lastCaptureStale' = TRUE
    /\ UNCHANGED <<ledgerHead, projectedHead, records, readerBefore,
                    readerProjection>>

UnsafePublishStaleCapture ==
    /\ UnsafePublishStaleReader
    /\ readerPhase = "Validate"
    /\ \/ readerBefore # ledgerHead
       \/ readerProjection # readerBefore
    /\ readerPhase' = "Done"
    /\ resultRevision' = readerBefore
    /\ resultFloor' = readerProjection
    /\ resultCertificate' = ledgerHead
    /\ lastCaptureStale' = TRUE
    /\ UNCHANGED <<ledgerHead, projectedHead, records, readerBefore,
                    readerProjection, retries>>

Next ==
    \/ AppendLedger
    \/ ProjectHead
    \/ BeginCapture
    \/ CaptureProjection
    \/ PublishCoherentCapture
    \/ RetryStaleCapture
    \/ UnsafePublishStaleCapture

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ ledgerHead \in 0..MaxRevision
    /\ projectedHead \in 0..MaxRevision
    /\ projectedHead <= ledgerHead
    /\ records \subseteq 1..MaxRevision
    /\ readerPhase \in Phases
    /\ readerBefore \in 0..MaxRevision
    /\ readerProjection \in 0..MaxRevision
    /\ resultRevision \in -1..MaxRevision
    /\ resultFloor \in -1..MaxRevision
    /\ resultCertificate \in -1..MaxRevision
    /\ retries \in Nat
    /\ lastCaptureStale \in BOOLEAN

Inv_RecordPrefix == records = 1..ledgerHead

Inv_ReaderResultCoherent ==
    resultRevision = -1 \/
        /\ resultRevision = resultFloor
        /\ resultFloor = resultCertificate
        /\ resultRevision <= ledgerHead

Inv_ReaderResultRecorded ==
    resultRevision <= 0 \/ resultRevision \in records

Inv_StaleReaderHasNoResult ==
    readerPhase # "Done" => resultRevision = -1

Safety ==
    /\ TypeOK
    /\ Inv_RecordPrefix
    /\ Inv_ReaderResultCoherent
    /\ Inv_ReaderResultRecorded
    /\ Inv_StaleReaderHasNoResult

=============================================================================

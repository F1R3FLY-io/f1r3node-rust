------------------------- MODULE BufferScanResidency -------------------------
EXTENDS Naturals, Sequences, FiniteSets, Apalache

CONSTANTS
    \* @type: Set(Str);
    Blocks,
    \* @type: Int;
    MaxBlockBytes,
    \* @type: Int;
    ByteCap,
    \* @type: Int;
    CountCap,
    \* @type: Int;
    MaxParallel,
    \* @type: Bool;
    ScanBounded

ASSUME MaxBlockBytes <= ByteCap
ASSUME MaxBlockBytes >= 1 /\ CountCap >= 1 /\ MaxParallel >= 1

VARIABLES
    \* @type: Str -> Int;
    size,
    \* @type: Set(Str);
    durable,
    \* @type: Set(Str);
    scanning,
    \* @type: Seq(Str);
    queued,
    \* @type: Set(Str);
    processing,
    \* @type: Set(Str);
    processed

vars == <<size, durable, scanning, queued, processing, processed>>

\* @type: Seq(Str) => Set(Str);
SeqRange(s) == {s[i] : i \in DOMAIN s}

\* @type: (Int, Str) => Int;
AddBlockBytes(total, block) == total + size[block]

\* @type: Set(Str) => Int;
SetBytes(S) == ApaFoldSet(AddBlockBytes, 0, S)

\* @type: Seq(Str) => Int;
SeqBytes(s) == SetBytes(SeqRange(s))

Persisted == durable \cup processed
Owned == SeqRange(queued) \cup processing
Eligible == durable \ Owned
RetainedBytes == SeqBytes(queued) + SetBytes(processing)
ScannerBytes == SetBytes(scanning)

Init ==
    /\ size = [b \in Blocks |-> 0]
    /\ durable = {}
    /\ scanning = {}
    /\ queued = <<>>
    /\ processing = {}
    /\ processed = {}

Persist(b, bytes) ==
    /\ b \notin Persisted
    /\ size' = [size EXCEPT ![b] = bytes]
    /\ durable' = durable \cup {b}
    /\ UNCHANGED <<scanning, queued, processing, processed>>

BeginBoundedScan(b) ==
    /\ ScanBounded
    /\ scanning = {}
    /\ b \in Eligible
    /\ scanning' = {b}
    /\ UNCHANGED <<size, durable, queued, processing, processed>>

BeginUnboundedScan ==
    /\ ~ScanBounded
    /\ scanning = {}
    /\ Eligible # {}
    /\ scanning' = Eligible
    /\ UNCHANGED <<size, durable, queued, processing, processed>>

AdmissionOk(b) ==
    /\ Len(queued) < CountCap
    /\ RetainedBytes + size[b] <= ByteCap

AdmitScanned(b) ==
    /\ b \in scanning
    /\ AdmissionOk(b)
    /\ scanning' = scanning \ {b}
    /\ queued' = Append(queued, b)
    /\ UNCHANGED <<size, durable, processing, processed>>

ReleaseScanned(b) ==
    /\ b \in scanning
    /\ ~AdmissionOk(b)
    /\ scanning' = scanning \ {b}
    /\ UNCHANGED <<size, durable, queued, processing, processed>>

StartProcessing ==
    /\ queued # <<>>
    /\ Cardinality(processing) < MaxParallel
    /\ processing' = processing \cup {Head(queued)}
    /\ queued' = Tail(queued)
    /\ UNCHANGED <<size, durable, scanning, processed>>

Complete(b) ==
    /\ b \in processing
    /\ processing' = processing \ {b}
    /\ durable' = durable \ {b}
    /\ processed' = processed \cup {b}
    /\ UNCHANGED <<size, scanning, queued>>

Done ==
    /\ processed = Blocks
    /\ durable = {}
    /\ scanning = {}
    /\ queued = <<>>
    /\ processing = {}
    /\ UNCHANGED vars

Next ==
    \/ \E b \in Blocks, bytes \in 1..MaxBlockBytes : Persist(b, bytes)
    \/ \E b \in Blocks : BeginBoundedScan(b)
    \/ BeginUnboundedScan
    \/ \E b \in Blocks : AdmitScanned(b)
    \/ \E b \in Blocks : ReleaseScanned(b)
    \/ StartProcessing
    \/ \E b \in Blocks : Complete(b)
    \/ Done

Fairness ==
    /\ \A b \in Blocks : WF_vars(BeginBoundedScan(b))
    /\ WF_vars(BeginUnboundedScan)
    /\ \A b \in Blocks : WF_vars(AdmitScanned(b))
    /\ \A b \in Blocks : WF_vars(ReleaseScanned(b))
    /\ WF_vars(StartProcessing)
    /\ \A b \in Blocks : WF_vars(Complete(b))

Spec == Init /\ [][Next]_vars /\ Fairness

TypeOK ==
    /\ size \in [Blocks -> 0..MaxBlockBytes]
    /\ durable \subseteq Blocks
    /\ scanning \subseteq durable
    /\ Len(queued) <= CountCap
    /\ \A index \in DOMAIN queued : queued[index] \in Blocks
    /\ Cardinality(SeqRange(queued)) = Len(queued)
    /\ processing \subseteq Blocks
    /\ processed \subseteq Blocks
    /\ Owned \subseteq durable
    /\ scanning \cap Owned = {}
    /\ durable \cap processed = {}
    /\ SeqRange(queued) \cap processing = {}

Inv_ScannerSinglePayload == Cardinality(scanning) <= 1
Inv_RetainedBytesBounded == RetainedBytes <= ByteCap
Inv_TotalPayloadBounded ==
    RetainedBytes + ScannerBytes <= ByteCap + MaxBlockBytes

Safety ==
    /\ TypeOK
    /\ Inv_ScannerSinglePayload
    /\ Inv_RetainedBytesBounded
    /\ Inv_TotalPayloadBounded

Live_AllPersistedProcessed ==
    \A b \in Blocks : (b \in Persisted) ~> (b \in processed)

=============================================================================

------------------------- MODULE BlockPayloadOwnership -------------------------
EXTENDS Naturals, Sequences, FiniteSets, Apalache

CONSTANTS
    \* @type: Set(Str);
    Attempts,
    \* @type: Int;
    ByteCap,
    \* @type: Int;
    MaxBytes,
    \* @type: Int;
    CountCap,
    \* @type: Int;
    MaxWorkers,
    \* @type: Int;
    ResultCap,
    \* @type: Bool;
    CompactResults,
    \* @type: Bool;
    KeepWorkerCopy,
    \* @type: Bool;
    GuardPayloadRelease

VARIABLES
    \* @type: Str -> Int;
    size,
    \* @type: Set(Str);
    reserved,
    \* @type: Set(Str);
    staged,
    \* @type: Seq(Str);
    queued,
    \* @type: Set(Str);
    active,
    \* @type: Set(Str);
    finished,
    \* @type: Set(Str);
    published,
    \* @type: Seq(Str);
    results,
    \* @type: Set(Str);
    resultPayload,
    \* @type: Set(Str);
    workerCopy,
    \* @type: Set(Str);
    cancelled

vars == <<size, reserved, staged, queued, active, finished, published,
          results, resultPayload, workerCopy, cancelled>>

\* @type: Seq(Str) => Set(Str);
SeqRange(sequence) == {sequence[index] : index \in DOMAIN sequence}

\* @type: (Int, Str) => Int;
AddSize(total, attempt) == total + size[attempt]

\* @type: Set(Str) => Int;
Bytes(owners) == ApaFoldSet(AddSize, 0, owners)

Begun == {attempt \in Attempts : size[attempt] > 0}
PayloadOwned == staged \cup SeqRange(queued) \cup active \cup resultPayload \cup workerCopy
LogicalPayloadBytes == Bytes(PayloadOwned)
ReservedBytes == Bytes(reserved)

Init ==
    /\ size = [attempt \in Attempts |-> 0]
    /\ reserved = {}
    /\ staged = {}
    /\ queued = <<>>
    /\ active = {}
    /\ finished = {}
    /\ published = {}
    /\ results = <<>>
    /\ resultPayload = {}
    /\ workerCopy = {}
    /\ cancelled = {}

Reserve(attempt, bytes) ==
    /\ attempt \notin Begun
    /\ bytes \in 1..MaxBytes
    /\ ReservedBytes + bytes <= ByteCap
    /\ size' = [size EXCEPT ![attempt] = bytes]
    /\ reserved' = reserved \cup {attempt}
    /\ staged' = staged \cup {attempt}
    /\ UNCHANGED <<queued, active, finished, published, results,
                    resultPayload, workerCopy, cancelled>>

Enqueue(attempt) ==
    /\ attempt \in staged
    /\ Len(queued) < CountCap
    /\ staged' = staged \ {attempt}
    /\ queued' = Append(queued, attempt)
    /\ UNCHANGED <<size, reserved, active, finished, published, results,
                    resultPayload, workerCopy, cancelled>>

Start ==
    /\ queued # <<>>
    /\ Cardinality(active) < MaxWorkers
    /\ active' = active \cup {Head(queued)}
    /\ workerCopy' = IF KeepWorkerCopy
                      THEN workerCopy \cup {Head(queued)} ELSE workerCopy
    /\ queued' = Tail(queued)
    /\ UNCHANGED <<size, reserved, staged, finished, published, results,
                    resultPayload, cancelled>>

Complete(attempt) ==
    /\ attempt \in active
    /\ active' = active \ {attempt}
    /\ finished' = finished \cup {attempt}
    /\ resultPayload' = IF CompactResults
                         THEN resultPayload ELSE resultPayload \cup {attempt}
    /\ UNCHANGED <<size, reserved, staged, queued, published, results,
                    workerCopy, cancelled>>

Publish(attempt) ==
    /\ attempt \in finished \ (published \cup cancelled)
    /\ Len(results) < ResultCap
    /\ published' = published \cup {attempt}
    /\ results' = Append(results, attempt)
    /\ UNCHANGED <<size, reserved, staged, queued, active, finished,
                    resultPayload, workerCopy, cancelled>>

ConsumeResult ==
    /\ results # <<>>
    /\ resultPayload' = resultPayload \ {Head(results)}
    /\ results' = Tail(results)
    /\ UNCHANGED <<size, reserved, staged, queued, active, finished,
                    published, workerCopy, cancelled>>

Release(attempt) ==
    /\ attempt \in reserved
    /\ attempt \in published \cup cancelled
    /\ GuardPayloadRelease => attempt \notin PayloadOwned
    /\ reserved' = reserved \ {attempt}
    /\ UNCHANGED <<size, staged, queued, active, finished, published,
                    results, resultPayload, workerCopy, cancelled>>

FinishTail(attempt) ==
    /\ attempt \in workerCopy
    /\ attempt \in published \cup cancelled
    /\ workerCopy' = workerCopy \ {attempt}
    /\ UNCHANGED <<size, reserved, staged, queued, active, finished,
                    published, results, resultPayload, cancelled>>

Cancel(attempt) ==
    /\ attempt \in staged \cup active \cup (finished \ published)
    /\ attempt \notin cancelled
    /\ staged' = staged \ {attempt}
    /\ active' = active \ {attempt}
    /\ resultPayload' = resultPayload \ {attempt}
    /\ workerCopy' = workerCopy \ {attempt}
    /\ cancelled' = cancelled \cup {attempt}
    /\ UNCHANGED <<size, reserved, queued, finished, published, results>>

DropQueued ==
    /\ queued # <<>>
    /\ cancelled' = cancelled \cup {Head(queued)}
    /\ queued' = Tail(queued)
    /\ UNCHANGED <<size, reserved, staged, active, finished, published,
                    results, resultPayload, workerCopy>>

Next ==
    \/ \E attempt \in Attempts, bytes \in 1..MaxBytes : Reserve(attempt, bytes)
    \/ \E attempt \in Attempts : Enqueue(attempt)
    \/ Start
    \/ \E attempt \in Attempts : Complete(attempt)
    \/ \E attempt \in Attempts : Publish(attempt)
    \/ ConsumeResult
    \/ \E attempt \in Attempts : Release(attempt)
    \/ \E attempt \in Attempts : FinishTail(attempt)
    \/ \E attempt \in Attempts : Cancel(attempt)
    \/ DropQueued
    \/ UNCHANGED vars

TypeOK ==
    /\ size \in [Attempts -> 0..MaxBytes]
    /\ reserved \subseteq Begun
    /\ staged \subseteq Begun
    /\ Len(queued) <= CountCap
    /\ SeqRange(queued) \subseteq Begun
    /\ Cardinality(SeqRange(queued)) = Len(queued)
    /\ active \subseteq Begun
    /\ Cardinality(active) <= MaxWorkers
    /\ finished \subseteq Begun
    /\ published \subseteq finished
    /\ Len(results) <= ResultCap
    /\ SeqRange(results) \subseteq published
    /\ Cardinality(SeqRange(results)) = Len(results)
    /\ resultPayload \subseteq finished
    /\ workerCopy \subseteq Begun
    /\ cancelled \subseteq Begun

Inv_ExactReservationCoverage == PayloadOwned \subseteq reserved
Inv_LogicalPayloadBound == LogicalPayloadBytes <= ReservedBytes /\ ReservedBytes <= ByteCap
Inv_NoResultPayload == resultPayload = {}
Inv_NoWorkerCopy == workerCopy = {}

Safety == TypeOK /\ Inv_ExactReservationCoverage /\ Inv_LogicalPayloadBound
Spec == Init /\ [][Next]_vars
=============================================================================

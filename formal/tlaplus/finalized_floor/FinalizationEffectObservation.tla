-------------------- MODULE FinalizationEffectObservation --------------------
EXTENDS Integers, FiniteSets

CONSTANTS
    \* @type: Int;
    MaxRounds,
    \* @type: Int;
    MaxReaders,
    \* @type: Str;
    Bug

ASSUME /\ MaxRounds > 0
       /\ MaxReaders > 0
       /\ Bug \in {"none", "missing-recheck"}

Rounds == 1..MaxRounds
Readers == 1..MaxReaders
Prefix(revision) == {round \in Rounds : round <= revision}

VARIABLES
    \* @type: Int;
    cursor,
    \* @type: Set(Int);
    receipts,
    \* @type: Set(Int);
    completed,
    \* @type: Int -> Int;
    target,
    \* @type: Int -> Str;
    phase,
    \* @type: Int -> Int;
    firstCursor,
    \* @type: Int -> Int;
    receiptCursor,
    \* @type: Int -> Bool;
    receiptPresent,
    \* @type: Int -> Bool;
    receiptComplete,
    \* @type: Int -> Int;
    lastCursor,
    \* @type: Int -> Int;
    answer

vars == <<cursor, receipts, completed, target, phase, firstCursor,
          receiptCursor, receiptPresent, receiptComplete, lastCursor, answer>>

Init ==
    /\ cursor = 0
    /\ receipts \in SUBSET Rounds
    /\ completed = receipts
    /\ target \in [Readers -> Rounds]
    /\ phase = [reader \in Readers |-> "first"]
    /\ firstCursor = [reader \in Readers |-> 0]
    /\ receiptCursor = [reader \in Readers |-> 0]
    /\ receiptPresent = [reader \in Readers |-> FALSE]
    /\ receiptComplete = [reader \in Readers |-> FALSE]
    /\ lastCursor = [reader \in Readers |-> 0]
    /\ answer = [reader \in Readers |-> -1]

Complete(round) ==
    /\ round \notin completed
    /\ receipts' = receipts \cup {round}
    /\ completed' = completed \cup {round}
    /\ UNCHANGED <<cursor, target, phase, firstCursor, receiptCursor,
                    receiptPresent, receiptComplete, lastCursor, answer>>

Advance ==
    /\ cursor < MaxRounds
    /\ cursor + 1 \in completed
    /\ cursor' = cursor + 1
    /\ UNCHANGED <<receipts, completed, target, phase, firstCursor,
                    receiptCursor, receiptPresent, receiptComplete, lastCursor, answer>>

Compact(round) ==
    /\ round <= cursor
    /\ round \in receipts
    /\ receipts' = receipts \ {round}
    /\ UNCHANGED <<cursor, completed, target, phase, firstCursor,
                    receiptCursor, receiptPresent, receiptComplete, lastCursor, answer>>

ReadFirst(reader) ==
    /\ phase[reader] = "first"
    /\ firstCursor' = [firstCursor EXCEPT ![reader] = cursor]
    /\ phase' = [phase EXCEPT ![reader] =
                   IF target[reader] <= cursor THEN "done" ELSE "receipt"]
    /\ answer' = [answer EXCEPT ![reader] =
                    IF target[reader] <= cursor THEN 1 ELSE -1]
    /\ UNCHANGED <<cursor, receipts, completed, target, receiptCursor,
                    receiptPresent, receiptComplete, lastCursor>>

ReadReceipt(reader) ==
    /\ phase[reader] = "receipt"
    /\ receiptCursor' = [receiptCursor EXCEPT ![reader] = cursor]
    /\ receiptPresent' = [receiptPresent EXCEPT ![reader] = target[reader] \in receipts]
    /\ receiptComplete' = [receiptComplete EXCEPT ![reader] =
                             target[reader] <= cursor \/ target[reader] \in receipts]
    /\ phase' = [phase EXCEPT ![reader] =
                   IF target[reader] \in receipts \/ Bug = "missing-recheck"
                   THEN "done" ELSE "recheck"]
    /\ answer' = [answer EXCEPT ![reader] =
                    IF target[reader] \in receipts THEN 1
                    ELSE IF Bug = "missing-recheck" THEN 0 ELSE -1]
    /\ UNCHANGED <<cursor, receipts, completed, target, firstCursor, lastCursor>>

ReadLast(reader) ==
    /\ phase[reader] = "recheck"
    /\ lastCursor' = [lastCursor EXCEPT ![reader] = cursor]
    /\ phase' = [phase EXCEPT ![reader] = "done"]
    /\ answer' = [answer EXCEPT ![reader] = IF target[reader] <= cursor THEN 1 ELSE 0]
    /\ UNCHANGED <<cursor, receipts, completed, target, firstCursor,
                    receiptCursor, receiptPresent, receiptComplete>>

ReadError(reader) ==
    /\ phase[reader] # "done"
    /\ phase' = [phase EXCEPT ![reader] = "done"]
    /\ answer' = [answer EXCEPT ![reader] = -2]
    /\ UNCHANGED <<cursor, receipts, completed, target, firstCursor,
                    receiptCursor, receiptPresent, receiptComplete, lastCursor>>

Next ==
    \/ \E round \in Rounds : Complete(round)
    \/ Advance
    \/ \E round \in Rounds : Compact(round)
    \/ \E reader \in Readers : ReadFirst(reader)
    \/ \E reader \in Readers : ReadReceipt(reader)
    \/ \E reader \in Readers : ReadLast(reader)
    \/ \E reader \in Readers : ReadError(reader)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ cursor \in 0..MaxRounds
    /\ receipts \in SUBSET Rounds
    /\ completed \in SUBSET Rounds
    /\ target \in [Readers -> Rounds]
    /\ phase \in [Readers -> {"first", "receipt", "recheck", "done"}]
    /\ firstCursor \in [Readers -> 0..MaxRounds]
    /\ receiptCursor \in [Readers -> 0..MaxRounds]
    /\ receiptPresent \in [Readers -> BOOLEAN]
    /\ receiptComplete \in [Readers -> BOOLEAN]
    /\ lastCursor \in [Readers -> 0..MaxRounds]
    /\ answer \in [Readers -> {-2, -1, 0, 1}]

Inv_LogicalCompletion == completed = Prefix(cursor) \cup receipts
Inv_SnapshotsBounded ==
    \A reader \in Readers :
        /\ firstCursor[reader] <= cursor
        /\ receiptCursor[reader] <= cursor
        /\ lastCursor[reader] <= cursor
Inv_ReceiptMeaning ==
    \A reader \in Readers : receiptComplete[reader] =
        (target[reader] <= receiptCursor[reader] \/ receiptPresent[reader])
Inv_RecheckReceipt == \A reader \in Readers :
    phase[reader] = "recheck" => ~receiptPresent[reader]
Inv_Pending == \A reader \in Readers : phase[reader] # "done" => answer[reader] = -1
Inv_FalseWitness == \A reader \in Readers : answer[reader] = 0 => ~receiptComplete[reader]
Inv_TrueWitness == \A reader \in Readers : answer[reader] = 1 =>
    (target[reader] <= firstCursor[reader] \/ receiptPresent[reader]
     \/ target[reader] <= lastCursor[reader])
Safety == TypeOK /\ Inv_LogicalCompletion /\ Inv_SnapshotsBounded
          /\ Inv_ReceiptMeaning /\ Inv_RecheckReceipt /\ Inv_Pending
          /\ Inv_FalseWitness /\ Inv_TrueWitness
=============================================================================

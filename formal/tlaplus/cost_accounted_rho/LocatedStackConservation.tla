---------------------- MODULE LocatedStackConservation ----------------------
EXTENDS Naturals, TLC

CONSTANTS StackCells, InitialSource, DuplicateSecondIdentity,
          AcceptDuplicateAsIdempotent, PartialBatchDebit, ReplayOmitsOneCell

ASSUME /\ StackCells \in Nat \ {0}
       /\ InitialSource \in Nat
       /\ DuplicateSecondIdentity \in BOOLEAN
       /\ AcceptDuplicateAsIdempotent \in BOOLEAN
       /\ PartialBatchDebit \in BOOLEAN
       /\ ReplayOmitsOneCell \in BOOLEAN

VARIABLES phase, attempts, source, target, certifiedCells,
          acceptedCount, rejectedCount, replaySource, replayTarget

vars == <<phase, attempts, source, target, certifiedCells,
          acceptedCount, rejectedCount, replaySource, replayTarget>>

DebitAmount == IF PartialBatchDebit /\ StackCells > 1
               THEN StackCells - 1
               ELSE StackCells

SecondIdentityCollides == attempts = 1 /\ DuplicateSecondIdentity

Init ==
  /\ phase = "Execute"
  /\ attempts = 0
  /\ source = InitialSource
  /\ target = 0
  /\ certifiedCells = 0
  /\ acceptedCount = 0
  /\ rejectedCount = 0
  /\ replaySource = InitialSource
  /\ replayTarget = 0

RejectAttempt ==
  /\ attempts' = attempts + 1
  /\ rejectedCount' = rejectedCount + 1
  /\ phase' = IF attempts' = 2 THEN "Replay" ELSE phase
  /\ UNCHANGED <<source, target, certifiedCells, acceptedCount,
                  replaySource, replayTarget>>

CommitAttempt ==
  /\ source >= StackCells
  /\ source' = source -
       IF SecondIdentityCollides /\ AcceptDuplicateAsIdempotent
       THEN 0
       ELSE DebitAmount
  /\ target' = target + StackCells
  /\ certifiedCells' =
       certifiedCells +
         IF SecondIdentityCollides /\ AcceptDuplicateAsIdempotent
         THEN 0
         ELSE DebitAmount
  /\ acceptedCount' = acceptedCount + 1
  /\ attempts' = attempts + 1
  /\ phase' = IF attempts' = 2 THEN "Replay" ELSE phase
  /\ UNCHANGED <<rejectedCount, replaySource, replayTarget>>

Attempt ==
  /\ phase = "Execute"
  /\ attempts < 2
  /\ IF SecondIdentityCollides /\ ~AcceptDuplicateAsIdempotent
        THEN RejectAttempt
        ELSE IF source < StackCells THEN RejectAttempt ELSE CommitAttempt

Replay ==
  /\ phase = "Replay"
  /\ LET replayCells ==
           IF ReplayOmitsOneCell /\ certifiedCells > 0
           THEN certifiedCells - 1
           ELSE certifiedCells
     IN /\ replaySource' = InitialSource - replayCells
        /\ replayTarget' = replayCells
  /\ phase' = "Done"
  /\ UNCHANGED <<attempts, source, target, certifiedCells,
                  acceptedCount, rejectedCount>>

Next == Attempt \/ Replay

Spec == /\ Init
        /\ [][Next]_vars
        /\ WF_vars(Attempt)
        /\ WF_vars(Replay)

TypeOK ==
  /\ phase \in {"Execute", "Replay", "Done"}
  /\ attempts \in 0..2
  /\ source \in Nat
  /\ target \in Nat
  /\ certifiedCells \in Nat
  /\ acceptedCount \in 0..2
  /\ rejectedCount \in 0..2
  /\ replaySource \in Nat
  /\ replayTarget \in Nat

UserStackProductionConserves == source + target = InitialSource

EveryProducedCellHasASettlementEvent == certifiedCells = target

DuplicateIdentityIsFailClosed ==
  DuplicateSecondIdentity /\ attempts = 2 /\ ~AcceptDuplicateAsIdempotent =>
    acceptedCount <= 1

ReplayMatchesCommittedTransfer ==
  phase = "Done" => /\ replaySource = source
                    /\ replayTarget = target

EveryAcceptedStackIsComplete == target = acceptedCount * StackCells

EventuallyDone == <>(phase = "Done")

=============================================================================

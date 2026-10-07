-------------------- MODULE NativeBudgetCapture --------------------
EXTENDS Naturals
CONSTANT PendingFirst
VARIABLES pending, invalid, abandoned, writer, reader, seenPending, seenInvalid, exported
vars == <<pending, invalid, abandoned, writer, reader, seenPending, seenInvalid, exported>>
Init ==
  /\ pending = TRUE
  /\ invalid = FALSE
  /\ abandoned \in BOOLEAN
  /\ writer = 0
  /\ reader = 0
  /\ seenPending = TRUE
  /\ seenInvalid = TRUE
  /\ exported = FALSE
Invalidate ==
  /\ writer = 0
  /\ invalid' = abandoned
  /\ writer' = 1
  /\ UNCHANGED <<pending, abandoned, reader, seenPending, seenInvalid, exported>>
DropTicket ==
  /\ writer = 1
  /\ pending' = FALSE
  /\ writer' = 2
  /\ UNCHANGED <<invalid, abandoned, reader, seenPending, seenInvalid, exported>>
ReadFirst ==
  /\ reader = 0
  /\ reader' = 1
  /\ IF PendingFirst THEN /\ seenPending' = pending /\ UNCHANGED seenInvalid
     ELSE /\ seenInvalid' = invalid /\ UNCHANGED seenPending
  /\ UNCHANGED <<pending, invalid, abandoned, writer, exported>>
ReadSecond ==
  /\ reader = 1
  /\ reader' = 2
  /\ IF PendingFirst THEN /\ seenInvalid' = invalid /\ UNCHANGED seenPending
     ELSE /\ seenPending' = pending /\ UNCHANGED seenInvalid
  /\ UNCHANGED <<pending, invalid, abandoned, writer, exported>>
Export ==
  /\ reader = 2
  /\ reader' = 3
  /\ exported' = ~seenPending /\ ~seenInvalid
  /\ UNCHANGED <<pending, invalid, abandoned, writer, seenPending, seenInvalid>>
Next == Invalidate \/ DropTicket \/ ReadFirst \/ ReadSecond \/ Export
Spec == Init /\ [][Next]_vars
NoAbandonedExport == exported => ~abandoned
NoPendingExport == exported => ~pending
=============================================================================

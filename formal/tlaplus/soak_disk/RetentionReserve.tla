----------------------------- MODULE RetentionReserve -----------------------------
EXTENDS Integers
CONSTANTS SkipUnderPressure, Copies, CopySize, Reserve, Required
VARIABLES free, done

vars == <<free, done>>

Init ==
    /\ free = Reserve
    /\ done = 0

Copy ==
    /\ done < Copies
    /\ done' = done + 1
    /\ IF SkipUnderPressure /\ (free - CopySize < Required)
        THEN free' = free
        ELSE free' = free - CopySize

Next == Copy \/ (done = Copies /\ UNCHANGED vars)
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ free \in (Reserve - Copies * CopySize) .. Reserve
    /\ done \in 0 .. Copies

ReserveHeld == free >= Required
=============================================================================

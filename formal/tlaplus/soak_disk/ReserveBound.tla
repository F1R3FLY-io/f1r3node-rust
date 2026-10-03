------------------------------ MODULE ReserveBound ------------------------------
EXTENDS Integers
CONSTANTS Bounded, Deadline, Growth, Burst, Reserve, Required, MaxSteps
VARIABLES free, step, burstUsed

vars == <<free, step, burstUsed>>

Init ==
    /\ free = Reserve
    /\ step = 0
    /\ burstUsed = FALSE

Grow ==
    /\ step < MaxSteps
    /\ (Bounded => step < Deadline)
    /\ free' = free - Growth
    /\ step' = step + 1
    /\ UNCHANGED burstUsed

ApplyBurst ==
    /\ ~burstUsed
    /\ (Bounded => step <= Deadline)
    /\ free' = free - Burst
    /\ burstUsed' = TRUE
    /\ UNCHANGED step

Next == Grow \/ ApplyBurst
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ free \in (Reserve - Growth * MaxSteps - Burst) .. Reserve
    /\ step \in 0 .. MaxSteps
    /\ burstUsed \in BOOLEAN

OperatingReserveHeld == free >= Required
=============================================================================

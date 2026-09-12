------------------------ MODULE DiskProbeDeadline ------------------------
EXTENDS Naturals, TLC

CONSTANT EnforceTimeout
VARIABLES phase, elapsed, timedOut, known, breachRecorded
vars == <<phase, elapsed, timedOut, known, breachRecorded>>

Init ==
    /\ phase = "probe"
    /\ elapsed = 0
    /\ timedOut = FALSE
    /\ known = FALSE
    /\ breachRecorded = FALSE

Tick ==
    /\ phase = "probe"
    /\ elapsed < 4
    /\ elapsed' = elapsed + 1
    /\ timedOut' = (EnforceTimeout /\ elapsed' = 3)
    /\ phase' = IF timedOut' THEN "decide" ELSE "probe"
    /\ UNCHANGED <<known, breachRecorded>>

ProbeReturns ==
    /\ phase = "probe"
    /\ elapsed = 4
    /\ phase' = "decide"
    /\ UNCHANGED <<elapsed, timedOut, known, breachRecorded>>

Decide ==
    /\ phase = "decide"
    /\ known' = ~timedOut
    /\ breachRecorded' = timedOut
    /\ phase' = "done"
    /\ UNCHANGED <<elapsed, timedOut>>

Next == Tick \/ ProbeReturns \/ Decide
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"probe", "decide", "done"}
    /\ elapsed \in 0..4
    /\ timedOut \in BOOLEAN
    /\ known \in BOOLEAN
    /\ breachRecorded \in BOOLEAN

ProbeWithinDeadline == phase = "probe" => elapsed < 3
TimedOutSampleRejected == (phase = "done" /\ timedOut) => (~known /\ breachRecorded)
Completes == <>(phase = "done")
=============================================================================

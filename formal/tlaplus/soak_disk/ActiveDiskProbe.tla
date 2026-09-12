------------------------- MODULE ActiveDiskProbe -------------------------
EXTENDS TLC

CONSTANT RejectUnavailable
VARIABLES phase, known, interruptRequested, breachRecorded

vars == <<phase, known, interruptRequested, breachRecorded>>

Init ==
    /\ phase = "probe"
    /\ known = TRUE
    /\ interruptRequested = FALSE
    /\ breachRecorded = FALSE

Probe ==
    /\ phase = "probe"
    /\ known' \in BOOLEAN
    /\ phase' = "decide"
    /\ UNCHANGED <<interruptRequested, breachRecorded>>

Decide ==
    /\ phase = "decide"
    /\ interruptRequested' = (RejectUnavailable /\ ~known)
    /\ breachRecorded' = interruptRequested'
    /\ phase' = "decided"
    /\ UNCHANGED known

Next == Probe \/ Decide
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"probe", "decide", "decided"}
    /\ known \in BOOLEAN
    /\ interruptRequested \in BOOLEAN
    /\ breachRecorded \in BOOLEAN

InvalidSampleRequiresInterrupt ==
    (phase = "decided" /\ ~known) => (interruptRequested /\ breachRecorded)

Completes == <>(phase = "decided")
=============================================================================

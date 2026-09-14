----------------------- MODULE DiskEmergencyRecord -----------------------
EXTENDS Naturals, TLC

CONSTANT RecordFirst
VARIABLES phase, freeMiB, recorded, stopStarted
vars == <<phase, freeMiB, recorded, stopStarted>>

Init ==
    /\ phase = "watch"
    /\ freeMiB = 8192
    /\ recorded = FALSE
    /\ stopStarted = FALSE

ExternalWrite ==
    /\ freeMiB' \in {value \in {0, 1024, 8192} : value <= freeMiB}
    /\ UNCHANGED <<phase, recorded, stopStarted>>

Detect ==
    /\ phase = "watch"
    /\ freeMiB < 2048
    /\ phase' = IF RecordFirst THEN "record" ELSE "stop"
    /\ UNCHANGED <<freeMiB, recorded, stopStarted>>

Record ==
    /\ phase = "record"
    /\ recorded' = TRUE
    /\ phase' = "stop"
    /\ UNCHANGED <<freeMiB, stopStarted>>

BeginStop ==
    /\ phase = "stop"
    /\ stopStarted' = TRUE
    /\ phase' = "waiting"
    /\ UNCHANGED <<freeMiB, recorded>>

StopReturns ==
    /\ phase = "waiting"
    /\ phase' = "finished"
    /\ UNCHANGED <<freeMiB, recorded, stopStarted>>

PublishLate ==
    /\ phase = "finished"
    /\ ~recorded
    /\ recorded' = TRUE
    /\ UNCHANGED <<phase, freeMiB, stopStarted>>

Next == ExternalWrite \/ Detect \/ Record \/ BeginStop \/ StopReturns \/ PublishLate
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"watch", "record", "stop", "waiting", "finished"}
    /\ freeMiB \in {0, 1024, 8192}
    /\ recorded \in BOOLEAN
    /\ stopStarted \in BOOLEAN

StopRequiresRecord == stopStarted => recorded
=============================================================================

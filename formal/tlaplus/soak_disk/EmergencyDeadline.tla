--------------------------- MODULE EmergencyDeadline ---------------------------
EXTENDS Naturals

CONSTANTS Roots, Deadline, ComposedDeadline
VARIABLES phase, recorded, copied, skipped, elapsed, stalled

vars == <<phase, recorded, copied, skipped, elapsed, stalled>>

Init ==
    /\ phase = "breach"
    /\ recorded = FALSE
    /\ copied = 0
    /\ skipped = 0
    /\ elapsed = 0
    /\ stalled \in BOOLEAN

Record ==
    /\ phase = "breach"
    /\ recorded' = TRUE
    /\ phase' = "copying"
    /\ UNCHANGED <<copied, skipped, elapsed, stalled>>

CopyRoot ==
    /\ phase = "copying"
    /\ copied + skipped < Roots
    /\ ~stalled
    /\ copied' = copied + 1
    /\ elapsed' = elapsed + 1
    /\ UNCHANGED <<phase, recorded, skipped, stalled>>

StallRoot ==
    /\ phase = "copying"
    /\ copied + skipped < Roots
    /\ stalled
    /\ ~ComposedDeadline \/ elapsed < Deadline
    /\ elapsed' = elapsed + 1
    /\ UNCHANGED <<phase, recorded, copied, skipped, stalled>>

SkipRoot ==
    /\ phase = "copying"
    /\ copied + skipped < Roots
    /\ ComposedDeadline
    /\ elapsed >= Deadline
    /\ skipped' = skipped + 1
    /\ UNCHANGED <<phase, recorded, copied, elapsed, stalled>>

Publish ==
    /\ phase = "copying"
    /\ copied + skipped = Roots
    /\ phase' = "published"
    /\ UNCHANGED <<recorded, copied, skipped, elapsed, stalled>>

Next == Record \/ CopyRoot \/ StallRoot \/ SkipRoot \/ Publish
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"breach", "copying", "published"}
    /\ recorded \in BOOLEAN
    /\ copied \in 0..Roots
    /\ skipped \in 0..Roots
    /\ elapsed \in Nat
    /\ stalled \in BOOLEAN

ResponseWithinDeadline == elapsed <= Deadline
RecordBeforeCopy == phase /= "breach" => recorded
Publishes == <>(phase = "published")
=============================================================================

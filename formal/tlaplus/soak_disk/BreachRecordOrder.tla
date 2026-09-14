--------------------------- MODULE BreachRecordOrder ---------------------------
CONSTANT RecordFirst
VARIABLES phase, recorded, attributionStarted, stalled

vars == <<phase, recorded, attributionStarted, stalled>>

Init ==
    /\ phase = "breach"
    /\ recorded = FALSE
    /\ attributionStarted = FALSE
    /\ stalled \in BOOLEAN

Record ==
    /\ ~recorded
    /\ IF RecordFirst THEN phase = "breach" ELSE phase = "attributed"
    /\ recorded' = TRUE
    /\ phase' = IF RecordFirst THEN "recorded" ELSE "done"
    /\ UNCHANGED <<attributionStarted, stalled>>

Attribute ==
    /\ ~attributionStarted
    /\ IF RecordFirst THEN phase = "recorded" ELSE phase = "breach"
    /\ attributionStarted' = TRUE
    /\ phase' = "attributing"
    /\ UNCHANGED <<recorded, stalled>>

Finish ==
    /\ phase = "attributing"
    /\ ~stalled
    /\ phase' = IF recorded THEN "done" ELSE "attributed"
    /\ UNCHANGED <<recorded, attributionStarted, stalled>>

Next == Record \/ Attribute \/ Finish
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"breach", "recorded", "attributing", "attributed", "done"}
    /\ recorded \in BOOLEAN
    /\ attributionStarted \in BOOLEAN
    /\ stalled \in BOOLEAN

AttributionRequiresRecord == attributionStarted => recorded
RecordPublished == <>recorded
=============================================================================

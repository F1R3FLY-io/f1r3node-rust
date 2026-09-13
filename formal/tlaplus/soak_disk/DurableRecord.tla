----------------------------- MODULE DurableRecord -----------------------------
CONSTANT AtomicPublish
VARIABLES visible, temp

vars == <<visible, temp>>

Init ==
    /\ visible = "none"
    /\ temp = "none"

WriteTempPartial ==
    /\ AtomicPublish
    /\ temp = "none"
    /\ temp' = "partial"
    /\ UNCHANGED visible

WriteTempFull ==
    /\ AtomicPublish
    /\ temp = "partial"
    /\ temp' = "full"
    /\ UNCHANGED visible

Rename ==
    /\ AtomicPublish
    /\ temp = "full"
    /\ visible' = "full"
    /\ temp' = "none"

WriteInPlacePartial ==
    /\ ~AtomicPublish
    /\ visible = "none"
    /\ visible' = "partial"
    /\ UNCHANGED temp

WriteInPlaceFull ==
    /\ ~AtomicPublish
    /\ visible = "partial"
    /\ visible' = "full"
    /\ UNCHANGED temp

Next == WriteTempPartial \/ WriteTempFull \/ Rename \/ WriteInPlacePartial \/ WriteInPlaceFull
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ visible \in {"none", "partial", "full"}
    /\ temp \in {"none", "partial", "full"}

VisibleImpliesComplete == visible \in {"none", "full"}
Publishes == <>(visible = "full")
=============================================================================

----------------------------- MODULE DurableRecord -----------------------------
CONSTANT AtomicPublish
VARIABLES visible, synced, temp, tempSynced, dirSynced, crashed

vars == <<visible, synced, temp, tempSynced, dirSynced, crashed>>

Init ==
    /\ visible = "none"
    /\ synced = FALSE
    /\ temp = "none"
    /\ tempSynced = FALSE
    /\ dirSynced = FALSE
    /\ crashed = FALSE

WritePartial ==
    /\ ~crashed
    /\ visible = "none"
    /\ temp = "none"
    /\ IF AtomicPublish
        THEN temp' = "partial" /\ UNCHANGED visible
        ELSE visible' = "partial" /\ UNCHANGED temp
    /\ UNCHANGED <<synced, tempSynced, dirSynced, crashed>>

WriteFull ==
    /\ ~crashed
    /\ IF AtomicPublish THEN temp = "partial" ELSE visible = "partial"
    /\ IF AtomicPublish
        THEN temp' = "full" /\ UNCHANGED visible
        ELSE visible' = "full" /\ UNCHANGED temp
    /\ UNCHANGED <<synced, tempSynced, dirSynced, crashed>>

SyncFile ==
    /\ ~crashed
    /\ AtomicPublish
    /\ temp = "full"
    /\ ~tempSynced
    /\ tempSynced' = TRUE
    /\ UNCHANGED <<visible, synced, temp, dirSynced, crashed>>

Rename ==
    /\ ~crashed
    /\ AtomicPublish
    /\ tempSynced
    /\ visible' = "full"
    /\ synced' = TRUE
    /\ temp' = "none"
    /\ tempSynced' = FALSE
    /\ UNCHANGED <<dirSynced, crashed>>

SyncDir ==
    /\ ~crashed
    /\ AtomicPublish
    /\ visible = "full"
    /\ ~dirSynced
    /\ dirSynced' = TRUE
    /\ UNCHANGED <<visible, synced, temp, tempSynced, crashed>>

Crash ==
    /\ ~crashed
    /\ crashed' = TRUE
    /\ visible' \in (IF dirSynced THEN {visible} ELSE {"none", visible})
    /\ temp' = "none"
    /\ tempSynced' = FALSE
    /\ UNCHANGED <<synced, dirSynced>>

Next == WritePartial \/ WriteFull \/ SyncFile \/ Rename \/ SyncDir \/ Crash
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ visible \in {"none", "partial", "full"}
    /\ synced \in BOOLEAN
    /\ temp \in {"none", "partial", "full"}
    /\ tempSynced \in BOOLEAN
    /\ dirSynced \in BOOLEAN
    /\ crashed \in BOOLEAN

VisibleImpliesDurable == visible = "none" \/ (visible = "full" /\ synced)
NoTemporaryAfterPublish == dirSynced => temp = "none"
Settles == <>(crashed \/ dirSynced)
=============================================================================

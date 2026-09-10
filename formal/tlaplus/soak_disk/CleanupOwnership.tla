---------------------- MODULE CleanupOwnership ----------------------
EXTENDS TLC
CONSTANT SweepAged
VARIABLES age, present, phase, ownerKnown, writerAlive
vars == <<age, present, phase, ownerKnown, writerAlive>>
Init ==
    /\ age \in {"old", "recent"}
    /\ present = TRUE
    /\ phase = "hygiene"
    /\ ownerKnown = FALSE
    /\ writerAlive = TRUE
Sweep ==
    /\ phase = "hygiene"
    /\ present' = ~(SweepAged /\ age = "old")
    /\ phase' = "checked"
    /\ UNCHANGED <<age, ownerKnown, writerAlive>>
Spec == Init /\ [][Sweep]_vars /\ WF_vars(Sweep)
TypeOK ==
    /\ age \in {"old", "recent"}
    /\ present \in BOOLEAN
    /\ phase \in {"hygiene", "checked"}
    /\ ownerKnown = FALSE
    /\ writerAlive = TRUE
UnownedSessionPreserved == ~ownerKnown => present
Completes == <>(phase = "checked")
=============================================================================

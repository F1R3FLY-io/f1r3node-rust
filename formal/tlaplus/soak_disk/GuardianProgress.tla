---------------------- MODULE GuardianProgress ----------------------
EXTENDS Naturals, TLC
CONSTANTS CheckProgress, SilenceTicks
VARIABLES age, phase, guardianAlive, interrupted
vars == <<age, phase, guardianAlive, interrupted>>

Init ==
    /\ age = 0
    /\ phase = "waiting"
    /\ guardianAlive = TRUE
    /\ interrupted = FALSE

Tick ==
    /\ phase = "waiting"
    /\ age <= SilenceTicks
    /\ age' = age + 1
    /\ UNCHANGED <<phase, guardianAlive, interrupted>>

Observe ==
    /\ phase = "waiting"
    /\ age = SilenceTicks + 1
    /\ interrupted' = CheckProgress
    /\ phase' = "checked"
    /\ UNCHANGED <<age, guardianAlive>>

Next == Tick \/ Observe
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK ==
    /\ age \in 0..(SilenceTicks + 1)
    /\ phase \in {"waiting", "checked"}
    /\ guardianAlive = TRUE
    /\ interrupted \in BOOLEAN
StaleGuardianRequiresInterrupt == phase = "checked" => interrupted
Completes == <>(phase = "checked")
=============================================================================

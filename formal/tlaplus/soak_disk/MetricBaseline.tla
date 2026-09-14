---- MODULE MetricBaseline ----
EXTENDS Naturals

CONSTANTS RequireBaseline, BaselineAvailable
VARIABLES phase, delta, hasDelta

vars == <<phase, delta, hasDelta>>

Init == /\ phase = "sampled"
        /\ delta = 0
        /\ hasDelta = FALSE

Compute == /\ phase = "sampled"
           /\ hasDelta' = (BaselineAvailable \/ ~RequireBaseline)
           /\ delta' = IF BaselineAvailable THEN 8
                        ELSE IF RequireBaseline THEN 0 ELSE 12
           /\ phase' = "retained"

Spec == Init /\ [][Compute]_vars

TypeOK == /\ phase \in {"sampled", "retained"}
          /\ delta \in {0, 8, 12}
          /\ hasDelta \in BOOLEAN

MissingBaselineUnavailable ==
    phase = "retained" /\ ~BaselineAvailable => ~hasDelta

AvailableBaselineComputesDelta ==
    phase = "retained" /\ BaselineAvailable => hasDelta /\ delta = 8
====

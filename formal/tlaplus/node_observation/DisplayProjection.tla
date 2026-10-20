---- MODULE DisplayProjection ----
EXTENDS Naturals, FiniteSets
CONSTANT Bug
VARIABLES phase, captured, dagTxn, trackerTxn, finalized, original, missing, base, result
vars == <<phase, captured, dagTxn, trackerTxn, finalized, original, missing, base, result>>
Init == /\ phase = "idle" /\ captured = FALSE
        /\ dagTxn = 0 /\ trackerTxn = 0
        /\ finalized = FALSE /\ original = FALSE /\ missing = FALSE
        /\ base = "none" /\ result = "pending"
Capture(c, f, o, m) ==
    /\ phase = "idle" /\ phase' = "derive"
    /\ captured' = c /\ finalized' = f /\ original' = o /\ missing' = m
    /\ dagTxn' = 1 /\ trackerTxn' = IF Bug = "interval" THEN 2 ELSE 1
    /\ UNCHANGED <<base, result>>
CorrectBase == IF finalized THEN "persisted_metadata"
               ELSE IF original THEN "original_oracle"
               ELSE IF missing THEN "missing_history_minimum"
               ELSE "none"
Derive == /\ phase = "derive" /\ phase' = "done"
          /\ base' = IF Bug = "source" /\ finalized THEN "original_oracle" ELSE CorrectBase
          /\ result' = IF (captured /\ CorrectBase # "none") \/ Bug = "fabricated"
                        THEN "available" ELSE "unavailable"
          /\ UNCHANGED <<captured, dagTxn, trackerTxn, finalized, original, missing>>
Next == \/ (\E c, f, o, m \in BOOLEAN: Capture(c, f, o, m)) \/ Derive
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase \in {"idle", "derive", "done"}
          /\ captured \in BOOLEAN /\ finalized \in BOOLEAN /\ original \in BOOLEAN /\ missing \in BOOLEAN
          /\ dagTxn \in 0..2 /\ trackerTxn \in 0..2
          /\ base \in {"none", "persisted_metadata", "original_oracle", "missing_history_minimum"}
          /\ result \in {"pending", "available", "unavailable"}
NoFabrication == result = "available" => captured /\ CorrectBase # "none"
BaseSource == result = "available" => base = CorrectBase
OneInterval == phase # "idle" => dagTxn = trackerTxn
====

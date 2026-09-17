------------------------- MODULE MetricMonotonicity -------------------------
EXTENDS Naturals, Sequences

CONSTANT ValidateMonotonicity
VARIABLES phase, before, after, invalid, hasDelta
vars == <<phase, before, after, invalid, hasDelta>>

Init ==
    /\ phase = "ready"
    /\ before = <<>>
    /\ after = <<>>
    /\ invalid = FALSE
    /\ hasDelta = FALSE

Capture ==
    /\ phase = "ready"
    /\ \/ /\ before' = <<12>>
          /\ after' = <<4>>
       \/ /\ before' = <<10, 8>>
          /\ after' = <<2, 9>>
       \/ /\ before' = <<1, 6>>
          /\ after' = <<2, 2>>
       \/ /\ before' = <<4>>
          /\ after' = <<12>>
       \/ /\ before' = <<1, 2>>
          /\ after' = <<2, 6>>
       \/ /\ before' = <<4>>
          /\ after' = <<4>>
       \/ /\ before' = <<1, 2>>
          /\ after' = <<1, 2>>
    /\ phase' = "captured"
    /\ UNCHANGED <<invalid, hasDelta>>

Decreased == \E i \in DOMAIN before : after[i] < before[i]

Publish ==
    /\ phase = "captured"
    /\ invalid' = (ValidateMonotonicity /\ Decreased)
    /\ hasDelta' = (~ValidateMonotonicity \/ ~Decreased)
    /\ phase' = "retained"
    /\ UNCHANGED <<before, after>>

Next == Capture \/ Publish
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"ready", "captured", "retained"}
    /\ before \in Seq(0..12)
    /\ after \in Seq(0..12)
    /\ DOMAIN before = DOMAIN after
    /\ invalid \in BOOLEAN
    /\ hasDelta \in BOOLEAN

DecreasingSamplesUnavailable ==
    (phase = "retained" /\ Decreased) => (invalid /\ ~hasDelta)

NondecreasingSamplesAvailable ==
    (phase = "retained" /\ ~Decreased) => (~invalid /\ hasDelta)
=============================================================================

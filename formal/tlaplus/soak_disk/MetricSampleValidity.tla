------------------------- MODULE MetricSampleValidity -------------------------
EXTENDS TLC

CONSTANT ValidateValues
VARIABLES phase, sample, reason, hasDelta
vars == <<phase, sample, reason, hasDelta>>

Nonfinite == {"nonfinite-before", "nonfinite-after"}
Negative == {"negative-before", "negative-after"}
InvalidSamples == Nonfinite \cup Negative
Samples == InvalidSamples \cup {"valid"}
LegacyDecreased == {"nonfinite-before", "negative-after"}

Init ==
    /\ phase = "ready"
    /\ sample = "unselected"
    /\ reason = "none"
    /\ hasDelta = FALSE

Capture ==
    /\ phase = "ready"
    /\ sample' \in Samples
    /\ phase' = "captured"
    /\ UNCHANGED <<reason, hasDelta>>

SampleReason ==
    CASE sample \in Nonfinite -> "nonfinite_sample"
      [] sample \in Negative -> "negative_sample"
      [] OTHER -> "none"

Publish ==
    /\ phase = "captured"
    /\ reason' = IF ValidateValues THEN SampleReason
                  ELSE IF sample \in LegacyDecreased THEN "cumulative_decrease"
                       ELSE "none"
    /\ hasDelta' = (reason' = "none")
    /\ phase' = "retained"
    /\ UNCHANGED sample

Next == Capture \/ Publish
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"ready", "captured", "retained"}
    /\ sample \in Samples \cup {"unselected"}
    /\ reason \in {"none", "cumulative_decrease", "nonfinite_sample", "negative_sample"}
    /\ hasDelta \in BOOLEAN

InvalidSamplesRejected ==
    (phase = "retained" /\ sample \in InvalidSamples) =>
        (~hasDelta /\ reason = SampleReason)

ValidSamplesAvailable ==
    (phase = "retained" /\ sample = "valid") => (hasDelta /\ reason = "none")
=============================================================================

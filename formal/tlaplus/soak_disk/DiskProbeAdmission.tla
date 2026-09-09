------------------------- MODULE DiskProbeAdmission -------------------------
EXTENDS Naturals, TLC

CONSTANTS FloorMiB, BandMiB, ValidSamples, RejectMissing

ASSUME /\ FloorMiB \in Nat \ {0}
       /\ BandMiB \in Nat
       /\ ValidSamples \subseteq Nat
       /\ RejectMissing \in BOOLEAN

VARIABLES phase, sample, admitted, admissionSample, stopReason, evidence

vars == <<phase, sample, admitted, admissionSample, stopReason, evidence>>
MissingSample == [known |-> FALSE, freeMiB |-> 0]
Samples == {[known |-> TRUE, freeMiB |-> value] : value \in ValidSamples} \cup {MissingSample}
BelowBand == IF sample.known THEN sample.freeMiB < FloorMiB + BandMiB ELSE FALSE

Init ==
    /\ phase = "boundary-probe"
    /\ sample = MissingSample
    /\ admitted = FALSE
    /\ admissionSample = MissingSample
    /\ stopReason = "none"
    /\ evidence = FALSE

ProbeBoundary ==
    /\ phase = "boundary-probe"
    /\ sample' \in Samples
    /\ phase' = "boundary-decision"
    /\ UNCHANGED <<admitted, admissionSample, stopReason, evidence>>

DecideHygiene ==
    /\ phase = "boundary-decision"
    /\ phase' = IF BelowBand THEN "hygiene" ELSE "admission-check"
    /\ UNCHANGED <<sample, admitted, admissionSample, stopReason, evidence>>

Hygiene ==
    /\ phase = "hygiene"
    /\ phase' = "post-probe"
    /\ UNCHANGED <<sample, admitted, admissionSample, stopReason, evidence>>

ProbeAfterHygiene ==
    /\ phase = "post-probe"
    /\ sample' \in Samples
    /\ phase' = "post-decision"
    /\ UNCHANGED <<admitted, admissionSample, stopReason, evidence>>

DecideAfterHygiene ==
    /\ phase = "post-decision"
    /\ IF BelowBand
          THEN /\ phase' = "stopped"
               /\ stopReason' = "disk"
          ELSE /\ phase' = "admission-check"
               /\ UNCHANGED stopReason
    /\ UNCHANGED <<sample, admitted, admissionSample, evidence>>

CheckAdmission ==
    /\ phase = "admission-check"
    /\ IF RejectMissing /\ ~sample.known
          THEN /\ phase' = "stopped"
               /\ stopReason' = "probe"
          ELSE /\ phase' = "admit"
               /\ UNCHANGED stopReason
    /\ UNCHANGED <<sample, admitted, admissionSample, evidence>>

Admit ==
    /\ phase = "admit"
    /\ admitted' = TRUE
    /\ admissionSample' = sample
    /\ phase' = "running"
    /\ UNCHANGED <<sample, stopReason, evidence>>

PublishRefusal ==
    /\ phase = "stopped"
    /\ evidence' = TRUE
    /\ phase' = "done"
    /\ UNCHANGED <<sample, admitted, admissionSample, stopReason>>

Next == ProbeBoundary \/ DecideHygiene \/ Hygiene \/ ProbeAfterHygiene
        \/ DecideAfterHygiene \/ CheckAdmission \/ Admit \/ PublishRefusal

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"boundary-probe", "boundary-decision", "hygiene", "post-probe",
                  "post-decision", "admission-check", "admit", "running", "stopped", "done"}
    /\ sample \in Samples
    /\ admitted \in BOOLEAN
    /\ admissionSample \in Samples
    /\ stopReason \in {"none", "disk", "probe"}
    /\ evidence \in BOOLEAN

AdmissionRequiresSample == admitted => admissionSample.known
AdmissionRequiresBand == admitted =>
    IF admissionSample.known THEN admissionSample.freeMiB >= FloorMiB + BandMiB ELSE TRUE
StopPreventsAdmission == stopReason # "none" => ~admitted
RefusalRecorded == phase = "done" => evidence /\ stopReason # "none" /\ ~admitted
Completes == <>(admitted \/ (phase = "done" /\ evidence))
=============================================================================

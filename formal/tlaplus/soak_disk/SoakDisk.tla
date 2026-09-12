----------------------------- MODULE SoakDisk -----------------------------
EXTENDS Naturals, TLC

CONSTANTS FloorMiB, BandMiB, FreeSamples, InitialFreeMiB, RequireBand

ASSUME /\ FloorMiB \in Nat \ {0}
       /\ BandMiB \in Nat
       /\ FreeSamples \subseteq Nat
       /\ InitialFreeMiB \in FreeSamples
       /\ RequireBand \in BOOLEAN

VARIABLES phase, free, sample, guardian, admitted, admissionSample,
          stopReason, evidence

vars == <<phase, free, sample, guardian, admitted, admissionSample,
          stopReason, evidence>>

Init ==
    /\ phase = "guard"
    /\ free = InitialFreeMiB
    /\ sample = InitialFreeMiB
    /\ guardian = FALSE
    /\ admitted = FALSE
    /\ admissionSample = 0
    /\ stopReason = "none"
    /\ evidence = FALSE

CheckGuardian ==
    /\ phase \in {"guard", "post-guard"}
    /\ IF guardian
          THEN /\ phase' = "stopped"
               /\ stopReason' = "guardian"
          ELSE /\ phase' = IF phase = "guard" THEN "probe" ELSE "post-probe"
               /\ UNCHANGED stopReason
    /\ UNCHANGED <<free, sample, guardian, admitted, admissionSample, evidence>>

ProbeBoundary ==
    /\ phase = "probe"
    /\ sample' = free
    /\ phase' = IF free < FloorMiB + BandMiB THEN "hygiene" ELSE "admit"
    /\ UNCHANGED <<free, guardian, admitted, admissionSample, stopReason, evidence>>

Hygiene ==
    /\ phase = "hygiene"
    /\ free' \in {value \in FreeSamples : value >= free}
    /\ phase' = "post-guard"
    /\ UNCHANGED <<sample, guardian, admitted, admissionSample, stopReason, evidence>>

ProbeAfterHygiene ==
    /\ phase = "post-probe"
    /\ sample' = free
    /\ phase' = "decide"
    /\ UNCHANGED <<free, guardian, admitted, admissionSample, stopReason, evidence>>

Decide ==
    /\ phase = "decide"
    /\ IF sample < (IF RequireBand THEN FloorMiB + BandMiB ELSE FloorMiB)
          THEN /\ phase' = "stopped"
               /\ stopReason' = "disk"
          ELSE /\ phase' = "admit"
               /\ UNCHANGED stopReason
    /\ UNCHANGED <<free, sample, guardian, admitted, admissionSample, evidence>>

Admit ==
    /\ phase = "admit"
    /\ admitted' = TRUE
    /\ admissionSample' = sample
    /\ phase' = "running"
    /\ UNCHANGED <<free, sample, guardian, stopReason, evidence>>

GuardianTrip ==
    /\ phase \notin {"running", "stopped", "done"}
    /\ ~guardian
    /\ guardian' = TRUE
    /\ UNCHANGED <<phase, free, sample, admitted, admissionSample, stopReason, evidence>>

PublishRefusal ==
    /\ phase = "stopped"
    /\ evidence' = TRUE
    /\ phase' = "done"
    /\ UNCHANGED <<free, sample, guardian, admitted, admissionSample, stopReason>>

Next == CheckGuardian \/ ProbeBoundary \/ Hygiene \/ ProbeAfterHygiene
        \/ Decide \/ Admit \/ GuardianTrip \/ PublishRefusal

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"guard", "probe", "hygiene", "post-guard", "post-probe",
                  "decide", "admit", "running", "stopped", "done"}
    /\ free \in FreeSamples
    /\ sample \in FreeSamples
    /\ guardian \in BOOLEAN
    /\ admitted \in BOOLEAN
    /\ admissionSample \in FreeSamples \cup {0}
    /\ stopReason \in {"none", "disk", "guardian"}
    /\ evidence \in BOOLEAN

AdmissionRequiresBand == admitted => admissionSample >= FloorMiB + BandMiB
StopPreventsAdmission == stopReason # "none" => ~admitted
RefusalRecorded == phase = "done" => evidence /\ stopReason # "none" /\ ~admitted
Completes == <>(admitted \/ (phase = "done" /\ evidence))
=============================================================================

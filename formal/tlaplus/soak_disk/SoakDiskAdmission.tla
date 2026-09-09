-------------------------- MODULE SoakDiskAdmission --------------------------
(* One iteration-boundary disk admission decision in                         *)
(* scripts/run-merge-recovery-soak.sh: probe, optional hygiene, re-probe,    *)
(* decide. Three constants switch the three admission corrections on and     *)
(* off so that each pre-fix configuration reproduces one historical defect.  *)
EXTENDS Naturals, TLC

CONSTANTS FloorMiB, BandMiB, FreeSamples, InitialFreeMiB, MalformedPrefixMiB,
          RequireBand,     \* post-hygiene refusal compares against floor + band
          RejectMissing,   \* a probe that returns nothing cannot admit
          RejectMalformed  \* a field such as 16384junk cannot admit

ASSUME /\ FloorMiB \in Nat \ {0}
       /\ BandMiB \in Nat
       /\ FreeSamples \subseteq Nat
       /\ InitialFreeMiB \in FreeSamples
       /\ MalformedPrefixMiB \in Nat
       /\ {RequireBand, RejectMissing, RejectMalformed} \subseteq BOOLEAN

Threshold == IF RequireBand THEN FloorMiB + BandMiB ELSE FloorMiB

MissingRaw   == [kind |-> "missing", mib |-> 0]
MalformedRaw == [kind |-> "malformed", mib |-> MalformedPrefixMiB]
RawSamples   == {[kind |-> "valid", mib |-> v] : v \in FreeSamples}
                \cup {MissingRaw, MalformedRaw}

Unknown       == [known |-> FALSE, mib |-> 0]
ParsedSamples == {[known |-> TRUE, mib |-> v] :
                    v \in FreeSamples \cup {MalformedPrefixMiB}} \cup {Unknown}

\* disk_free_mb: the digit check rejects a malformed field only under RejectMalformed.
Parse(r) ==
    CASE r.kind = "valid"     -> [known |-> TRUE, mib |-> r.mib]
      [] r.kind = "malformed" -> IF RejectMalformed THEN Unknown
                                 ELSE [known |-> TRUE, mib |-> r.mib]
      [] r.kind = "missing"   -> Unknown

VARIABLES phase, free, raw, sample, guardian, admitted,
          admissionRaw, admissionSample, stopReason, evidence

vars == <<phase, free, raw, sample, guardian, admitted,
          admissionRaw, admissionSample, stopReason, evidence>>

Init ==
    /\ phase = "guard"
    /\ free = InitialFreeMiB
    /\ raw = MissingRaw
    /\ sample = Unknown
    /\ guardian = FALSE
    /\ admitted = FALSE
    /\ admissionRaw = MissingRaw
    /\ admissionSample = Unknown
    /\ stopReason = "none"
    /\ evidence = FALSE

\* df either reports the true free space, prints a malformed field, or fails.
Probe(next) ==
    /\ raw' \in {[kind |-> "valid", mib |-> free], MissingRaw, MalformedRaw}
    /\ sample' = Parse(raw')
    /\ phase' = next

BelowBand == sample.known /\ sample.mib < FloorMiB + BandMiB

CheckGuardian ==
    /\ phase \in {"guard", "post-guard"}
    /\ IF guardian
          THEN /\ phase' = "stopped"
               /\ stopReason' = "guardian"
          ELSE /\ phase' = IF phase = "guard" THEN "probe" ELSE "post-probe"
               /\ UNCHANGED stopReason
    /\ UNCHANGED <<free, raw, sample, guardian, admitted,
                   admissionRaw, admissionSample, evidence>>

ProbeBoundary ==
    /\ phase = "probe"
    /\ Probe("boundary-decide")
    /\ UNCHANGED <<free, guardian, admitted, admissionRaw, admissionSample,
                   stopReason, evidence>>

DecideHygiene ==
    /\ phase = "boundary-decide"
    /\ phase' = IF BelowBand THEN "hygiene" ELSE "admission-check"
    /\ UNCHANGED <<free, raw, sample, guardian, admitted,
                   admissionRaw, admissionSample, stopReason, evidence>>

\* reclaim_disk_space never loses space and does not always recover any.
Hygiene ==
    /\ phase = "hygiene"
    /\ free' \in {v \in FreeSamples : v >= free}
    /\ phase' = "post-guard"
    /\ UNCHANGED <<raw, sample, guardian, admitted,
                   admissionRaw, admissionSample, stopReason, evidence>>

ProbeAfterHygiene ==
    /\ phase = "post-probe"
    /\ Probe("post-decide")
    /\ UNCHANGED <<free, guardian, admitted, admissionRaw, admissionSample,
                   stopReason, evidence>>

DecideAfterHygiene ==
    /\ phase = "post-decide"
    /\ IF sample.known /\ sample.mib < Threshold
          THEN /\ phase' = "stopped"
               /\ stopReason' = "disk"
          ELSE /\ phase' = "admission-check"
               /\ UNCHANGED stopReason
    /\ UNCHANGED <<free, raw, sample, guardian, admitted,
                   admissionRaw, admissionSample, evidence>>

CheckAdmission ==
    /\ phase = "admission-check"
    /\ IF RejectMissing /\ ~sample.known
          THEN /\ phase' = "stopped"
               /\ stopReason' = "probe"
          ELSE /\ phase' = "admit"
               /\ UNCHANGED stopReason
    /\ UNCHANGED <<free, raw, sample, guardian, admitted,
                   admissionRaw, admissionSample, evidence>>

Admit ==
    /\ phase = "admit"
    /\ admitted' = TRUE
    /\ admissionRaw' = raw
    /\ admissionSample' = sample
    /\ phase' = "running"
    /\ UNCHANGED <<free, raw, sample, guardian, stopReason, evidence>>

\* The guardian marker can appear at any point before the workload starts.
GuardianTrip ==
    /\ phase \notin {"running", "stopped", "done"}
    /\ ~guardian
    /\ guardian' = TRUE
    /\ UNCHANGED <<phase, free, raw, sample, admitted,
                   admissionRaw, admissionSample, stopReason, evidence>>

PublishRefusal ==
    /\ phase = "stopped"
    /\ evidence' = TRUE
    /\ phase' = "done"
    /\ UNCHANGED <<free, raw, sample, guardian, admitted,
                   admissionRaw, admissionSample, stopReason>>

Next == CheckGuardian \/ ProbeBoundary \/ DecideHygiene \/ Hygiene
        \/ ProbeAfterHygiene \/ DecideAfterHygiene \/ CheckAdmission
        \/ Admit \/ GuardianTrip \/ PublishRefusal

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"guard", "probe", "boundary-decide", "hygiene", "post-guard",
                  "post-probe", "post-decide", "admission-check", "admit",
                  "running", "stopped", "done"}
    /\ free \in FreeSamples
    /\ raw \in RawSamples
    /\ sample \in ParsedSamples
    /\ guardian \in BOOLEAN
    /\ admitted \in BOOLEAN
    /\ admissionRaw \in RawSamples
    /\ admissionSample \in ParsedSamples
    /\ stopReason \in {"none", "disk", "probe", "guardian"}
    /\ evidence \in BOOLEAN

AdmissionRequiresBand ==
    admitted /\ admissionSample.known => admissionSample.mib >= FloorMiB + BandMiB
AdmissionRequiresSample == admitted => admissionSample.known
AdmissionRequiresValidSample == admitted => admissionRaw.kind # "malformed"
StopPreventsAdmission == stopReason # "none" => ~admitted
RefusalRecorded == phase = "done" => evidence /\ stopReason # "none" /\ ~admitted
Completes == <>(admitted \/ (phase = "done" /\ evidence))
=============================================================================

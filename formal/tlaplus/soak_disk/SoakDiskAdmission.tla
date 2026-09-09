-------------------------- MODULE SoakDiskAdmission --------------------------
(* One iteration-boundary disk admission decision in                         *)
(* scripts/run-merge-recovery-soak.sh: probe, optional hygiene, re-probe,    *)
(* decide, after the opening benchmark on the first segment. Seven constants *)
(* switch the seven corrections on and off so that each pre-fix              *)
(* configuration reproduces one historical defect.                           *)
EXTENDS Naturals, TLC

CONSTANTS FloorMiB, BandMiB, FreeSamples, InitialFreeMiB, MalformedPrefixMiB,
          RequireBand,     \* post-hygiene refusal compares against floor + band
          RejectMissing,   \* a probe that returns nothing cannot admit
          RejectMalformed, \* a field such as 16384junk cannot admit
          CheckGuardianAlive, \* a dead guardian process cannot admit
          CheckRetainedBreach, \* a retained breach marker blocks the opening benchmark
          CheckDiskBand, \* the opening benchmark needs a sample at or above floor + band
          MonitorOpening \* the guardian is started before the opening benchmark

ASSUME /\ FloorMiB \in Nat \ {0}
       /\ BandMiB \in Nat
       /\ FreeSamples \subseteq Nat
       /\ InitialFreeMiB \in FreeSamples
       /\ MalformedPrefixMiB \in Nat
       /\ {RequireBand, RejectMissing, RejectMalformed, CheckGuardianAlive,
           CheckRetainedBreach, CheckDiskBand, MonitorOpening} \subseteq BOOLEAN

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

VARIABLES phase, free, raw, sample, guardian, guardianAlive, admitted,
          admissionRaw, admissionSample, stopReason, evidence,
          retained,  \* a breach marker left behind by the previous run
          benchmark, \* the opening benchmark was launched
          benchmarkSample, \* the disk sample read before the benchmark decision
          benchmarkFault,  \* the disk fell below the hard floor during the benchmark
          benchmarkObserved \* a watching guardian recorded that fall and asked for a stop

vars == <<phase, free, raw, sample, guardian, guardianAlive, admitted,
          admissionRaw, admissionSample, stopReason, evidence, retained, benchmark,
          benchmarkSample, benchmarkFault, benchmarkObserved>>

Init ==
    /\ phase = "benchmark"
    /\ free = InitialFreeMiB
    /\ raw = MissingRaw
    /\ sample = Unknown
    /\ retained \in BOOLEAN
    /\ guardian = retained
    /\ benchmark = FALSE
    /\ benchmarkSample = Unknown
    /\ benchmarkFault = FALSE
    /\ benchmarkObserved = FALSE
    /\ guardianAlive = TRUE
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

\* The opening benchmark of the first segment. B15: the driver skips it when a
\* breach marker was retained from the previous run; the recovery block that
\* reads the marker runs later. B16: otherwise it probes the disk first and
\* refuses, recording a protection failure, unless a known sample is at or
\* above floor + band. The benchmark-time sample ranges over every raw sample;
\* the boundary decision that follows starts from InitialFreeMiB, since the
\* benchmark itself consumes space. B17: while an admitted benchmark runs, the
\* disk may fall below the hard floor; only a guardian started before the
\* benchmark observes that, records the marker, and asks for a stop.
Benchmark ==
    /\ phase = "benchmark"
    /\ IF CheckRetainedBreach /\ retained
          THEN /\ benchmark' = FALSE
               /\ benchmarkSample' = Unknown
               /\ benchmarkFault' = FALSE
               /\ benchmarkObserved' = FALSE
               /\ phase' = "guard"
               /\ UNCHANGED <<stopReason, guardian>>
          ELSE \E r \in RawSamples, fault \in BOOLEAN :
               LET s == Parse(r)
                   ok == ~CheckDiskBand \/ (s.known /\ s.mib >= FloorMiB + BandMiB)
                   observed == ok /\ fault /\ MonitorOpening
               IN /\ benchmarkSample' = s
                  /\ benchmark' = ok
                  /\ benchmarkFault' = (ok /\ fault)
                  /\ benchmarkObserved' = observed
                  /\ guardian' = (guardian \/ observed)
                  /\ phase' = IF ok THEN "guard" ELSE "stopped"
                  /\ stopReason' = IF ok THEN stopReason
                                   ELSE IF s.known THEN "disk" ELSE "probe"
    /\ UNCHANGED <<free, raw, sample, guardianAlive, admitted,
                   admissionRaw, admissionSample, evidence, retained>>

CheckGuardian ==
    /\ phase \in {"guard", "post-guard"}
    /\ IF guardian
          THEN /\ phase' = "stopped"
               /\ stopReason' = "guardian"
          ELSE /\ phase' = IF phase = "guard" THEN "probe" ELSE "post-probe"
               /\ UNCHANGED stopReason
    /\ UNCHANGED <<free, raw, sample, guardian, guardianAlive, admitted,
                   admissionRaw, admissionSample, evidence>>

ProbeBoundary ==
    /\ phase = "probe"
    /\ Probe("boundary-decide")
    /\ UNCHANGED <<free, guardian, guardianAlive, admitted, admissionRaw, admissionSample,
                   stopReason, evidence>>

DecideHygiene ==
    /\ phase = "boundary-decide"
    /\ phase' = IF BelowBand THEN "hygiene" ELSE "admission-check"
    /\ UNCHANGED <<free, raw, sample, guardian, guardianAlive, admitted,
                   admissionRaw, admissionSample, stopReason, evidence>>

\* reclaim_disk_space never loses space and does not always recover any.
Hygiene ==
    /\ phase = "hygiene"
    /\ free' \in {v \in FreeSamples : v >= free}
    /\ phase' = "post-guard"
    /\ UNCHANGED <<raw, sample, guardian, guardianAlive, admitted,
                   admissionRaw, admissionSample, stopReason, evidence>>

ProbeAfterHygiene ==
    /\ phase = "post-probe"
    /\ Probe("post-decide")
    /\ UNCHANGED <<free, guardian, guardianAlive, admitted, admissionRaw, admissionSample,
                   stopReason, evidence>>

DecideAfterHygiene ==
    /\ phase = "post-decide"
    /\ IF sample.known /\ sample.mib < Threshold
          THEN /\ phase' = "stopped"
               /\ stopReason' = "disk"
          ELSE /\ phase' = "admission-check"
               /\ UNCHANGED stopReason
    /\ UNCHANGED <<free, raw, sample, guardian, guardianAlive, admitted,
                   admissionRaw, admissionSample, evidence>>

\* The common check before work starts: a missing sample, then (B14) a
\* guardian process that died since the boundary probe.
CheckAdmission ==
    /\ phase = "admission-check"
    /\ IF RejectMissing /\ ~sample.known
          THEN /\ phase' = "stopped"
               /\ stopReason' = "probe"
          ELSE IF CheckGuardianAlive /\ ~guardianAlive
          THEN /\ phase' = "stopped"
               /\ stopReason' = "guardian"
          ELSE /\ phase' = "admit"
               /\ UNCHANGED stopReason
    /\ UNCHANGED <<free, raw, sample, guardian, guardianAlive, admitted,
                   admissionRaw, admissionSample, evidence>>

Admit ==
    /\ phase = "admit"
    /\ admitted' = TRUE
    /\ admissionRaw' = raw
    /\ admissionSample' = sample
    /\ phase' = "running"
    /\ UNCHANGED <<free, raw, sample, guardian, guardianAlive, stopReason, evidence>>

\* The guardian marker can appear at any point before the workload starts.
GuardianTrip ==
    /\ phase \notin {"running", "stopped", "done"}
    /\ ~guardian
    /\ guardian' = TRUE
    /\ UNCHANGED <<phase, free, raw, sample, guardianAlive, admitted,
                   admissionRaw, admissionSample, stopReason, evidence>>

\* The guardian process can die at any point up to the admission check. The
\* check and the workload start are one step here, as in the B14 model; the
\* production window between them is not closed by this correction.
GuardianCrash ==
    /\ phase \notin {"admit", "running", "stopped", "done"}
    /\ guardianAlive
    /\ guardianAlive' = FALSE
    /\ UNCHANGED <<phase, free, raw, sample, guardian, admitted,
                   admissionRaw, admissionSample, stopReason, evidence>>

PublishRefusal ==
    /\ phase = "stopped"
    /\ evidence' = TRUE
    /\ phase' = "done"
    /\ UNCHANGED <<free, raw, sample, guardian, guardianAlive, admitted,
                   admissionRaw, admissionSample, stopReason>>

Next == Benchmark
        \/ (/\ CheckGuardian \/ ProbeBoundary \/ DecideHygiene \/ Hygiene
               \/ ProbeAfterHygiene \/ DecideAfterHygiene \/ CheckAdmission
               \/ Admit \/ GuardianTrip \/ GuardianCrash \/ PublishRefusal
            /\ UNCHANGED <<retained, benchmark, benchmarkSample, benchmarkFault,
                           benchmarkObserved>>)

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"benchmark", "guard", "probe", "boundary-decide", "hygiene",
                  "post-guard", "post-probe", "post-decide", "admission-check",
                  "admit", "running", "stopped", "done"}
    /\ free \in FreeSamples
    /\ raw \in RawSamples
    /\ sample \in ParsedSamples
    /\ guardian \in BOOLEAN
    /\ guardianAlive \in BOOLEAN
    /\ admitted \in BOOLEAN
    /\ admissionRaw \in RawSamples
    /\ admissionSample \in ParsedSamples
    /\ stopReason \in {"none", "disk", "probe", "guardian"}
    /\ evidence \in BOOLEAN
    /\ retained \in BOOLEAN
    /\ benchmark \in BOOLEAN
    /\ benchmarkSample \in ParsedSamples
    /\ benchmarkFault \in BOOLEAN
    /\ benchmarkObserved \in BOOLEAN

AdmissionRequiresBand ==
    admitted /\ admissionSample.known => admissionSample.mib >= FloorMiB + BandMiB
AdmissionRequiresSample == admitted => admissionSample.known
AdmissionRequiresValidSample == admitted => admissionRaw.kind # "malformed"
AdmissionRequiresGuardian == admitted => guardianAlive
RetainedBreachPreventsBenchmark == retained => ~benchmark
BenchmarkRequiresBand ==
    benchmark => benchmarkSample.known /\ benchmarkSample.mib >= FloorMiB + BandMiB
BenchmarkBreachObserved == benchmarkFault => benchmarkObserved /\ guardian
StopPreventsAdmission == stopReason # "none" => ~admitted
RefusalRecorded == phase = "done" => evidence /\ stopReason # "none" /\ ~admitted
Completes == <>(admitted \/ (phase = "done" /\ evidence))
=============================================================================

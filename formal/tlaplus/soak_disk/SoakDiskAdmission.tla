-------------------------- MODULE SoakDiskAdmission --------------------------
(* One iteration-boundary disk admission decision in                         *)
(* scripts/run-merge-recovery-soak.sh: probe, optional hygiene, re-probe,    *)
(* decide, after the opening benchmark on the first segment. Nine Boolean    *)
(* constants switch the corrections on and off so that each pre-fix          *)
(* configuration reproduces one historical defect; BenchmarkFaults selects   *)
(* the fault kinds an admitted benchmark can suffer.                         *)
EXTENDS Naturals, TLC

CONSTANTS FloorMiB, BandMiB, FreeSamples, InitialFreeMiB, MalformedPrefixMiB,
          RequireBand,     \* post-hygiene refusal compares against floor + band
          RejectMissing,   \* a probe that returns nothing cannot admit
          RejectMalformed, \* a field such as 16384junk cannot admit
          CheckGuardianAlive, \* a dead guardian process cannot admit
          CheckRetainedBreach, \* a retained breach marker blocks the opening benchmark
          CheckDiskBand, \* the opening benchmark needs a sample at or above floor + band
          MonitorOpening, \* the guardian is started before the opening benchmark
          WatchGuardian,  \* a guardian fault during the benchmark cancels it (B18)
          CheckProgress,  \* stale guardian progress cannot admit (B22)
          BenchmarkFaults \* fault kinds an admitted benchmark can suffer: "breach", "death"

ASSUME /\ FloorMiB \in Nat \ {0}
       /\ BandMiB \in Nat
       /\ FreeSamples \subseteq Nat
       /\ InitialFreeMiB \in FreeSamples
       /\ MalformedPrefixMiB \in Nat
       /\ {RequireBand, RejectMissing, RejectMalformed, CheckGuardianAlive,
           CheckRetainedBreach, CheckDiskBand, MonitorOpening, WatchGuardian,
           CheckProgress} \subseteq BOOLEAN
       /\ BenchmarkFaults \subseteq {"breach", "death"}

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
          benchmarkFault,  \* "none", or the fault the admitted benchmark suffered
          benchmarkObserved, \* a started guardian recorded the breach and asked for a stop
          benchmarkCancelled, \* the driver cancelled the benchmark on the fault (B18)
          benchmarkGuardianAlive, \* the guardian was alive when the benchmark was admitted (B19)
          guardianFresh,  \* the guardian's progress record is within the silence limit
          admissionFresh, \* progress was fresh when the iteration was admitted (B22)
          benchmarkFresh  \* progress was fresh when the benchmark was admitted (B22)

vars == <<phase, free, raw, sample, guardian, guardianAlive, admitted,
          admissionRaw, admissionSample, stopReason, evidence, retained, benchmark,
          benchmarkSample, benchmarkFault, benchmarkObserved, benchmarkCancelled,
          benchmarkGuardianAlive, guardianFresh, admissionFresh, benchmarkFresh>>

Init ==
    /\ phase = "benchmark"
    /\ free = InitialFreeMiB
    /\ raw = MissingRaw
    /\ sample = Unknown
    /\ retained \in BOOLEAN
    /\ guardian = retained
    /\ benchmark = FALSE
    /\ benchmarkSample = Unknown
    /\ benchmarkFault = "none"
    /\ benchmarkObserved = FALSE
    /\ benchmarkCancelled = FALSE
    /\ benchmarkGuardianAlive = TRUE
    /\ guardianFresh = TRUE
    /\ admissionFresh = TRUE
    /\ benchmarkFresh = TRUE
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
\* benchmark itself consumes space. B19: a guardian that died before the
\* benchmark cannot admit it. B17: while an admitted benchmark runs, the disk
\* may fall below the hard floor; only a guardian started before the benchmark
\* observes that, records the marker, and asks for a stop. B18: a fault during
\* the benchmark (a recorded breach, or guardian death) cancels the benchmark
\* and publishes the failure only when the driver watches the guardian; the
\* stop, TERM, grace, and kill sequence is one step here.
Benchmark ==
    /\ phase = "benchmark"
    /\ IF CheckRetainedBreach /\ retained
          THEN /\ benchmark' = FALSE
               /\ benchmarkSample' = Unknown
               /\ benchmarkFault' = "none"
               /\ benchmarkObserved' = FALSE
               /\ benchmarkCancelled' = FALSE
               /\ benchmarkGuardianAlive' = guardianAlive
               /\ benchmarkFresh' = guardianFresh
               /\ phase' = "guard"
               /\ UNCHANGED <<stopReason, guardian, guardianAlive>>
          ELSE \E r \in RawSamples, fault \in {"none"} \cup BenchmarkFaults :
               LET s == Parse(r)
                   diskOk == ~CheckDiskBand \/ (s.known /\ s.mib >= FloorMiB + BandMiB)
                   guardianOk == /\ ~CheckGuardianAlive \/ guardianAlive
                                 /\ ~CheckProgress \/ guardianFresh
                   admit == diskOk /\ guardianOk
                   f == IF admit THEN fault ELSE "none"
                   observed == f = "breach" /\ MonitorOpening
                   cancelled == f # "none" /\ WatchGuardian
               IN /\ benchmarkSample' = s
                  /\ benchmark' = admit
                  /\ benchmarkGuardianAlive' = guardianAlive
                  /\ benchmarkFresh' = guardianFresh
                  /\ benchmarkFault' = f
                  /\ benchmarkObserved' = observed
                  /\ benchmarkCancelled' = cancelled
                  /\ guardian' = (guardian \/ observed \/ cancelled)
                  /\ guardianAlive' = (guardianAlive /\ f # "death")
                  /\ phase' = IF admit THEN "guard" ELSE "stopped"
                  /\ stopReason' = IF admit THEN stopReason
                                   ELSE IF ~diskOk /\ s.known THEN "disk"
                                   ELSE IF ~diskOk THEN "probe"
                                   ELSE "guardian"
    /\ UNCHANGED <<free, raw, sample, admitted, admissionRaw, admissionSample,
                   evidence, retained, guardianFresh, admissionFresh>>

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
\* guardian process that died since the boundary probe, then (B22) a guardian
\* whose progress record has expired.
CheckAdmission ==
    /\ phase = "admission-check"
    /\ IF RejectMissing /\ ~sample.known
          THEN /\ phase' = "stopped"
               /\ stopReason' = "probe"
          ELSE IF CheckGuardianAlive /\ ~guardianAlive
          THEN /\ phase' = "stopped"
               /\ stopReason' = "guardian"
          ELSE IF CheckProgress /\ ~guardianFresh
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
    /\ admissionFresh' = guardianFresh
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

\* The guardian stays alive but stops recording progress (SIGSTOP, a paused
\* host) at any point up to the admission check, as GuardianCrash does.
GuardianStall ==
    /\ phase \notin {"admit", "running", "stopped", "done"}
    /\ guardianFresh
    /\ guardianFresh' = FALSE
    /\ UNCHANGED <<phase, free, raw, sample, guardian, guardianAlive, admitted,
                   admissionRaw, admissionSample, stopReason, evidence>>

PublishRefusal ==
    /\ phase = "stopped"
    /\ evidence' = TRUE
    /\ phase' = "done"
    /\ UNCHANGED <<free, raw, sample, guardian, guardianAlive, admitted,
                   admissionRaw, admissionSample, stopReason>>

FrozenAfterBenchmark == <<retained, benchmark, benchmarkSample, benchmarkFault,
                          benchmarkObserved, benchmarkCancelled,
                          benchmarkGuardianAlive, benchmarkFresh>>

Next == Benchmark
        \/ (/\ Admit
            /\ UNCHANGED FrozenAfterBenchmark
            /\ UNCHANGED guardianFresh)
        \/ (/\ GuardianStall
            /\ UNCHANGED FrozenAfterBenchmark
            /\ UNCHANGED admissionFresh)
        \/ (/\ CheckGuardian \/ ProbeBoundary \/ DecideHygiene \/ Hygiene
               \/ ProbeAfterHygiene \/ DecideAfterHygiene \/ CheckAdmission
               \/ GuardianTrip \/ GuardianCrash \/ PublishRefusal
            /\ UNCHANGED FrozenAfterBenchmark
            /\ UNCHANGED <<guardianFresh, admissionFresh>>)

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
    /\ benchmarkFault \in {"none", "breach", "death"}
    /\ benchmarkObserved \in BOOLEAN
    /\ benchmarkCancelled \in BOOLEAN
    /\ benchmarkGuardianAlive \in BOOLEAN
    /\ guardianFresh \in BOOLEAN
    /\ admissionFresh \in BOOLEAN
    /\ benchmarkFresh \in BOOLEAN

AdmissionRequiresBand ==
    admitted /\ admissionSample.known => admissionSample.mib >= FloorMiB + BandMiB
AdmissionRequiresSample == admitted => admissionSample.known
AdmissionRequiresValidSample == admitted => admissionRaw.kind # "malformed"
AdmissionRequiresGuardian ==
    /\ admitted => guardianAlive
    /\ benchmark => benchmarkGuardianAlive
RetainedBreachPreventsBenchmark == retained => ~benchmark
BenchmarkRequiresBand ==
    benchmark => benchmarkSample.known /\ benchmarkSample.mib >= FloorMiB + BandMiB
BenchmarkBreachObserved == benchmarkFault = "breach" => benchmarkObserved /\ guardian
BenchmarkCancellationObserved ==
    benchmarkFault # "none" => benchmarkCancelled /\ guardian
StaleProgressPreventsAdmission ==
    /\ admitted => admissionFresh
    /\ benchmark => benchmarkFresh
StopPreventsAdmission == stopReason # "none" => ~admitted
RefusalRecorded == phase = "done" => evidence /\ stopReason # "none" /\ ~admitted
Completes == <>(admitted \/ (phase = "done" /\ evidence))
=============================================================================

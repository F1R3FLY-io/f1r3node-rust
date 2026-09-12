-------------------------- MODULE SoakDiskAdmission --------------------------
(* One iteration-boundary disk admission decision in                         *)
(* scripts/run-merge-recovery-soak.sh: probe, optional hygiene, re-probe,    *)
(* decide, after the opening benchmark on the first segment. Twenty-one      *)
(* Boolean constants switch the corrections on and off so that each pre-fix  *)
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
          EnforceHygieneDeadline, \* hygiene commands run under a deadline (B23)
          CheckRange,     \* floor, band, and their sum must fit in 64 bits (B24)
          PreserveUnowned, \* hygiene never deletes a temporary session by age (B25)
          EnforceCleanupFailures, \* a failed cleanup command fails hygiene (B26)
          PreserveDockerResources, \* hygiene inspects Docker resources, never prunes them (B27)
          RememberInFlight, \* a segment that crashed mid-iteration refuses the next segment (B29)
          RememberBenchmark, \* a segment that crashed during the opening benchmark refuses the next segment (B37)
          WatchMonitor, \* a crash monitor exit during the benchmark cancels it (B41)
          CheckMonitorAlive, \* a dead crash monitor cannot admit the benchmark or an iteration (B42)
          VerifyPlacement, \* required containment admits work only when the trusted run-domain record matches the driver's own placement (B45)
          BindIdentity, \* the record is trusted only when every path component was opened from the root, root-owned, unwritable, and within the size bound (B46)
          RecordBeforeAttribution, \* the driver writes the breach record and the early-exit record before any disk usage attribution starts (B48)
          BenchmarkFaults \* fault kinds an admitted benchmark can suffer: "breach", "death"

ASSUME /\ FloorMiB \in Nat \ {0}
       /\ BandMiB \in Nat
       /\ FreeSamples \subseteq Nat
       /\ InitialFreeMiB \in FreeSamples
       /\ MalformedPrefixMiB \in Nat
       /\ {RequireBand, RejectMissing, RejectMalformed, CheckGuardianAlive,
           CheckRetainedBreach, CheckDiskBand, MonitorOpening, WatchGuardian,
           CheckProgress, EnforceHygieneDeadline, CheckRange, PreserveUnowned,
           EnforceCleanupFailures, PreserveDockerResources, RememberInFlight,
           RememberBenchmark, WatchMonitor, CheckMonitorAlive, VerifyPlacement,
           BindIdentity, RecordBeforeAttribution} \subseteq BOOLEAN
       /\ BenchmarkFaults \subseteq {"breach", "death", "monitor-death"}

Threshold == IF RequireBand THEN FloorMiB + BandMiB ELSE FloorMiB

HygieneBudget == 2  \* SOAK_DISK_HYGIENE_SECONDS: TERM at 1, KILL at 2

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
          benchmarkFresh, \* progress was fresh when the benchmark was admitted (B22)
          hygieneStalled, hygieneElapsed, hygieneTermSent, hygieneKillSent,
            \* a hygiene command group that ignores TERM, under the deadline (B23)
          settingsValid, \* the configured floor, band, and their sum fit in 64 bits
          sessionAge,    \* an unowned temporary session with a live writer: "old" or "recent"
          sessionPresent, \* that session still exists after hygiene
          cleanupFailed, \* a Docker cleanup command failed during hygiene (B26)
          dockerPresent, \* unowned Docker resources still exist after hygiene (B27)
          interrupted,   \* the previous segment died with an iteration in flight (B29)
          benchmarkInterrupted, \* the previous segment died with the opening benchmark in flight (B37)
          monitorAlive, \* the crash monitor process is alive (B41, B42)
          benchmarkMonitorAlive, \* the monitor was alive when the benchmark was admitted (B42)
          recordMatches, \* the launcher's run-domain record matches the driver's cgroup and uid (B45)
          recordTrusted, \* the record the driver opened sits below a root-owned, unwritable chain and ends within its bound (B46)
          attributionStarted \* the disk usage attribution has started after a disk breach (B48)

HygieneVars == <<hygieneStalled, hygieneElapsed, hygieneTermSent, hygieneKillSent>>

DriverVars == <<phase, free, raw, sample, guardian, guardianAlive, admitted,
          admissionRaw, admissionSample, stopReason, evidence, retained, benchmark,
          benchmarkSample, benchmarkFault, benchmarkObserved, benchmarkCancelled,
          benchmarkGuardianAlive, guardianFresh, admissionFresh, benchmarkFresh,
          hygieneStalled, hygieneElapsed, hygieneTermSent, hygieneKillSent,
          settingsValid, sessionAge, sessionPresent, cleanupFailed, dockerPresent,
          interrupted, benchmarkInterrupted, monitorAlive, benchmarkMonitorAlive, recordMatches,
          recordTrusted>>

vars == <<DriverVars, attributionStarted>>

Init ==
    /\ phase = "config"
    /\ settingsValid \in BOOLEAN
    /\ sessionAge \in {"old", "recent"}
    /\ sessionPresent = TRUE
    /\ cleanupFailed = FALSE
    /\ dockerPresent = TRUE
    /\ interrupted \in BOOLEAN
    /\ benchmarkInterrupted \in BOOLEAN
    /\ monitorAlive = TRUE
    /\ benchmarkMonitorAlive = TRUE
    /\ recordMatches \in BOOLEAN
    /\ recordTrusted \in BOOLEAN
    /\ attributionStarted = FALSE
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
    /\ hygieneStalled = FALSE
    /\ hygieneElapsed = 0
    /\ hygieneTermSent = FALSE
    /\ hygieneKillSent = FALSE
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

\* B24: the configured floor and band are validated as decimals before any
\* work; an out-of-range value, or a sum past the 64-bit maximum, rejects the
\* configuration outright (exit 2, no summary). Whether a given text is in
\* range is abstracted to settingsValid; the harness checks the three
\* concrete boundary texts.
\* B29: the state file records an iteration in flight; a segment that finds
\* one counts a failure and refuses work, since the writers' termination is
\* unconfirmed. The pre-fix driver resumed as if the iteration had ended. The
\* same record now covers the opening benchmark: a segment that died with the
\* benchmark in flight counts a failure and a benchmark failure and refuses.
ValidateSettings ==
    /\ phase = "config"
    /\ LET rejected == CheckRange /\ ~settingsValid
           halted == ~rejected /\ ((RememberInFlight /\ interrupted)
                                   \/ (RememberBenchmark /\ benchmarkInterrupted))
       IN /\ phase' = IF rejected THEN "rejected"
                      ELSE IF halted THEN "stopped" ELSE "benchmark"
          /\ stopReason' = IF halted THEN "interrupted" ELSE stopReason
    /\ UNCHANGED <<free, raw, sample, guardian, guardianAlive, admitted,
                   admissionRaw, admissionSample, evidence>>

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
\* The run-domain check passes when the record matches the driver's placement.
\* With BindIdentity (B46) the record must also be the one the driver opened
\* through a root-owned, unwritable chain, within its size bound. The pre-fix
\* driver trusted the pathname view and read the record separately.
PlacementVerified == recordMatches /\ (~BindIdentity \/ recordTrusted)

Benchmark ==
    /\ phase = "benchmark"
    /\ IF CheckRetainedBreach /\ retained
          THEN /\ benchmark' = FALSE
               /\ benchmarkSample' = Unknown
               /\ benchmarkFault' = "none"
               /\ benchmarkObserved' = FALSE
               /\ benchmarkCancelled' = FALSE
               /\ benchmarkGuardianAlive' = guardianAlive
               /\ benchmarkMonitorAlive' = monitorAlive
               /\ benchmarkFresh' = guardianFresh
               /\ phase' = "guard"
               /\ UNCHANGED <<stopReason, guardian, guardianAlive, monitorAlive>>
          ELSE \E r \in RawSamples, fault \in {"none"} \cup BenchmarkFaults :
               LET s == Parse(r)
                   diskOk == ~CheckDiskBand \/ (s.known /\ s.mib >= FloorMiB + BandMiB)
                   guardianOk == /\ ~CheckGuardianAlive \/ guardianAlive
                                 /\ ~CheckProgress \/ guardianFresh
                                 /\ ~CheckMonitorAlive \/ monitorAlive
                                 /\ ~VerifyPlacement \/ PlacementVerified
                   admit == diskOk /\ guardianOk
                   f == IF admit THEN fault ELSE "none"
                   observed == f = "breach" /\ MonitorOpening
                   cancelled == \/ f \in {"breach", "death"} /\ WatchGuardian
                                \/ f = "monitor-death" /\ WatchMonitor
               IN /\ benchmarkSample' = s
                  /\ benchmark' = admit
                  /\ benchmarkGuardianAlive' = guardianAlive
                  /\ benchmarkMonitorAlive' = monitorAlive
                  /\ benchmarkFresh' = guardianFresh
                  /\ benchmarkFault' = f
                  /\ benchmarkObserved' = observed
                  /\ benchmarkCancelled' = cancelled
                  /\ guardian' = (guardian \/ observed \/ cancelled)
                  /\ guardianAlive' = (guardianAlive /\ f # "death")
                  /\ monitorAlive' = (monitorAlive /\ f # "monitor-death")
                  /\ phase' = IF admit THEN "guard" ELSE "stopped"
                  /\ stopReason' = IF admit THEN stopReason
                                   ELSE IF ~diskOk /\ s.known THEN "disk"
                                   ELSE IF ~diskOk THEN "probe"
                                   ELSE "guardian"
    /\ UNCHANGED <<free, raw, sample, admitted, admissionRaw, admissionSample,
                   evidence, retained, guardianFresh, admissionFresh, settingsValid,
                   sessionAge, sessionPresent, cleanupFailed, dockerPresent,
                   interrupted, benchmarkInterrupted, recordMatches, recordTrusted>>

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

\* reclaim_disk_space never loses space and does not always recover any. B25:
\* directory age proves neither ownership nor writer termination, so the
\* corrected sweep keeps every temporary session; the pre-fix sweep deleted
\* old ones under a live writer. B26: any Docker cleanup command may fail;
\* the corrected driver keeps that failure across the group and takes the
\* same refusal path as a killed group, whatever the later sample says. B27:
\* the driver cannot tell its own Docker resources from the host's, so the
\* corrected hygiene only inspects them; the pre-fix hygiene pruned every
\* exited container, unused network, dangling image, and build cache.
SweepKeeps == PreserveUnowned \/ sessionAge = "recent"

HygieneCompletes(next) ==
    \E failed \in BOOLEAN :
        LET refuse == EnforceCleanupFailures /\ failed IN
        /\ cleanupFailed' = failed
        /\ free' \in {v \in FreeSamples : v >= free}
        /\ sessionPresent' = SweepKeeps
        /\ dockerPresent' = PreserveDockerResources
        /\ phase' = IF refuse THEN "stopped" ELSE next
        /\ stopReason' = IF refuse THEN "hygiene" ELSE stopReason
        /\ guardian' = (guardian \/ refuse)

Hygiene ==
    /\ phase = "hygiene"
    /\ HygieneCompletes("post-guard")
    /\ UNCHANGED <<raw, sample, guardianAlive, admitted,
                   admissionRaw, admissionSample, evidence>>

\* B23: the last hygiene client (docker system df) ignores TERM and stalls. Under
\* the deadline, timeout sends TERM at one unit and KILL at two; a killed group
\* is a protection breach, the marker is written, and the driver refuses work.
HygieneStall ==
    /\ phase = "hygiene"
    /\ ~hygieneStalled
    /\ hygieneStalled' = TRUE
    /\ phase' = "hygiene-stalled"
    /\ UNCHANGED <<free, raw, sample, guardian, guardianAlive, admitted,
                   admissionRaw, admissionSample, stopReason, evidence,
                   hygieneElapsed, hygieneTermSent, hygieneKillSent>>

HygieneTick ==
    /\ phase = "hygiene-stalled"
    /\ hygieneElapsed < 3
    /\ hygieneElapsed' = hygieneElapsed + 1
    /\ hygieneTermSent' = (EnforceHygieneDeadline /\ hygieneElapsed' >= 1)
    /\ hygieneKillSent' = (EnforceHygieneDeadline /\ hygieneElapsed' = HygieneBudget)
    /\ phase' = IF hygieneKillSent' THEN "stopped" ELSE "hygiene-stalled"
    /\ stopReason' = IF hygieneKillSent' THEN "hygiene" ELSE stopReason
    /\ guardian' = (guardian \/ hygieneKillSent')
    /\ UNCHANGED <<free, raw, sample, guardianAlive, admitted, admissionRaw,
                   admissionSample, evidence, hygieneStalled>>

\* The stalled client returns on its own; hygiene completes as usual.
HygieneReturns ==
    /\ phase = "hygiene-stalled"
    /\ HygieneCompletes("post-guard")
    /\ UNCHANGED <<raw, sample, guardianAlive, admitted,
                   admissionRaw, admissionSample, evidence, HygieneVars>>

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
\* guardian process that died since the boundary probe, then (B42) a dead
\* crash monitor, then (B45, B46) a run-domain record that does not match the
\* driver's placement, or was not opened through a trusted chain, under
\* required containment, then (B22) a guardian
\* whose progress record has expired.
CheckAdmission ==
    /\ phase = "admission-check"
    /\ IF RejectMissing /\ ~sample.known
          THEN /\ phase' = "stopped"
               /\ stopReason' = "probe"
          ELSE IF CheckGuardianAlive /\ ~guardianAlive
          THEN /\ phase' = "stopped"
               /\ stopReason' = "guardian"
          ELSE IF CheckMonitorAlive /\ ~monitorAlive
          THEN /\ phase' = "stopped"
               /\ stopReason' = "guardian"
          ELSE IF VerifyPlacement /\ ~PlacementVerified
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

\* B42: the crash monitor can die at any point up to either admission; the
\* corrected driver checks it before the benchmark and before each iteration.
\* The pre-fix driver checked it only at startup and mid-work.
MonitorCrash ==
    /\ phase \notin {"admit", "running", "stopped", "done"}
    /\ monitorAlive
    /\ monitorAlive' = FALSE
    /\ UNCHANGED <<phase, free, raw, sample, guardian, guardianAlive, admitted,
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

\* After a disk breach the driver attributes the usage with df, du, and
\* docker system df. The corrected driver (B48) writes the breach record and
\* the early-exit record first, so a stalled attribution cannot delay them.
\* The pre-fix driver attributed on the hygiene-pass path before it decided.
Attribute ==
    /\ phase \in {"stopped", "done"}
    /\ stopReason = "disk"
    /\ ~attributionStarted
    /\ ~RecordBeforeAttribution \/ evidence
    /\ attributionStarted' = TRUE
    /\ UNCHANGED DriverVars

FrozenAfterBenchmark == <<retained, benchmark, benchmarkSample, benchmarkFault,
                          benchmarkObserved, benchmarkCancelled,
                          benchmarkGuardianAlive, benchmarkMonitorAlive, benchmarkFresh,
                          settingsValid, sessionAge, interrupted, benchmarkInterrupted,
                          recordMatches, recordTrusted>>

HygieneOutcome == <<sessionPresent, cleanupFailed, dockerPresent>>

Next == (Benchmark /\ UNCHANGED <<HygieneVars, attributionStarted>>)
        \/ (/\ Admit
            /\ UNCHANGED <<FrozenAfterBenchmark, attributionStarted>>
            /\ UNCHANGED <<guardianFresh, monitorAlive, HygieneVars, HygieneOutcome>>)
        \/ (/\ GuardianStall
            /\ UNCHANGED <<FrozenAfterBenchmark, attributionStarted>>
            /\ UNCHANGED <<admissionFresh, monitorAlive, HygieneVars, HygieneOutcome>>)
        \/ (/\ HygieneStall \/ HygieneTick
            /\ UNCHANGED <<FrozenAfterBenchmark, attributionStarted>>
            /\ UNCHANGED <<guardianFresh, admissionFresh, monitorAlive, HygieneOutcome>>)
        \/ (/\ HygieneReturns
            /\ UNCHANGED <<FrozenAfterBenchmark, attributionStarted>>
            /\ UNCHANGED <<guardianFresh, admissionFresh, monitorAlive>>)
        \/ (/\ Hygiene
            /\ UNCHANGED <<FrozenAfterBenchmark, attributionStarted>>
            /\ UNCHANGED <<guardianFresh, admissionFresh, monitorAlive, HygieneVars>>)
        \/ (/\ ValidateSettings
               \/ CheckGuardian \/ ProbeBoundary \/ DecideHygiene
               \/ ProbeAfterHygiene \/ DecideAfterHygiene \/ CheckAdmission
               \/ GuardianTrip \/ GuardianCrash \/ PublishRefusal
            /\ UNCHANGED <<FrozenAfterBenchmark, attributionStarted>>
            /\ UNCHANGED <<guardianFresh, admissionFresh, monitorAlive, HygieneVars, HygieneOutcome>>)
        \/ (/\ MonitorCrash
            /\ UNCHANGED <<FrozenAfterBenchmark, attributionStarted>>
            /\ UNCHANGED <<guardianFresh, admissionFresh, HygieneVars, HygieneOutcome>>)
        \/ Attribute

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"config", "rejected", "benchmark", "guard", "probe",
                  "boundary-decide", "hygiene", "hygiene-stalled", "post-guard",
                  "post-probe", "post-decide", "admission-check", "admit",
                  "running", "stopped", "done"}
    /\ free \in FreeSamples
    /\ raw \in RawSamples
    /\ sample \in ParsedSamples
    /\ guardian \in BOOLEAN
    /\ guardianAlive \in BOOLEAN
    /\ admitted \in BOOLEAN
    /\ admissionRaw \in RawSamples
    /\ admissionSample \in ParsedSamples
    /\ stopReason \in {"none", "disk", "probe", "guardian", "hygiene", "interrupted"}
    /\ evidence \in BOOLEAN
    /\ retained \in BOOLEAN
    /\ benchmark \in BOOLEAN
    /\ benchmarkSample \in ParsedSamples
    /\ benchmarkFault \in {"none", "breach", "death", "monitor-death"}
    /\ benchmarkObserved \in BOOLEAN
    /\ benchmarkCancelled \in BOOLEAN
    /\ benchmarkGuardianAlive \in BOOLEAN
    /\ guardianFresh \in BOOLEAN
    /\ admissionFresh \in BOOLEAN
    /\ benchmarkFresh \in BOOLEAN
    /\ hygieneStalled \in BOOLEAN
    /\ hygieneElapsed \in 0..3
    /\ hygieneTermSent \in BOOLEAN
    /\ hygieneKillSent \in BOOLEAN
    /\ settingsValid \in BOOLEAN
    /\ sessionAge \in {"old", "recent"}
    /\ sessionPresent \in BOOLEAN
    /\ cleanupFailed \in BOOLEAN
    /\ dockerPresent \in BOOLEAN
    /\ interrupted \in BOOLEAN
    /\ benchmarkInterrupted \in BOOLEAN
    /\ monitorAlive \in BOOLEAN
    /\ benchmarkMonitorAlive \in BOOLEAN
    /\ recordMatches \in BOOLEAN
    /\ recordTrusted \in BOOLEAN
    /\ attributionStarted \in BOOLEAN

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
    benchmarkFault \in {"breach", "death"} => benchmarkCancelled /\ guardian
BenchmarkMonitorDeathObserved ==
    benchmarkFault = "monitor-death" => benchmarkCancelled /\ guardian
MonitorDeathPreventsAdmission ==
    /\ admitted => monitorAlive
    /\ benchmark => benchmarkMonitorAlive
UnverifiedPlacementPreventsAdmission == ~recordMatches => ~admitted /\ ~benchmark
UntrustedRecordPreventsAdmission == ~recordTrusted => ~admitted /\ ~benchmark
AttributionRequiresRecord == attributionStarted => evidence
StaleProgressPreventsAdmission ==
    /\ admitted => admissionFresh
    /\ benchmark => benchmarkFresh
HygieneWithinBudget == phase = "hygiene-stalled" => hygieneElapsed < HygieneBudget
AdmissionRequiresValidDiskSettings == (admitted \/ benchmark) => settingsValid
UnownedSessionPreserved == sessionPresent
CleanupFailurePreventsAdmission == admitted => ~cleanupFailed
UnownedDockerResourcesPreserved == dockerPresent
CrashRequiresRefusal == interrupted => ~admitted /\ ~benchmark
BenchmarkCrashRequiresRefusal == benchmarkInterrupted => ~admitted /\ ~benchmark
HygieneKillFollowsTerm == hygieneKillSent => hygieneTermSent
StopPreventsAdmission == stopReason # "none" => ~admitted
RefusalRecorded == phase = "done" => evidence /\ stopReason # "none" /\ ~admitted
Completes == <>(admitted \/ (phase = "done" /\ evidence) \/ phase = "rejected")
=============================================================================

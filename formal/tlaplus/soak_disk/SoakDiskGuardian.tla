--------------------------- MODULE SoakDiskGuardian ---------------------------
(* The emergency path of one soak iteration in                               *)
(* scripts/run-merge-recovery-soak.sh: the guardian probes, records a        *)
(* breach, stops the writers, attributes the space, and the next segment     *)
(* finds the marker. Six constants switch the six corrections on and off so  *)
(* that each pre-fix configuration reproduces one historical defect.         *)
EXTENDS Naturals, TLC

CONSTANTS DetectDeath,       \* the watcher treats a dead guardian as a breach
          RejectUnavailable, \* a probe without a sample interrupts the iteration
          EnforceTimeout,    \* df runs under a deadline; late output is discarded
          RecordFirst,       \* the breach record precedes the stop command
          AggregateDeadline, \* attribution has one budget for all roots
          PreserveBreach     \* a restart keeps the marker and a failure

ASSUME {DetectDeath, RejectUnavailable, EnforceTimeout, RecordFirst,
        AggregateDeadline, PreserveBreach} \subseteq BOOLEAN

ProbeDeadline     == 3  \* two-second timeout plus one-second kill grace
ProbeReturnsAt    == 4  \* a stalled df prints a valid field after the deadline
AttributionBudget == 1  \* SOAK_DISK_DIAGNOSTIC_SECONDS, one unit for every root

VARIABLES phase, alive, interruptRequested, breachRecorded,
          elapsed, timedOut, known,
          stopStarted, diagElapsed, rootsLeft,
          marker, priorFailures, failures, admitted

vars == <<phase, alive, interruptRequested, breachRecorded,
          elapsed, timedOut, known,
          stopStarted, diagElapsed, rootsLeft,
          marker, priorFailures, failures, admitted>>

Init ==
    /\ phase = "running"
    /\ alive = TRUE
    /\ interruptRequested = FALSE
    /\ breachRecorded = FALSE
    /\ elapsed = 0
    /\ timedOut = FALSE
    /\ known = FALSE
    /\ stopStarted = FALSE
    /\ diagElapsed = 0
    /\ rootsLeft \in {1, 3, 32}
    /\ marker = FALSE
    /\ priorFailures \in {0, 2}
    /\ failures = priorFailures
    /\ admitted = FALSE

\* The guardian process dies while the iteration runs.
Crash ==
    /\ phase = "running"
    /\ alive
    /\ alive' = FALSE
    /\ UNCHANGED <<phase, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, diagElapsed, rootsLeft, marker,
                   priorFailures, failures, admitted>>

\* The iteration watcher polls the guardian.
WatcherPoll ==
    /\ phase = "running"
    /\ ~alive
    /\ interruptRequested' = DetectDeath
    /\ breachRecorded' = DetectDeath
    /\ phase' = "watcher-decided"
    /\ UNCHANGED <<alive, elapsed, timedOut, known, stopStarted, diagElapsed,
                   rootsLeft, marker, priorFailures, failures, admitted>>

\* The live guardian starts a df probe.
StartProbe ==
    /\ phase = "running"
    /\ alive
    /\ phase' = "probing"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, diagElapsed, rootsLeft, marker,
                   priorFailures, failures, admitted>>

\* The clock advances while df has not returned.
Tick ==
    /\ phase = "probing"
    /\ elapsed < ProbeReturnsAt
    /\ elapsed' = elapsed + 1
    /\ timedOut' = (EnforceTimeout /\ elapsed' = ProbeDeadline)
    /\ phase' = IF timedOut' THEN "sampled" ELSE "probing"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, known, stopStarted,
                   diagElapsed, rootsLeft, marker, priorFailures, failures,
                   admitted>>

\* df returns promptly with or without a sample, or late with a valid field.
ProbeReturns ==
    /\ phase = "probing"
    /\ ~timedOut
    /\ \/ /\ elapsed < ProbeDeadline
          /\ known' \in BOOLEAN
       \/ /\ elapsed = ProbeReturnsAt
          /\ known' = TRUE
    /\ phase' = "sampled"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   stopStarted, diagElapsed, rootsLeft, marker, priorFailures,
                   failures, admitted>>

\* A timed-out probe supplies no sample. A valid sample here is below the
\* hard floor, so it proceeds to the breach; an unavailable sample must
\* interrupt the iteration on its own.
DecideSample ==
    /\ phase = "sampled"
    /\ LET valid == known /\ ~timedOut IN
       /\ known' = valid
       /\ IF valid
             THEN /\ phase' = "breach"
                  /\ UNCHANGED <<interruptRequested, breachRecorded>>
             ELSE /\ interruptRequested' = RejectUnavailable
                  /\ breachRecorded' = RejectUnavailable
                  /\ phase' = "sample-decided"
    /\ UNCHANGED <<alive, elapsed, timedOut, stopStarted, diagElapsed, rootsLeft,
                   marker, priorFailures, failures, admitted>>

Detect ==
    /\ phase = "breach"
    /\ phase' = IF RecordFirst THEN "record" ELSE "stop"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, diagElapsed, rootsLeft, marker,
                   priorFailures, failures, admitted>>

Record ==
    /\ phase = "record"
    /\ breachRecorded' = TRUE
    /\ phase' = "stop"
    /\ UNCHANGED <<alive, interruptRequested, elapsed, timedOut, known,
                   stopStarted, diagElapsed, rootsLeft, marker, priorFailures,
                   failures, admitted>>

\* pkill and docker kill start; termination is never confirmed.
BeginStop ==
    /\ phase = "stop"
    /\ stopStarted' = TRUE
    /\ interruptRequested' = TRUE
    /\ phase' = "attribution"
    /\ UNCHANGED <<alive, breachRecorded, elapsed, timedOut, known, diagElapsed,
                   rootsLeft, marker, priorFailures, failures, admitted>>

\* The pre-fix order writes the record after the stop command.
PublishLate ==
    /\ phase \in {"attribution", "finished"}
    /\ ~breachRecorded
    /\ breachRecorded' = TRUE
    /\ UNCHANGED <<phase, alive, interruptRequested, elapsed, timedOut, known,
                   stopStarted, diagElapsed, rootsLeft, marker, priorFailures,
                   failures, admitted>>

\* Attribution walks every root; one root can stall for the whole budget.
AttributionTick ==
    /\ phase = "attribution"
    /\ diagElapsed < 2
    /\ diagElapsed' = diagElapsed + 1
    /\ phase' = IF AggregateDeadline /\ diagElapsed' = AttributionBudget
                   THEN "finished" ELSE "attribution"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, rootsLeft, marker, priorFailures,
                   failures, admitted>>

CompleteRoot ==
    /\ phase = "attribution"
    /\ rootsLeft > 0
    /\ rootsLeft' = rootsLeft - 1
    /\ phase' = IF rootsLeft' = 0 THEN "finished" ELSE "attribution"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, diagElapsed, marker, priorFailures,
                   failures, admitted>>

\* The segment ends with the marker on disk; the next segment starts.
Finish ==
    /\ phase = "finished"
    /\ marker' = TRUE
    /\ phase' = "resume"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, diagElapsed, rootsLeft, priorFailures,
                   failures, admitted>>

Recover ==
    /\ phase = "resume"
    /\ marker' = PreserveBreach
    /\ failures' = IF PreserveBreach /\ failures = 0 THEN 1 ELSE failures
    /\ phase' = IF PreserveBreach THEN "stopped" ELSE "ready"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, diagElapsed, rootsLeft, priorFailures,
                   admitted>>

RestartDecision ==
    /\ phase \in {"stopped", "ready"}
    /\ admitted' = (phase = "ready")
    /\ phase' = "done"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, diagElapsed, rootsLeft, marker,
                   priorFailures, failures>>

Next == Crash \/ WatcherPoll \/ StartProbe \/ Tick \/ ProbeReturns
        \/ DecideSample \/ Detect \/ Record \/ BeginStop \/ PublishLate
        \/ AttributionTick \/ CompleteRoot \/ Finish \/ Recover
        \/ RestartDecision

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"running", "watcher-decided", "probing", "sampled",
                  "sample-decided", "breach", "record", "stop", "attribution",
                  "finished", "resume", "stopped", "ready", "done"}
    /\ alive \in BOOLEAN
    /\ interruptRequested \in BOOLEAN
    /\ breachRecorded \in BOOLEAN
    /\ elapsed \in 0..ProbeReturnsAt
    /\ timedOut \in BOOLEAN
    /\ known \in BOOLEAN
    /\ stopStarted \in BOOLEAN
    /\ diagElapsed \in 0..2
    /\ rootsLeft \in 0..32
    /\ marker \in BOOLEAN
    /\ priorFailures \in {0, 2}
    /\ failures \in 0..2
    /\ admitted \in BOOLEAN

DeadGuardianRequiresInterrupt ==
    (phase = "watcher-decided" /\ ~alive) => (interruptRequested /\ breachRecorded)
InvalidSampleRequiresInterrupt ==
    (phase = "sample-decided" /\ ~known) => (interruptRequested /\ breachRecorded)
ProbeWithinDeadline == phase = "probing" => elapsed < ProbeDeadline
TimedOutSampleRejected == (phase = "sample-decided" /\ timedOut) => ~known
StopRequiresRecord == stopStarted => breachRecorded
AttributionWithinBudget == phase = "attribution" => diagElapsed < AttributionBudget
RetainedBreachStopsRestart == phase = "done" => (marker /\ ~admitted /\ failures > 0)
PriorFailuresPreserved == failures >= priorFailures
=============================================================================

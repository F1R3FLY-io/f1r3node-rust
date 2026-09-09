--------------------------- MODULE SoakDiskGuardian ---------------------------
(* The emergency path of one soak iteration in                               *)
(* scripts/run-merge-recovery-soak.sh: the guardian probes, records a        *)
(* breach, stops the writers, attributes the space, and the next segment     *)
(* finds the marker. Eight constants switch the eight corrections on and off *)
(* so that each pre-fix configuration reproduces one historical defect.      *)
EXTENDS Naturals, TLC

CONSTANTS DetectDeath,       \* the watcher treats a dead guardian as a breach
          RejectUnavailable, \* a probe without a sample interrupts the iteration
          EnforceTimeout,    \* df runs under a deadline; late output is discarded
          RecordFirst,       \* the breach record precedes the stop command
          AggregateDeadline, \* attribution has one budget for all roots
          PreserveBreach,    \* a restart keeps the marker and a failure
          EnforceStopDeadline, \* pkill and docker kill run under a deadline
          CheckProgress \* a live guardian without recent progress counts as failed (B20, B21)

ASSUME {DetectDeath, RejectUnavailable, EnforceTimeout, RecordFirst,
        AggregateDeadline, PreserveBreach, EnforceStopDeadline, CheckProgress}
         \subseteq BOOLEAN

ProbeDeadline     == 3  \* two-second timeout plus one-second kill grace
ProbeReturnsAt    == 4  \* a stalled df prints a valid field after the deadline
AttributionBudget == 1  \* SOAK_DISK_DIAGNOSTIC_SECONDS, one unit for every root
StopBudget        == 2  \* SOAK_DISK_STOP_SECONDS: TERM at 1, KILL at 2

VARIABLES phase, alive, interruptRequested, breachRecorded,
          elapsed, timedOut, known,
          stopStarted, stopElapsed, termSent, killSent,
          diagElapsed, rootsLeft,
          marker, priorFailures, failures, admitted,
          stale \* the guardian is alive but its progress record has expired

vars == <<phase, alive, interruptRequested, breachRecorded,
          elapsed, timedOut, known,
          stopStarted, stopElapsed, termSent, killSent,
          diagElapsed, rootsLeft,
          marker, priorFailures, failures, admitted, stale>>

Init ==
    /\ phase = "running"
    /\ alive = TRUE
    /\ interruptRequested = FALSE
    /\ breachRecorded = FALSE
    /\ elapsed = 0
    /\ timedOut = FALSE
    /\ known = FALSE
    /\ stopStarted = FALSE
    /\ stopElapsed = 0
    /\ termSent = FALSE
    /\ killSent = FALSE
    /\ diagElapsed = 0
    /\ rootsLeft \in {1, 3, 32}
    /\ marker = FALSE
    /\ priorFailures \in {0, 2}
    /\ failures = priorFailures
    /\ admitted = FALSE
    /\ stale = FALSE

\* The guardian process is alive but stops making progress (SIGSTOP, a hung
\* probe): its progress record ages past SOAK_GUARDIAN_MAX_SILENCE_SECONDS.
Stall ==
    /\ phase = "running"
    /\ alive
    /\ ~stale
    /\ stale' = TRUE
    /\ UNCHANGED <<phase, alive, interruptRequested, breachRecorded, elapsed,
                   timedOut, known, stopStarted, stopElapsed, termSent, killSent,
                   diagElapsed, rootsLeft, marker, priorFailures, failures, admitted>>

\* The watcher reads the progress record; only the corrected driver treats an
\* expired record as a breach.
WatcherPollStale ==
    /\ phase = "running"
    /\ alive
    /\ stale
    /\ interruptRequested' = CheckProgress
    /\ breachRecorded' = CheckProgress
    /\ phase' = "progress-decided"
    /\ UNCHANGED <<alive, elapsed, timedOut, known, stopStarted, stopElapsed,
                   termSent, killSent, diagElapsed, rootsLeft, marker,
                   priorFailures, failures, admitted, stale>>

\* The guardian process dies while the iteration runs.
Crash ==
    /\ phase = "running"
    /\ alive
    /\ alive' = FALSE
    /\ UNCHANGED <<phase, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft, marker,
                   priorFailures, failures, admitted>>

\* The iteration watcher polls the guardian.
WatcherPoll ==
    /\ phase = "running"
    /\ ~alive
    /\ interruptRequested' = DetectDeath
    /\ breachRecorded' = DetectDeath
    /\ phase' = "watcher-decided"
    /\ UNCHANGED <<alive, elapsed, timedOut, known, stopStarted, stopElapsed, termSent, killSent, diagElapsed,
                   rootsLeft, marker, priorFailures, failures, admitted>>

\* The live guardian starts a df probe.
StartProbe ==
    /\ phase = "running"
    /\ alive
    /\ phase' = "probing"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft, marker,
                   priorFailures, failures, admitted>>

\* The clock advances while df has not returned.
Tick ==
    /\ phase = "probing"
    /\ elapsed < ProbeReturnsAt
    /\ elapsed' = elapsed + 1
    /\ timedOut' = (EnforceTimeout /\ elapsed' = ProbeDeadline)
    /\ phase' = IF timedOut' THEN "sampled" ELSE "probing"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, known, stopStarted, stopElapsed, termSent, killSent,
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
                   stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft, marker, priorFailures,
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
    /\ UNCHANGED <<alive, elapsed, timedOut, stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft,
                   marker, priorFailures, failures, admitted>>

Detect ==
    /\ phase = "breach"
    /\ phase' = IF RecordFirst THEN "record" ELSE "stop"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft, marker,
                   priorFailures, failures, admitted>>

Record ==
    /\ phase = "record"
    /\ breachRecorded' = TRUE
    /\ phase' = "stop"
    /\ UNCHANGED <<alive, interruptRequested, elapsed, timedOut, known,
                   stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft, marker, priorFailures,
                   failures, admitted>>

\* pkill and docker kill start; termination is never confirmed.
BeginStop ==
    /\ phase = "stop"
    /\ stopStarted' = TRUE
    /\ interruptRequested' = TRUE
    /\ phase' = "stopping"
    /\ UNCHANGED <<alive, breachRecorded, elapsed, timedOut, known, stopElapsed,
                   termSent, killSent, diagElapsed, rootsLeft, marker,
                   priorFailures, failures, admitted>>

\* The stop clients stall. Under the deadline, timeout sends TERM at one unit
\* and KILL at two; the guardian moves on whether or not they returned.
StopTick ==
    /\ phase = "stopping"
    /\ stopElapsed < 3
    /\ stopElapsed' = stopElapsed + 1
    /\ termSent' = (EnforceStopDeadline /\ stopElapsed' >= 1)
    /\ killSent' = (EnforceStopDeadline /\ stopElapsed' = StopBudget)
    /\ phase' = IF killSent' THEN "attribution" ELSE "stopping"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, diagElapsed, rootsLeft, marker,
                   priorFailures, failures, admitted>>

StopReturns ==
    /\ phase = "stopping"
    /\ phase' = "attribution"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, stopElapsed, termSent, killSent,
                   diagElapsed, rootsLeft, marker, priorFailures, failures,
                   admitted>>

\* The pre-fix order writes the record after the stop command.
PublishLate ==
    /\ phase \in {"stopping", "attribution", "finished"}
    /\ ~breachRecorded
    /\ breachRecorded' = TRUE
    /\ UNCHANGED <<phase, alive, interruptRequested, elapsed, timedOut, known,
                   stopStarted, stopElapsed, termSent, killSent, diagElapsed,
                   rootsLeft, marker, priorFailures, failures, admitted>>

\* Attribution walks every root; one root can stall for the whole budget.
AttributionTick ==
    /\ phase = "attribution"
    /\ diagElapsed < 2
    /\ diagElapsed' = diagElapsed + 1
    /\ phase' = IF AggregateDeadline /\ diagElapsed' = AttributionBudget
                   THEN "finished" ELSE "attribution"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, stopElapsed, termSent, killSent, rootsLeft, marker, priorFailures,
                   failures, admitted>>

CompleteRoot ==
    /\ phase = "attribution"
    /\ rootsLeft > 0
    /\ rootsLeft' = rootsLeft - 1
    /\ phase' = IF rootsLeft' = 0 THEN "finished" ELSE "attribution"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, stopElapsed, termSent, killSent, diagElapsed, marker, priorFailures,
                   failures, admitted>>

\* The segment ends with the marker on disk; the next segment starts.
Finish ==
    /\ phase = "finished"
    /\ marker' = TRUE
    /\ phase' = "resume"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft, priorFailures,
                   failures, admitted>>

Recover ==
    /\ phase = "resume"
    /\ marker' = PreserveBreach
    /\ failures' = IF PreserveBreach /\ failures = 0 THEN 1 ELSE failures
    /\ phase' = IF PreserveBreach THEN "stopped" ELSE "ready"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft, priorFailures,
                   admitted>>

RestartDecision ==
    /\ phase \in {"stopped", "ready"}
    /\ admitted' = (phase = "ready")
    /\ phase' = "done"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft, marker,
                   priorFailures, failures>>

Next == Stall \/ WatcherPollStale
        \/ (/\ Crash \/ WatcherPoll \/ StartProbe \/ Tick \/ ProbeReturns
               \/ DecideSample \/ Detect \/ Record \/ BeginStop \/ StopTick
               \/ StopReturns \/ PublishLate \/ AttributionTick \/ CompleteRoot
               \/ Finish \/ Recover \/ RestartDecision
            /\ UNCHANGED stale)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"running", "watcher-decided", "progress-decided", "probing",
                  "sampled", "sample-decided", "breach", "record", "stop",
                  "stopping", "attribution", "finished", "resume", "stopped",
                  "ready", "done"}
    /\ alive \in BOOLEAN
    /\ interruptRequested \in BOOLEAN
    /\ breachRecorded \in BOOLEAN
    /\ elapsed \in 0..ProbeReturnsAt
    /\ timedOut \in BOOLEAN
    /\ known \in BOOLEAN
    /\ stopStarted \in BOOLEAN
    /\ stopElapsed \in 0..3
    /\ termSent \in BOOLEAN
    /\ killSent \in BOOLEAN
    /\ diagElapsed \in 0..2
    /\ rootsLeft \in 0..32
    /\ marker \in BOOLEAN
    /\ priorFailures \in {0, 2}
    /\ failures \in 0..2
    /\ admitted \in BOOLEAN
    /\ stale \in BOOLEAN

DeadGuardianRequiresInterrupt ==
    (phase = "watcher-decided" /\ ~alive) => (interruptRequested /\ breachRecorded)
StaleGuardianRequiresInterrupt ==
    (phase = "progress-decided" /\ stale) => (interruptRequested /\ breachRecorded)
InvalidSampleRequiresInterrupt ==
    (phase = "sample-decided" /\ ~known) => (interruptRequested /\ breachRecorded)
ProbeWithinDeadline == phase = "probing" => elapsed < ProbeDeadline
TimedOutSampleRejected == (phase = "sample-decided" /\ timedOut) => ~known
StopRequiresRecord == stopStarted => breachRecorded
StopWithinBudget == phase = "stopping" => stopElapsed < StopBudget
KillFollowsTerm == killSent => termSent
AttributionWithinBudget == phase = "attribution" => diagElapsed < AttributionBudget
RetainedBreachStopsRestart == phase = "done" => (marker /\ ~admitted /\ failures > 0)
PriorFailuresPreserved == failures >= priorFailures
=============================================================================

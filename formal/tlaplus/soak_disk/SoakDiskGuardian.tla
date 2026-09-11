--------------------------- MODULE SoakDiskGuardian ---------------------------
(* The emergency path of one soak iteration in                               *)
(* scripts/run-merge-recovery-soak.sh: the guardian probes, records a        *)
(* breach, stops the writers, attributes the space, and the next segment     *)
(* finds the marker. Fourteen constants switch the corrections on and off    *)
(* so that each pre-fix configuration reproduces one historical defect.      *)
(* Four more constants state the conditional no-overrun theorem: free space  *)
(* stays positive when the hard floor covers the writers' worst consumption   *)
(* over the reaction time and the stop confirms termination.                  *)
EXTENDS Integers, TLC

CONSTANTS DetectDeath,       \* the watcher treats a dead guardian as a breach
          RejectUnavailable, \* a probe without a sample interrupts the iteration
          EnforceTimeout,    \* df runs under a deadline; late output is discarded
          RecordFirst,       \* the breach record precedes the stop command
          AggregateDeadline, \* attribution has one budget for all roots
          PreserveBreach,    \* a restart keeps the marker and a failure
          EnforceStopDeadline, \* pkill and docker kill run under a deadline
          CheckProgress, \* a live guardian without recent progress counts as failed (B20, B21)
          StopOnExit, \* the driver's exit trap stops the node writers it launched (B28)
          RetainStopFailure, \* a rejected stop command is a retained failure and a refusal (B31)
          SelectOwned, \* stop commands select only this run's owner-labeled writers (B30)
          SelectOwnedHost, \* host stops select only processes carrying this run's owner marker (B32, B33)
          MarkOwnedOnly, \* OOM preference is set only on owner-marked host processes (B34)
          ConfigureAtCreation, \* container OOM preference is set at creation, not by a periodic name scan (B36)
          WriteRateMax,     \* MiB the writers can consume per clock unit (measured, not derived)
          SamplePeriod,     \* clock units between guardian probes (the 5s sleep)
          HardFloorMiB,     \* free MiB at the last healthy sample; the breach line
          BoundTermination  \* a completed stop ends consumption (confirmed termination)

ASSUME /\ {DetectDeath, RejectUnavailable, EnforceTimeout, RecordFirst,
           AggregateDeadline, PreserveBreach, EnforceStopDeadline, CheckProgress,
           StopOnExit, BoundTermination, RetainStopFailure, SelectOwned,
           SelectOwnedHost, MarkOwnedOnly, ConfigureAtCreation} \subseteq BOOLEAN
       /\ WriteRateMax \in Nat
       /\ SamplePeriod \in Nat
       /\ HardFloorMiB \in Nat \ {0}

ProbeDeadline     == 3  \* two-second timeout plus one-second kill grace
ProbeReturnsAt    == 4  \* a stalled df prints a valid field after the deadline
AttributionBudget == 1  \* SOAK_DISK_DIAGNOSTIC_SECONDS, one unit for every root
StopBudget        == 2  \* SOAK_DISK_STOP_SECONDS: TERM at 1, KILL at 2
LateUnits         == 3  \* how long unconfirmed writers keep consuming after the stop

\* The reaction time from the last healthy sample to a completed stop, under
\* the corrected deadlines. The theorem's numeric premise: the hard floor must
\* exceed what the writers can consume in that time.
ReactionUnits       == SamplePeriod + ProbeDeadline + StopBudget
FloorCoversReaction == HardFloorMiB > WriteRateMax * ReactionUnits

VARIABLES phase, alive, interruptRequested, breachRecorded,
          elapsed, timedOut, known,
          stopStarted, stopElapsed, termSent, killSent,
          diagElapsed, rootsLeft,
          marker, priorFailures, failures, admitted,
          stale, \* the guardian is alive but its progress record has expired
          exitStop, \* the driver's exit trap stopped the writers (B28)
          freeMiB,      \* free space, consumed at WriteRateMax while the writers run
          writersAlive, \* the writers still consume space
          lateUnits,    \* clock units of unconfirmed consumption after the stop
          unownedStopped, \* a stop command also killed containers this run does not own (B30)
          exitRejected,   \* the exit trap's stop command was rejected by Docker (B31)
          exitFailureRetained, \* that rejection became a counted failure and a refusal (B31)
          unownedHostStopped, \* a host stop also killed processes this run does not own (B32, B33)
          unownedMarked, \* OOM preference was set on processes this run does not own (B34)
          unownedContainersMarked \* OOM preference was set on containers this run does not own (B36)

vars == <<phase, alive, interruptRequested, breachRecorded,
          elapsed, timedOut, known,
          stopStarted, stopElapsed, termSent, killSent,
          diagElapsed, rootsLeft,
          marker, priorFailures, failures, admitted, stale, exitStop,
          freeMiB, writersAlive, lateUnits,
          unownedStopped, exitRejected, exitFailureRetained,
          unownedHostStopped, unownedMarked, unownedContainersMarked>>

Consumption == <<freeMiB, writersAlive, lateUnits>>
StopVars == <<unownedStopped, exitRejected, exitFailureRetained, unownedHostStopped>>

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
    /\ exitStop = FALSE
    /\ freeMiB = HardFloorMiB
    /\ writersAlive = TRUE
    /\ lateUnits = 0
    /\ unownedStopped = FALSE
    /\ exitRejected = FALSE
    /\ exitFailureRetained = FALSE
    /\ unownedHostStopped = FALSE
    /\ unownedMarked = FALSE
    /\ unownedContainersMarked = FALSE

\* B28: the driver exits while an iteration or benchmark runs (a signal, an
\* early exit). Only the corrected EXIT trap stops the writers it launched;
\* the pre-fix driver left them running on the host.
DriverExit ==
    /\ phase = "running"
    /\ phase' = "exiting"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed,
                   timedOut, known, stopStarted, stopElapsed, termSent, killSent,
                   diagElapsed, rootsLeft, marker, priorFailures, failures, admitted,
                   stale, exitStop>>

\* B31: Docker can reject the stop; only the corrected trap records that as a
\* failure and a refusal. B30: the corrected stop selects the containers that
\* carry this run's owner label; the pre-fix stop killed every rnode container.
ExitTrap ==
    /\ phase = "exiting"
    /\ \E rejected \in BOOLEAN :
         /\ exitStop' = StopOnExit
         /\ exitRejected' = (StopOnExit /\ rejected)
         /\ exitFailureRetained' = (StopOnExit /\ rejected /\ RetainStopFailure)
         /\ unownedStopped' = (StopOnExit /\ ~SelectOwned)
         /\ unownedHostStopped' = (StopOnExit /\ ~SelectOwnedHost)
    /\ phase' = "exited"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed,
                   timedOut, known, stopStarted, stopElapsed, termSent, killSent,
                   diagElapsed, rootsLeft, marker, priorFailures, failures, admitted,
                   stale>>

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
\* B34: every guardian sample re-applies the OOM preference; the corrected
\* driver marks only processes that carry this run's owner marker. B36: the
\* container preference is set once at creation through the owner-labeling
\* wrapper; the pre-fix sample marked every container matching a name filter.
StartProbe ==
    /\ phase = "running"
    /\ alive
    /\ phase' = "probing"
    /\ freeMiB' = freeMiB - SamplePeriod * WriteRateMax
    /\ unownedMarked' = ~MarkOwnedOnly
    /\ unownedContainersMarked' = ~ConfigureAtCreation
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft, marker,
                   priorFailures, failures, admitted, writersAlive, lateUnits>>

\* The clock advances while df has not returned.
Tick ==
    /\ phase = "probing"
    /\ elapsed < ProbeReturnsAt
    /\ elapsed' = elapsed + 1
    /\ timedOut' = (EnforceTimeout /\ elapsed' = ProbeDeadline)
    /\ phase' = IF timedOut' THEN "sampled" ELSE "probing"
    /\ freeMiB' = freeMiB - WriteRateMax
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, known, stopStarted, stopElapsed, termSent, killSent,
                   diagElapsed, rootsLeft, marker, priorFailures, failures,
                   admitted, writersAlive, lateUnits>>

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
    /\ unownedStopped' = ~SelectOwned
    /\ unownedHostStopped' = ~SelectOwnedHost
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
    /\ freeMiB' = freeMiB - WriteRateMax
    /\ writersAlive' = (writersAlive /\ ~(killSent' /\ BoundTermination))
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, diagElapsed, rootsLeft, marker,
                   priorFailures, failures, admitted, lateUnits>>

StopReturns ==
    /\ phase = "stopping"
    /\ phase' = "attribution"
    /\ writersAlive' = (writersAlive /\ ~BoundTermination)
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, stopElapsed, termSent, killSent,
                   diagElapsed, rootsLeft, marker, priorFailures, failures,
                   admitted, freeMiB, lateUnits>>

\* Unconfirmed termination: the stop returned, but the writers keep
\* consuming for a bounded number of units afterwards.
LateWrite ==
    /\ phase \in {"attribution", "finished", "resume", "stopped", "ready", "done"}
    /\ writersAlive
    /\ lateUnits < LateUnits
    /\ lateUnits' = lateUnits + 1
    /\ freeMiB' = freeMiB - WriteRateMax
    /\ UNCHANGED <<phase, alive, interruptRequested, breachRecorded, elapsed,
                   timedOut, known, stopStarted, stopElapsed, termSent, killSent,
                   diagElapsed, rootsLeft, marker, priorFailures, failures,
                   admitted, writersAlive>>

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

Next == (DriverExit /\ UNCHANGED <<Consumption, StopVars, unownedMarked, unownedContainersMarked>>)
        \/ (ExitTrap /\ UNCHANGED <<Consumption, unownedMarked, unownedContainersMarked>>)
        \/ ((Stall \/ WatcherPollStale)
            /\ UNCHANGED <<exitStop, Consumption, StopVars, unownedMarked, unownedContainersMarked>>)
        \/ (StartProbe /\ UNCHANGED <<stale, exitStop, StopVars>>)
        \/ ((Tick \/ StopTick \/ StopReturns \/ LateWrite)
            /\ UNCHANGED <<stale, exitStop, StopVars, unownedMarked, unownedContainersMarked>>)
        \/ (BeginStop /\ UNCHANGED <<stale, exitStop, Consumption, exitRejected,
                                      exitFailureRetained, unownedMarked, unownedContainersMarked>>)
        \/ (/\ Crash \/ WatcherPoll \/ ProbeReturns
               \/ DecideSample \/ Detect \/ Record
               \/ PublishLate \/ AttributionTick \/ CompleteRoot
               \/ Finish \/ Recover \/ RestartDecision
            /\ UNCHANGED <<stale, exitStop, Consumption, StopVars, unownedMarked, unownedContainersMarked>>)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"running", "watcher-decided", "progress-decided", "probing",
                  "sampled", "sample-decided", "breach", "record", "stop",
                  "stopping", "attribution", "finished", "resume", "stopped",
                  "ready", "done", "exiting", "exited"}
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
    /\ exitStop \in BOOLEAN
    /\ freeMiB \in Int
    /\ writersAlive \in BOOLEAN
    /\ lateUnits \in 0..LateUnits
    /\ unownedStopped \in BOOLEAN
    /\ exitRejected \in BOOLEAN
    /\ exitFailureRetained \in BOOLEAN
    /\ unownedHostStopped \in BOOLEAN
    /\ unownedMarked \in BOOLEAN
    /\ unownedContainersMarked \in BOOLEAN

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
ExitStopsWriters == phase = "exited" => exitStop
FailedStopRetained == (phase = "exited" /\ exitRejected) => exitFailureRetained
UnownedWritersPreserved == ~unownedStopped
UnownedHostWritersPreserved == ~unownedHostStopped
UnownedPreferencesPreserved == ~unownedMarked
UnownedContainerPreferencesPreserved == ~unownedContainersMarked

\* The conditional theorem. Under FloorCoversReaction and BoundTermination the
\* configuration keeps free space positive on every path; the two assumption
\* controls each drop one premise and violate it.
NoOverrun == freeMiB > 0
=============================================================================

--------------------------- MODULE SoakDiskGuardian ---------------------------
(* The emergency path of one soak iteration in                               *)
(* scripts/run-merge-recovery-soak.sh: the guardian probes, records a        *)
(* breach, stops the writers, attributes the space, and the next segment     *)
(* finds the marker. Twenty-two constants switch the corrections on and off  *)
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
          SurvivesDriverCrash, \* a crash monitor in its own session outlives a killed driver and runs the owner-labeled stop (B38)
          RememberHandledExit, \* the driver's exit handling leaves a marker, and the monitor skips its stop after reading it (B39)
          DetectMonitorDeath, \* the iteration watcher treats a dead crash monitor as a breach, not only a startup failure (B40)
          StopBeforeDrain, \* the interrupted iteration stops the owned writers before it waits for output EOF (B43)
          ManagedContainment, \* a service manager kills the owned writers' control group when both controllers die (B44, launcher prototype)
          VerifyBeforeRelease, \* the launcher verifies manager placement and gate identity before it releases the driver, so a stalled status query prevents native admission (B47, launcher prototype)
          ComposedDeadline, \* the driver's whole emergency response after a breach runs under one composed budget, and a stalled evidence copy is skipped when the budget is spent (B49)
          ValidateAncestors, \* the launcher refuses work when any ancestor of its control directory is not a root-owned, unwritable directory (B50, launcher prototype)
          WriteRateMax,     \* MiB the writers can consume per clock unit (measured, not derived)
          SamplePeriod,     \* clock units between guardian probes (the 5s sleep)
          HardFloorMiB,     \* free MiB at the last healthy sample; the breach line
          BoundTermination  \* a completed stop ends consumption (confirmed termination)

ASSUME /\ {DetectDeath, RejectUnavailable, EnforceTimeout, RecordFirst,
           AggregateDeadline, PreserveBreach, EnforceStopDeadline, CheckProgress,
           StopOnExit, BoundTermination, RetainStopFailure, SelectOwned,
           SelectOwnedHost, MarkOwnedOnly, ConfigureAtCreation,
           SurvivesDriverCrash, RememberHandledExit, DetectMonitorDeath,
           StopBeforeDrain, ManagedContainment, VerifyBeforeRelease,
           ComposedDeadline, ValidateAncestors} \subseteq BOOLEAN
       /\ WriteRateMax \in Nat
       /\ SamplePeriod \in Nat
       /\ HardFloorMiB \in Nat \ {0}

ProbeDeadline     == 3  \* two-second timeout plus one-second kill grace
ProbeReturnsAt    == 4  \* a stalled df prints a valid field after the deadline
AttributionBudget == 1  \* SOAK_DISK_DIAGNOSTIC_SECONDS, one unit for every root
StopBudget        == 2  \* SOAK_DISK_STOP_SECONDS: TERM at 1, KILL at 2
LateUnits         == 3  \* how long unconfirmed writers keep consuming after the stop
EmergencyBudget   == 6  \* SOAK_EMERGENCY_DEADLINE_SECONDS: the stop, the attribution, and the evidence copy together (B49)

\* The reaction time from the last healthy sample to a completed stop, under
\* the corrected deadlines. The theorem's numeric premise: the hard floor must
\* exceed what the writers can consume in that time.
ReactionUnits       == SamplePeriod + ProbeDeadline + StopBudget
FloorCoversReaction == HardFloorMiB > WriteRateMax * ReactionUnits

VARIABLES phase, alive, monitorAlive, interruptRequested, breachRecorded,
          elapsed, timedOut, known,
          stopStarted, stopElapsed, termSent, killSent,
          diagElapsed, rootsLeft,
          marker, priorFailures, failures, admitted,
          stale, \* the guardian is alive but its progress record has expired
          exitStop, \* the driver's exit trap stopped the writers (B28)
          crashStop, \* the crash monitor stopped the owned writers after the driver died without its trap (B38)
          repeatedStop, \* the monitor issued a second stop after the trap had already stopped the writers (B39)
          pipeHeld, \* owned writers still hold the iteration output pipe after the client exited (B43)
          drained, \* the driver waited for output EOF on the interrupted iteration (B43)
          containedStop, \* the service manager stopped the owned writers after both controllers died (B44)
          queryAvailable, \* the launcher's service-status query returned before the release deadline (B47)
          controlTrusted, \* every ancestor of the launcher's control directory is a root-owned, unwritable directory (B50)
          copied, \* the failure-evidence copy completed or was skipped (B49)
          copyElapsed, \* budget units the evidence copy consumed (B49)
          copyStalled, \* the evidence copy command ignores its termination signal (B49)
          freeMiB,      \* free space, consumed at WriteRateMax while the writers run
          writersAlive, \* the writers still consume space
          lateUnits,    \* clock units of unconfirmed consumption after the stop
          unownedStopped, \* a stop command also killed containers this run does not own (B30)
          exitRejected,   \* the exit trap's stop command was rejected by Docker (B31)
          exitFailureRetained, \* that rejection became a counted failure and a refusal (B31)
          unownedHostStopped, \* a host stop also killed processes this run does not own (B32, B33)
          unownedMarked, \* OOM preference was set on processes this run does not own (B34)
          unownedContainersMarked \* OOM preference was set on containers this run does not own (B36)

vars == <<phase, alive, monitorAlive, interruptRequested, breachRecorded,
          elapsed, timedOut, known,
          stopStarted, stopElapsed, termSent, killSent,
          diagElapsed, rootsLeft,
          marker, priorFailures, failures, admitted, stale, exitStop, crashStop, repeatedStop,
          pipeHeld, drained, containedStop, queryAvailable, controlTrusted,
          copied, copyElapsed, copyStalled,
          freeMiB, writersAlive, lateUnits,
          unownedStopped, exitRejected, exitFailureRetained,
          unownedHostStopped, unownedMarked, unownedContainersMarked>>

Consumption == <<freeMiB, writersAlive, lateUnits>>
StopVars == <<unownedStopped, exitRejected, exitFailureRetained, unownedHostStopped>>
DrainVars == <<pipeHeld, drained, containedStop>>

Init ==
    /\ phase = "launch"
    /\ alive = TRUE
    /\ monitorAlive = TRUE
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
    /\ crashStop = FALSE
    /\ repeatedStop = FALSE
    /\ pipeHeld = TRUE
    /\ drained = FALSE
    /\ containedStop = FALSE
    /\ queryAvailable \in BOOLEAN
    /\ controlTrusted \in BOOLEAN
    /\ copied = FALSE
    /\ copyElapsed = 0
    /\ copyStalled \in BOOLEAN
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

\* B39: after the trap completes its exit handling it leaves a marker, and
\* the crash monitor reads it before acting. The pre-fix monitor issued a
\* second stop on every driver exit.
MonitorObservesExit ==
    /\ phase = "exited"
    /\ repeatedStop' = (exitStop /\ ~RememberHandledExit)
    /\ phase' = "exit-checked"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed,
                   timedOut, known, stopStarted, stopElapsed, termSent, killSent,
                   diagElapsed, rootsLeft, marker, priorFailures, failures, admitted,
                   stale, exitStop, crashStop>>

\* The driver is killed (SIGKILL, OOM) with an iteration in flight, so its
\* EXIT trap never runs. Only a crash monitor in its own session outlives it;
\* the pre-fix monitor shared the driver's process group and died with it.
DriverCrash ==
    /\ phase = "running"
    /\ phase' = "crashed"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed,
                   timedOut, known, stopStarted, stopElapsed, termSent, killSent,
                   diagElapsed, rootsLeft, marker, priorFailures, failures, admitted,
                   stale, exitStop, crashStop, repeatedStop>>

\* B44: the driver and the crash monitor die together, so no in-tree
\* supervisor survives. Only a service manager that owns the writers' control
\* group can stop them; the launcher prototype provides one, and the direct
\* launch does not. The driver is unchanged, and B44 stays open on the source.
\* The launcher's release gate (B47, B50, launcher prototype). The corrected
\* launcher starts a trusted gate, verifies the manager placement and the gate
\* identity, and only then releases the driver. A status query that never
\* returns refuses the launch, and so does an untrusted ancestor of the
\* control directory (B50). The pre-fix launcher started the driver first
\* and checked only the immediate parent.
Release ==
    /\ phase = "launch"
    /\ phase' = IF /\ ~VerifyBeforeRelease \/ queryAvailable
                   /\ ~ValidateAncestors \/ controlTrusted
                THEN "running" ELSE "launch-refused"
    /\ UNCHANGED <<alive, monitorAlive, interruptRequested, breachRecorded, elapsed, timedOut, known, stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft, marker, priorFailures, failures, admitted, stale, exitStop, crashStop, repeatedStop, pipeHeld, drained, containedStop, freeMiB, writersAlive, lateUnits, unownedStopped, exitRejected, exitFailureRetained, unownedHostStopped, unownedMarked, unownedContainersMarked, queryAvailable>>

ControllerLoss ==
    /\ phase = "running"
    /\ monitorAlive
    /\ monitorAlive' = FALSE
    /\ phase' = "controllers-lost"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed,
                   timedOut, known, stopStarted, stopElapsed, termSent, killSent,
                   diagElapsed, rootsLeft, marker, priorFailures, failures, admitted,
                   stale, exitStop, crashStop, repeatedStop, pipeHeld, drained, containedStop>>

ContainmentResponse ==
    /\ phase = "controllers-lost"
    /\ containedStop' = ManagedContainment
    /\ phase' = "containment-checked"
    /\ UNCHANGED <<alive, monitorAlive, interruptRequested, breachRecorded, elapsed,
                   timedOut, known, stopStarted, stopElapsed, termSent, killSent,
                   diagElapsed, rootsLeft, marker, priorFailures, failures, admitted,
                   stale, exitStop, crashStop, repeatedStop, pipeHeld, drained>>

\* The surviving monitor runs the same owner-selecting stop as the exit trap.
CrashMonitor ==
    /\ phase = "crashed"
    /\ crashStop' = SurvivesDriverCrash
    /\ unownedStopped' = (SurvivesDriverCrash /\ ~SelectOwned)
    /\ unownedHostStopped' = (SurvivesDriverCrash /\ ~SelectOwnedHost)
    /\ phase' = "crash-checked"
    /\ UNCHANGED <<alive, interruptRequested, breachRecorded, elapsed,
                   timedOut, known, stopStarted, stopElapsed, termSent, killSent,
                   diagElapsed, rootsLeft, marker, priorFailures, failures, admitted,
                   stale, exitStop, repeatedStop>>

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

\* B40: the crash monitor dies while the iteration runs. The pre-fix driver
\* checked the monitor only at startup; the corrected watcher treats its
\* death as a breach, so termination is unconfirmed and work is refused.
MonitorCrash ==
    /\ phase = "running"
    /\ monitorAlive
    /\ monitorAlive' = FALSE
    /\ UNCHANGED <<phase, alive, interruptRequested, breachRecorded, elapsed, timedOut,
                   known, stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft, marker,
                   priorFailures, failures, admitted>>

WatcherPollMonitor ==
    /\ phase = "running"
    /\ ~monitorAlive
    /\ interruptRequested' = DetectMonitorDeath
    /\ breachRecorded' = DetectMonitorDeath
    /\ phase' = "monitor-decided"
    /\ UNCHANGED <<alive, monitorAlive, elapsed, timedOut, known, stopStarted, stopElapsed, termSent, killSent, diagElapsed,
                   rootsLeft, marker, priorFailures, failures, admitted>>

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

\* B43: the interrupted iteration's client has exited, and the driver waits
\* for the output pipe to reach EOF. Owned writers still hold that pipe. The
\* corrected driver stops them first; the pre-fix driver drained first and hung.
Drain ==
    /\ phase = "finished"
    /\ ~drained
    /\ drained' = TRUE
    /\ pipeHeld' = (pipeHeld /\ ~StopBeforeDrain)
    /\ UNCHANGED <<phase, alive, monitorAlive, interruptRequested, breachRecorded,
                   elapsed, timedOut, known, stopStarted, stopElapsed, termSent, killSent,
                   diagElapsed, rootsLeft, marker, priorFailures, failures, admitted,
                   stale, exitStop, crashStop, repeatedStop, containedStop, Consumption, StopVars,
                   unownedMarked, unownedContainersMarked>>

\* The segment ends with the marker on disk; the next segment starts.
\* The driver's failure-evidence copy after the attribution (B49). The
\* corrected driver runs the whole response under one composed budget that
\* starts at the breach decision: when the budget is spent, a stalled copy is
\* skipped and recorded, and the summary still publishes. The pre-fix driver
\* had no bound on the copy, so one stalled root delayed everything after it.
ResponseElapsed == stopElapsed + diagElapsed + copyElapsed

CopyEvidence ==
    /\ phase = "finished"
    /\ ~copied
    /\ ~copyStalled
    /\ copied' = TRUE
    /\ copyElapsed' = copyElapsed + 1
    /\ UNCHANGED <<phase, alive, monitorAlive, interruptRequested, breachRecorded, elapsed, timedOut, known, stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft, marker, priorFailures, failures, admitted, stale, exitStop, crashStop, repeatedStop, pipeHeld, drained, containedStop, queryAvailable, controlTrusted, copyStalled, freeMiB, writersAlive, lateUnits, unownedStopped, exitRejected, exitFailureRetained, unownedHostStopped, unownedMarked, unownedContainersMarked>>

StallCopy ==
    /\ phase = "finished"
    /\ ~copied
    /\ copyStalled
    /\ ~ComposedDeadline \/ ResponseElapsed < EmergencyBudget
    /\ copyElapsed' = copyElapsed + 1
    /\ UNCHANGED <<phase, alive, monitorAlive, interruptRequested, breachRecorded, elapsed, timedOut, known, stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft, marker, priorFailures, failures, admitted, stale, exitStop, crashStop, repeatedStop, pipeHeld, drained, containedStop, queryAvailable, controlTrusted, copyStalled, freeMiB, writersAlive, lateUnits, unownedStopped, exitRejected, exitFailureRetained, unownedHostStopped, unownedMarked, unownedContainersMarked, copied>>

SkipCopy ==
    /\ phase = "finished"
    /\ ~copied
    /\ copyStalled
    /\ ComposedDeadline
    /\ ResponseElapsed >= EmergencyBudget
    /\ copied' = TRUE
    /\ UNCHANGED <<phase, alive, monitorAlive, interruptRequested, breachRecorded, elapsed, timedOut, known, stopStarted, stopElapsed, termSent, killSent, diagElapsed, rootsLeft, marker, priorFailures, failures, admitted, stale, exitStop, crashStop, repeatedStop, pipeHeld, drained, containedStop, queryAvailable, controlTrusted, copyStalled, freeMiB, writersAlive, lateUnits, unownedStopped, exitRejected, exitFailureRetained, unownedHostStopped, unownedMarked, unownedContainersMarked, copyElapsed>>

Finish ==
    /\ phase = "finished"
    /\ copied
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

NextCore == (DriverExit /\ UNCHANGED <<monitorAlive, DrainVars, Consumption, StopVars, unownedMarked, unownedContainersMarked, crashStop, repeatedStop>>)
        \/ (ExitTrap /\ UNCHANGED <<monitorAlive, DrainVars, Consumption, unownedMarked, unownedContainersMarked, crashStop, repeatedStop>>)
        \/ (MonitorObservesExit /\ UNCHANGED <<monitorAlive, DrainVars, Consumption, StopVars, unownedMarked, unownedContainersMarked>>)
        \/ (DriverCrash /\ UNCHANGED <<monitorAlive, DrainVars, Consumption, StopVars, unownedMarked, unownedContainersMarked>>)
        \/ ((ControllerLoss \/ ContainmentResponse)
            /\ UNCHANGED <<Consumption, StopVars, unownedMarked, unownedContainersMarked>>)
        \/ (CrashMonitor /\ UNCHANGED <<monitorAlive, DrainVars, Consumption, exitRejected, exitFailureRetained,
                                         unownedMarked, unownedContainersMarked>>)
        \/ ((Stall \/ WatcherPollStale)
            /\ UNCHANGED <<monitorAlive, DrainVars, exitStop, crashStop, repeatedStop, Consumption, StopVars, unownedMarked, unownedContainersMarked>>)
        \/ (StartProbe /\ UNCHANGED <<monitorAlive, DrainVars, stale, exitStop, crashStop, repeatedStop, StopVars>>)
        \/ ((Tick \/ StopTick \/ StopReturns \/ LateWrite)
            /\ UNCHANGED <<monitorAlive, DrainVars, stale, exitStop, crashStop, repeatedStop, StopVars, unownedMarked, unownedContainersMarked>>)
        \/ (BeginStop /\ UNCHANGED <<monitorAlive, DrainVars, stale, exitStop, crashStop, repeatedStop, Consumption, exitRejected,
                                      exitFailureRetained, unownedMarked, unownedContainersMarked>>)
        \/ ((MonitorCrash \/ WatcherPollMonitor)
            /\ UNCHANGED <<DrainVars, stale, exitStop, crashStop, repeatedStop, Consumption, StopVars, unownedMarked, unownedContainersMarked>>)
        \/ Drain
        \/ Release
        \/ (/\ Crash \/ WatcherPoll \/ ProbeReturns
               \/ DecideSample \/ Detect \/ Record
               \/ PublishLate \/ AttributionTick \/ CompleteRoot
               \/ Finish \/ Recover \/ RestartDecision
            /\ UNCHANGED <<monitorAlive, DrainVars, stale, exitStop, crashStop, repeatedStop, Consumption, StopVars, unownedMarked, unownedContainersMarked>>)

\* The launcher's query outcome, the control-directory trust, and the copy
\* command's behavior are fixed at launch. The copy steps change nothing else.
Next == \/ NextCore /\ UNCHANGED <<queryAvailable, controlTrusted, copied, copyElapsed, copyStalled>>
        \/ CopyEvidence \/ StallCopy \/ SkipCopy

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"running", "watcher-decided", "monitor-decided", "progress-decided", "probing",
                  "sampled", "sample-decided", "breach", "record", "stop",
                  "stopping", "attribution", "finished", "resume", "stopped",
                  "ready", "done", "exiting", "exited", "exit-checked",
                  "crashed", "crash-checked", "controllers-lost", "containment-checked",
                  "launch", "launch-refused"}
    /\ alive \in BOOLEAN
    /\ monitorAlive \in BOOLEAN
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
    /\ crashStop \in BOOLEAN
    /\ repeatedStop \in BOOLEAN
    /\ pipeHeld \in BOOLEAN
    /\ drained \in BOOLEAN
    /\ containedStop \in BOOLEAN
    /\ queryAvailable \in BOOLEAN
    /\ controlTrusted \in BOOLEAN
    /\ copied \in BOOLEAN
    /\ copyElapsed \in Nat
    /\ copyStalled \in BOOLEAN
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
DeadMonitorRequiresInterrupt ==
    (phase = "monitor-decided" /\ ~monitorAlive) => (interruptRequested /\ breachRecorded)
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
CrashStopsOwnedWriters == phase = "crash-checked" => crashStop
HandledExitHasNoExtraStop == phase = "exit-checked" => ~repeatedStop
DrainRequiresOwnedStop == drained => ~pipeHeld
ControllerLossStopsOwnedWriters == phase = "containment-checked" => containedStop
UnavailableQueryPreventsRelease == ~queryAvailable => phase \in {"launch", "launch-refused"}
UntrustedControlPreventsRelease == ~controlTrusted => phase \in {"launch", "launch-refused"}
ResponseWithinDeadline == ResponseElapsed <= EmergencyBudget
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

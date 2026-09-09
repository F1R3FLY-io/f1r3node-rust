# Guardian Progress Correspondence

## Active work: B20 and B21

A live process can stop progress without exiting. These cycles suspend the guardian through `SIGSTOP`, rather than killing it.

B20 holds a benchmark client that ignores `TERM`. B21 holds the iteration client. Independent observers check cancellation and summary publication before release after twelve seconds.

Both observers confirm that the guardian remains suspended at observation. No node writers start.

The driver initializes a progress timestamp before guardian startup. This timestamp supplies the startup grace, not evidence of a completed guardian sample.

The guardian replaces that timestamp after its disk checks. Atomic rename prevents the parent from reading a partially written timestamp.

The parent reads elapsed host time from `/proc/uptime`. It rejects missing, invalid, future, or expired timestamps when it checks progress.

`SOAK_GUARDIAN_MAX_SILENCE_SECONDS` permits integers from 8 through 30. Its default is ten seconds. The fixtures use eight seconds.

The model keeps `guardianAlive` true and prevents progress. `Tick` increases the age, and `Observe` represents the parent's response after expiration.

The negative control disables the progress check. It requires exit 12 and `StaleGuardianRequiresInterrupt`. The corrected model has five distinct states.

B21 reuses the unchanged model and control after its own production RED. Its baseline includes the retained B20 correction.

## Admission: B22

B22 pauses the driver and guardian during a valid admission probe. The disk shim excludes probes that originate from the guardian.

After nine seconds, the observer confirms both suspended processes and the expired progress timestamp. It resumes the driver before the guardian.

Separate fixtures exercise iteration and opening benchmark admission. Both baseline paths admit work with stale progress, despite later active supervision.

The correction checks progress after the admission probe, before either work counter increments. Both fixtures then refuse admission and record one protection failure.

`GuardianProgressAdmission` represents ages below, equal to, and above its limit. The negative control disables freshness enforcement.

The control requires exit 12 and `StaleProgressPreventsAdmission`. The corrected model has six distinct states and records refusal.

## Limits

These finite models assume clock progress and completing parent decisions. Abstract ticks do not establish a production deadline or scheduler guarantee.

The active fixtures confirm client cancellation and local failure publication. They do not establish termination of every writer or successful artifact upload.

Atomic rename is not durable storage. Filesystem stalls, publication errors, process identifier reuse, and stale records from another process still need verification.

Progress checks and work launch are not atomic. This cycle does not cover every later crash, interleaved scheduling pause, or cleanup boundary.

The configured silence limit is a component limit. The complete response deadline, writer-growth reserve, hosted checks, and maintainer review remain pending.

D2, claim discharge, and acceptance remain pending.

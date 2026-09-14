# B35 Benchmark Recovery and B36 Docker Preferences

## Status

B35 passes unchanged-production characterization.
B36 passes its selected production and formal RED/GREEN cycle.
D2, all four claims, all nine gates, and soak acceptance remain pending.
The workload and 45-second finalization wait remain unchanged.
No real node workload, hosted dispatch, or acceptance soak ran in these tests.

The current test source starts from `980285d4c5620929566ff783a77fabf90292b8b3`.
Its archive includes a current inventory overlay that changes source bindings only.
The archive passed inventory validation before transfer and after extraction.
Source digests, not the commit identifier alone, identify the archive.
External commits and their hooks are not attested by this session.

## B35 characterization

The fixture runs the production benchmark and driver with a restricted busybox writer.
An external Docker fault rejects `kill` and Compose `down` with exit 42.
The initial stop retains an unconfirmed-termination marker while the writer remains running.
Two restarts refuse new work and preserve these counters:

| Counter | Value after each restart |
| --- | ---: |
| `iterations` | 0 |
| `failures` | 1 |
| `bench_segments` | 1 |

The retained baseline and current driver both pass this characterization.
B35 required no production repair and has no fabricated formal counterexample.
The fixture removes its recorded container after the observations.
That fixture cleanup does not prove production writer termination.
A benchmark crash before any outcome or stop record remains untested.

## B36 RED/GREEN

The original production and formal counterexamples preceded the correction.
The original GREEN transfer failed after the earlier VM expired, before remote GREEN execution.
The new batch repeats the production counterexamples and tests the corrected driver on one fresh guarded VM.

| Launch | Phase | Unrelated before | Unrelated after | Workload after | Fixture exit |
| --- | --- | ---: | ---: | ---: | ---: |
| Docker `run` | RED | 0 | 1000 | 1000 | 1 |
| Compose `up` | RED | 0 | 1000 | 1000 | 1 |
| Docker `run` | GREEN | 0 | 0 | 1000 | 0 |
| Compose `up` | GREEN | 0 | 0 | 1000 | 0 |

The correction configures the workload preference at container creation instead of periodically selecting Docker processes by name.
Both GREEN cases also stop the workload writer and preserve the unrelated writer and its file growth.
Actual memory samples remain healthy throughout these cases.
The exact formal control violates `UnownedContainerPreferencesPreserved` with exit 12.
The positive model completes with three distinct states.

The [model correspondence](../../../formal/tlaplus/soak_disk/DockerOomOwnership.md) defines the cooperative creation contract and its limits.
Conflicting caller options, other creation variants, direct APIs, existing containers, and concurrent creation require further checks.
A preference value does not guarantee runner survival during memory exhaustion.

## Composed verification

The final real-system batch contains 12 cases:

- Two B36 production counterexamples and two corrected preference cases.
- B35 characterization against the unchanged baseline and current driver.
- B30 stop ownership with Docker `run` and Compose.
- B31 rejected-stop retention.
- B27 Docker resource preservation, B28 exit shutdown, and B29 crash recovery.

All 12 cases match their expected outcomes.
The B30 default-configuration cases retain creation preference zero with host-memory protection disabled.
B29 still uses fixture cleanup for the writer that survives the driver crash.
That cleanup remains outside the production guarantee.

Current-source regressions also pass:

- 31 positive configurations and 33 exact negative controls.
- 64 actual TLC logs, saved before classifier or routing substitutes ran.
- 231 classifier cases and six routing scenarios.
- 41 isolated emergency cases and the supporting regression suite.
- Seven primary language-server checks without diagnostics.

The input audit compares 211 executed verification files with the current source.
Other source snapshots identify captured bytes, not execution coverage.

## Evidence and setup failures

The final archive contains 421 unique regular files.
The archive audit rejects unsafe paths, duplicate names, special entries, and excessive expansion before extraction.
Every extracted file matches its archived digest.
Both VMs launched during this continuation were observed terminated.

An initial local archive used GNU extension entries and failed validation before VM launch.
A repeated preparation failed its input-count guard before VM launch.
The first new VM lacked Ruby and stopped before behavior tests.
The final VM installs Ruby for inventory validation and passes the batch.
These setup failures are not behavioral RED.
The original B35 working-directory failure also remains separate from its passing characterization.

The manifest records original hashes, published hashes, and counts for every path substitution.
Raw archives, source snapshots, provisioning records, and complete local evidence remain outside Git and Cargo `target/`.
Historical evidence bytes and claim statuses remain unchanged.

## Remaining obligations

Full D2 completion still requires storage-fault tests, remaining crash windows, complete shutdown evidence, and one aggregate emergency deadline.
D3 must establish all-writer growth and reserve bounds.
Hosted checks, enforcement, maintainer review, claim ratification, and the separately authorized 60-hour acceptance soak remain pending.

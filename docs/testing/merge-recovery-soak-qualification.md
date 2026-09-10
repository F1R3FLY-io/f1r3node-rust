# Merge recovery soak qualification

## Purpose and status

The PR 216 qualification requires at least 86,400 qualified seconds after a successful full integration preflight.
The scheduled daily profile remains a separate 79,200-second run.
The scheduled weekend profile remains unchanged.

The manual `qualification-24h` profile implements this contract in the existing workflow.
Local model, collector, driver, verifier, and workflow tests check the connection.
These tests do not replace the actual 24-hour qualification run.

## Scope of the result

The existing driver repeats merge recovery tests with Docker and subprocess providers.
Each iteration can create a new shard.
A qualified run therefore demonstrates repeated merge recovery operation, not uninterrupted survival of one shard.
It does not establish month-long uptime, a failure probability, or Casper correctness for all executions.

A qualified run uses one uninterrupted driver session on one runner boot.
Qualification cannot combine restarted jobs, checkpoint segments, different revisions, or separate runs.
Preflight, setup outside test execution, idle time, and artifact uploads do not contribute qualified seconds.
An iteration contributes its supervised monotonic duration only after all its tests pass and its evidence passes validation.
The final iteration must finish normally, even when the duration threshold occurs during that iteration.
A timeout at the threshold does not qualify that iteration.

## Fixed identity

The collector records the initial identity before test execution.
The verifier also requires that separately recorded identity, not only the identity supplied inside the event trace.
The initial identity binds these values for the complete run.

| Field | Meaning |
|---|---|
| `target_ref` | The requested source reference. |
| `target_sha` | The complete node source commit identifier. |
| `control_sha` | The complete workflow and driver commit identifier. |
| `harness_sha` | The complete integration harness commit identifier. |
| `image_id` | The Docker content identifier, including its `sha256:` prefix. |
| `binary_sha256` | The digest of the extracted subprocess executable. |
| `configuration_sha256` | The digest of the complete recorded effective configuration. |
| `seeds` | The ordered list of explicit Python hash seeds. |
| `run_id`, `run_attempt` | The GitHub run identity. Qualification requires attempt one. |
| `session_id`, `boot_id` | The driver session and runner boot identifiers. |

The collector must use the immutable image identifier, not a mutable image tag.
It must compare source references and effective inputs at each iteration boundary and before final publication.
An observed reference change permanently invalidates that run, even if the reference later returns to its original value.
An unreadable identity cannot establish equality and must fail qualification.

Boundary observations do not prove that an external reference never changed between observations.
Immutable executable identities protect the actual tested candidate from that observation limit.
The manifest must not claim continuous observation of remote Git references.

The current pinned load test has deterministic load phases and no general random-seed option.
The qualification profile sets `PYTHONHASHSEED=216` for preflight and workload execution.
The collector supports an ordered list of unsigned 32-bit hash seeds.
These seeds do not control cryptographic randomness, validator scheduling, network timing, or operating-system scheduling.

The configuration artifact records the workload, provider schedule, timeouts, selected environment settings, defaults digest, workload-source digest, and harness lock digest.
The contract assumes a trusted runner software environment.
It does not certify arbitrary changes to the operating system or dependencies outside the recorded inputs.

## Evidence protocol

The input document has schema version one and scope `merge-recovery-iterations`.
Its required duration is exactly 86,400 seconds.
It contains the initial identity, effective configuration, preflight evidence, and an ordered event list.
The configuration artifact digest must match the initial identity.
Each event repeats the observed identity and carries an integer monotonic timestamp in nanoseconds.

The event sequence starts with `start` and ends with exactly one `finish`.
Between these events, each `iteration_start` has one matching `iteration_end`.
Iteration identifiers increase from one without gaps.
The provider alternates between `docker` and `subprocess`.
The seed follows the recorded seed list in order and repeats when necessary.
Both providers must complete at least one successful iteration.

Each completed iteration records its exit code, JUnit report, metrics document, and pytest log.
Each artifact reference contains a relative path and its SHA-256 digest.
The verifier rejects missing files, changed digests, symbolic links, path traversal, and reused artifact paths.
It checks the actual JUnit test cases instead of trusting only summary counters.
Failures, errors, skipped tests, empty reports, and inconsistent suite counts invalidate the run.
The metrics document must identify the same iteration and provider with a successful exit and nonempty resource measurements.

The final event requires outcome `completed`.
Cancellation, forced finalization, protection breaches, input changes, and unknown events cannot produce a qualifying manifest.
Missing or extra events fail validation.
The external workflow conclusion must also be `success`.
An offline manifest alone cannot override a cancelled or failed workflow.
The collector's local result always reports `qualified: false` until the external job conclusion is available.
Its separate `trace_complete` field records whether local evidence collection finished normally.

## Duration invariant

Let $`b_i`$ and $`e_i`$ denote the monotonic start and end timestamps of successful iteration $`i`$.
Let $`Q`$ denote qualified nanoseconds and $`R`$ denote the required nanoseconds.

```math
Q = \sum_i (e_i - b_i), \qquad R = 86{,}400 \times 10^9.
```

The verifier requires ordered, disjoint intervals with $`e_i > b_i`$.
It requires $`Q \ge R`$ and at least $`R`$ elapsed nanoseconds within the same session.
Idle intervals cannot increase $`Q`$.
Integer arithmetic prevents rounding a shortened run up to the threshold.

For example, 86,399 seconds and 999,999,999 additional nanoseconds must fail.
Two 12-hour runs must also fail because they have different sessions.
A 25-hour run with only 23 hours of completed test execution must fail.

## Verification boundaries

The TLA+ model explores independent work completion, idle time, input invalidation, cancellation, and final publication.
Its safe configuration checks that publication implies every qualification predicate.
Its unsafe configuration permits elapsed time alone to establish success and must produce a counterexample.
The small duration bound represents threshold classes, not a claim that two seconds demonstrate real endurance.

The Rocq model proves duration and failure-preservation properties over arbitrary finite event lists and natural-number durations.
Generated tests apply those properties to the offline verifier at nanosecond precision.
The implementation tests also exercise malformed and missing artifacts that the abstract models do not parse.
These layers prove different claims and do not replace the actual 24-hour workload.

The model assumes a trusted collector, a monotonic clock within one boot, and an authentic external workflow conclusion.
It does not prove operating-system clock correctness, GitHub integrity, cryptographic collision resistance, or node liveness.
Loom does not apply to this single-threaded Python evidence verifier.
Casper concurrency tests remain separate requirements.

| Invariant | Formal evidence | Implementation test |
|---|---|---|
| The duration threshold cannot decrease. | `qualification_requires_full_duration`, `subthreshold_never_qualifies` | Generated tests at one nanosecond below, at, and above the threshold. |
| Idle time cannot produce credit. | `idle_does_not_create_credit`, `CreditBoundedByElapsed` | A long idle interval with insufficient work must fail. |
| A later success cannot erase failure. | `invalidation_survives_any_suffix`, `FailureIsPermanent` | Invalidating events at each sequence position must fail. |
| Publication requires all acceptance conditions. | `QualifiedImpliesContract` | Missing events, wrong identities, interrupted sessions, and invalid evidence must fail. |
| Credit uses exact integer arithmetic. | `run_duration` over natural numbers | Large integer and nanosecond-boundary tests must agree. |

The [Rocq source](../../formal/rocq/soak_qualification/SoakQualification.v) proves nine propositions without additional axioms.
The [TLA+ source](../../formal/tlaplus/soak_qualification/SoakQualification.tla) checks the publication state machine.
The [verifier tests](../../scripts/soak/test_qualification.py) connect the abstract predicates to concrete event and artifact validation.
Collector tests verify source changes, image changes, configuration changes, driver replacement, and permanent rejection after restored inputs.
Workflow tests execute the actual schedule and final-gate shell bodies.
The real-driver fixture uses a simulated monotonic clock and fake workload tools.
It verifies duration bookkeeping and artifact flow without pretending that the fixture ran for 24 hours.

## Local verification

Run the formal gate inside a bounded systemd scope.

```bash
systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 -p CPUQuota=100% \
  bash scripts/check-soak-qualification-models.sh
```

Run the offline verifier tests inside a separate bounded scope.

```bash
systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 -p CPUQuota=100% \
  bash scripts/test-merge-recovery-soak-qualification.sh
```

Both commands preserve logs and input hashes under `target/verification/soak-qualification/`.
Neither command starts node processes or dispatches a GitHub workflow.
Keep the combined limits of concurrent heavy commands within the available memory budget.

The test gate also runs the existing driver regressions and workflow invariant checker.
New Python and shell gate files require clean lint results.
The modified driver must add no ShellCheck diagnostics compared with its committed baseline.
Four existing driver warnings remain outside this repair: three dependent `local` assignments and one unchecked directory change.
The baseline comparison does not suppress newly introduced diagnostics.

The offline verifier consumes an event trace, its separately pinned identity, and the associated evidence directory.
The workflow conclusion must come from the external workflow result, not from the test trace.

```bash
python3 scripts/soak/qualification.py evidence/trace.json \
  --expected-identity evidence/identity.json \
  --evidence-root evidence \
  --workflow-conclusion "$WORKFLOW_CONCLUSION" \
  --output evidence/qualification.json
```

A zero exit code means that the supplied evidence satisfies this contract under its stated trust assumptions.
A nonzero exit code rejects qualification.
When input validation fails, the command emits a negative manifest if the output location remains writable.
Missing output is never a successful result.

## Workflow operation

The manual qualification path disables scheduled checkpoint segmentation and automatic remainder-window retries.
It preserves the full duration after preflight and finishes the last test iteration normally.
Each iteration retains the existing 1,200-second pytest timeout and has a 1,800-second outer timeout.
An outer timeout rejects qualification, even after the minimum duration.

The job has a 32-hour limit for build, preflight, qualified execution, and artifact handling.
Its runner lease includes the existing two-hour cleanup margin after that control window.
These control limits do not reduce the required qualified duration.
Failure to obtain 86,400 qualified seconds within those limits fails the run.

The qualification profile uses a separate concurrency group.
It does not publish into the scheduled daily or weekend comparison series.
Raw evidence and the final qualification manifest have 30-day artifact retention.
The existing daily and weekend scheduling policies retain their current behavior.

The qualifying run must not start until the assigned CI and cost-accounting repairs pass their required gates.
No workflow dispatch occurs as part of verifier development.

After the prerequisite gates pass, dispatch the profile against the exact candidate commit.

```bash
gh workflow run merge-recovery-soak.yml \
  --ref feature/cost-accounted-rho \
  -f target_ref="$CANDIDATE_SHA" \
  -f duration=qualification-24h
```

`CANDIDATE_SHA` must contain the full reviewed commit identifier.
The workflow revision must contain the qualification implementation.
Treat the `Verify 24-hour Qualification` job and its manifest as the qualification result.
A successful preflight status, local collector result, or scheduled daily run is not sufficient.

---
doc_type: work_log
task: pr216-soak-qualification-hardening
status: claimed_done
handoff_status: ready
date: 2026-09-08
next_steps:
  - Complete the separate pgmcp machine-verification requirement.
  - Continue assigned campaign repairs before additional audit findings.
---

# PR 216 soak qualification

## Priority and scope

The user requires assigned campaign work before additional audit findings unless those findings have a demonstrated blocking relationship.
Scheduler revision 657 selected this existing task after the additional state-import audit received its follow-up placement.
The task remains in progress.
No new state-import checks or production repairs started during this increment.

The current daily profile requests 79,200 seconds despite its `daily-24h` label.
Preflight also reduces that budget.
The current driver can stop successfully at the deadline during an unfinished iteration.
The daily profile can end early after target movement.
These behaviors cannot establish this campaign's required 86,400-second qualification.

The scheduled profiles have separate operational purposes.
Their existing policy remains unchanged.
The new [qualification contract](../testing/merge-recovery-soak-qualification.md) defines a separate manual path.
The existing workload recreates shards between iterations and cannot establish continuous survival of one shard.

## Formal evidence before implementation

The formal gate passed in `target/verification/soak-qualification/models.WApmNR`.
The systemd invocation identifier was `46941539bd3449d8a2694187b1bb819e`.
Its memory limit was 2 GiB, with no swap and one CPU equivalent.

Rocq compiled and kernel-checked seven propositions.
All seven assumption checks reported a closed global context.
The proofs cover arbitrary finite credit sequences, exact duration, idle neutrality, and permanent invalidation.

The safe TLA+ configuration generated 859 states and explored 449 distinct states.
The search reached depth 13 with an empty queue.
The elapsed-only unsafe configuration violated `QualifiedImpliesContract` as required.
The configured model does not establish real endurance or quantify node reliability.

Two earlier proof compilation attempts failed before model checking.
Their evidence remains in `models.HikTpN` and `models.gBo7gn`.
The corrections addressed proof reduction steps without changing the stated propositions.

## Offline verifier

The verifier requires one session, immutable input identities, successful preflight, both providers, explicit seeds, and complete iteration evidence.
It computes qualified nanoseconds from completed iteration intervals.
It rejects failed, skipped, cancelled, interrupted, reordered, incomplete, and shortened traces.
Artifact hashes bind actual JUnit reports, resource metrics, logs, and effective configuration.
The expected identity comes from a separate input to prevent consistent substitution of all trace identities.

The first test gate passed 20 tests in `verifier.bS75sr`.
The expanded gate passed 26 tests in `verifier.50SE5k`.
The latter invocation identifier was `30c614d0335d4989834271bce0345dbe`.
Both runs used 2 GiB memory limits, no swap, and one CPU equivalent.
Generated coverage includes 300 seeded partitions at each of three nanosecond threshold positions.
Additional tests cover every identity field at every event boundary and invalidating events throughout the sequence.

Source review also found Python numeric equality could accept a Boolean as an integer observation.
The verifier now validates observation types before equality checks.
Regression tests cover this behavior and numerically equal floating-point seeds.

The final offline test and lint gate passed in `verifier.wo4mRP`.
Its invocation identifier was `5a914aef2f674e6bb23b1f2c34bcb31a`.
All 26 tests, Ruff checks, formatting checks, and ShellCheck passed.
The workspace whitespace check also passed.
The earlier quality attempts detected import ordering, string formatting, and explicit subprocess-check requirements.
Those corrections remain in the final source.

## Acceptance work after the first increment

The collector and workflow connection remain unfinished.
The offline gate must not serve as complete evidence for pgmcp criterion 2081 yet.
That criterion also requires workflow tests for duration, pinning, skips, cancellation, and shortened success.

The collector must supervise monotonic intervals and record actual immutable inputs and explicit seeds.
The workflow must preserve a full duration after preflight and let the final iteration finish normally.
The external final gate must reject missing, cancelled, skipped, or failed qualification jobs.
The artifact archive must contain the manifest and every referenced evidence file.

No workflow dispatch, Casper implementation change, Git commit, or Git push occurred in this increment.

## Collector and workflow connection

The collector and workflow connection now exist.
The manual `qualification-24h` profile retains the full 86,400-second requirement after preflight.
It disables checkpoint segmentation and automatic remainder-window retries.
The scheduled daily and weekend profiles retain their existing policy.

The collector binds the source revisions, executable identities, recorded configuration, seeds, runner boot, and original driver process.
It checks those inputs at iteration boundaries and preserves the first rejection.
Each successful iteration contributes only its supervised monotonic interval.
The final iteration must complete with valid test results and resource evidence.

The local collector result cannot establish qualification by itself.
The separate workflow gate verifies the artifact identity and the external job conclusion.
Missing artifacts, unsuccessful jobs, skipped tests, changed inputs, and insufficient duration cannot qualify.
Raw evidence and the final manifest have 30-day retention for this profile.

The legacy driver test previously invoked the production host-wide process cleanup through real `pkill`.
Its fixture now intercepts that command and checks the requested cleanup arguments.
The fixture also disables disk cleanup to protect unrelated local files.
Production process cleanup remains unchanged.

## Final local evidence

The expanded formal gate passed in `target/verification/soak-qualification/models.jZII2g`.
Its systemd invocation identifier was `24aa551fd15d4e218d5586df7194d8f8`.
Rocq compiled and kernel-checked nine propositions with closed global contexts.
The additional propositions require a successful external job before publication.

The safe TLA+ run generated 1,979 states and explored 1,121 distinct states.
It reached depth 14 with an empty queue.
The elapsed-only configuration produced the required counterexample with expected exit code 12.
The formal gate returned zero.

The final integration gate passed in `target/verification/soak-qualification/verifier.RDg9Di`.
Its systemd invocation identifier was `eafeab0472d54a9a9239232e9d1f7f09`.
All 44 tests passed, including collector, workflow, verifier, and actual-driver fixtures.
The generated duration tests include 900 seeded boundary cases.
The actual-driver fixture uses a simulated clock and fake workload tools, not a real 24-hour workload.

The existing driver regressions, Ruff checks, formatting checks, shell syntax checks, and workflow invariant checker also passed.
New shell gate files passed ShellCheck.
The modified driver added no diagnostics against its committed baseline.
Four pre-existing driver diagnostics remain outside this repair.
The gate verified its captured source hashes before it returned zero.

Both final gates used 2 GiB systemd memory limits, zero swap, and one CPU equivalent.
They stored local temporary files under `target/verification/soak-qualification/`, not the host's `/tmp`.

## Acceptance boundary and next work

Local implementation checks have passed.
Pgmcp records this task as `claimed_done`, not `verified`.
Progress record 11203 and evidence record 595 contain the local results.
Pgmcp criterion 2081 still requires evidence from its approved machine-verification route.
No approved local recipe exists for this project at the time of this record.
Local command results do not satisfy that separate provenance requirement automatically.

The actual campaign qualification run remains a later requirement after the assigned CI and cost-accounting gates pass.
This work did not launch nodes, dispatch a workflow, change Casper, commit files, or push files.
Additional state-import findings remain follow-up work unless evidence establishes an assigned-work blocker.

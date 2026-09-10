---
kind: tdd-plan
scope: soak-recurrence-prevention
produced_at: 2026-09-08T11:55:00Z
source_plan: docs/plans/soak-recurrence-prevention-2026-09-08.md
glossary: docs/Glossary.md
test_runner: shell
system_boundaries:
  - verifier-process
  - filesystem
  - github-actions
  - disk-probe
  - container-engine
  - workload-process
  - clock
behaviors:
  - id: B1
    statement: The formal gate accepts a carrier negative control only when TLC reports its expected invariant violation.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md
        test: scripts/ci/test-check-tla-invariants.sh
        red_exit: 1
        green_exit: 0
        tracer: true
  - id: B2
    statement: Pull requests run bounded finalization baseline checks and carrier negative controls without the exhaustive tier.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/github-workflows-slashing-tests-yml.md
        test: scripts/ci/test-soak-pr-formal-gate.sh
        red_exit: 1
        green_exit: 0
        hosted_confirmation: confirmed
        hosted_evidence: docs/cbc-evidence/soak-g0-b3-2026-09-08/hosted-observation.jsonc
  - id: B3
    statement: Every required claim identifies its implementation, verification command, evidence identity, and pending obligations.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/scripts-ci-test-soak-claim-inventory-sh.md
        test: scripts/ci/test-soak-claim-inventory.sh
        red_exit: 1
        green_exit: 0
        required_check_enforcement: pending
  - id: B4
    statement: Disk admission refuses a new iteration when cleanup leaves the valid sample below floor plus band.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/scripts-run-merge-recovery-soak-sh.md
        test: scripts/bench/test-soak-disk-admission.sh
        red_revision: 0f5d2b7414786cd27b8a686ca636485c35a5edce
        corrected_revision: ca85cfe3ecf73123b044b188b0c363fdbdff1e33
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B5
    statement: Disk admission refuses work when a post-start probe provides no sample.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-probe-2026-09-08/manifest.jsonc
        test: scripts/bench/test-soak-disk-probe.sh
        red_revision: 80914eedabd3413e174e9fca4fe8bd5c17021cb4
        red_exit: 1
        formal_red_exit: 12
        green_binding: source-sha256
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        model_review: pending
        claim_discharge: pending
  - id: B6
    statement: Disk admission rejects a field with a numeric prefix followed by text after a valid startup sample.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-sample-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-disk-sample.sh
        red_revision: 59430d59b45e4640187fd4b0414b03249c431e8f
        red_exit: 1
        formal_red_exit: 12
        green_binding: source-sha256
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        model_review: pending
        claim_discharge: pending
  - id: B7
    statement: An unavailable disk sample stops an active iteration.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-emergency-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-disk-active-probe.sh
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B8
    statement: The disk guardian records a breach before requesting a Docker stop.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-emergency-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-disk-record.sh
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B9
    statement: Observed guardian death stops an active iteration.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-emergency-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-guardian-death.sh
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        fixture_readiness_correction: retained
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B10
    statement: A stalled disk command cannot supply a valid sample after its timeout.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-emergency-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-disk-probe-timeout.sh
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B11
    statement: Stalled disk attribution has one aggregate command deadline across session roots.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-emergency-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-disk-diagnostic-deadline.sh
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B12
    statement: A retained guardian breach prevents new work after segment restart.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-emergency-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-disk-restart.sh
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B13
    statement: Stalled disk stop clients cannot prevent local failure publication or remain active past the fixture deadline.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-stop-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-disk-stop-deadline.sh
        red_revision: 3d2aa79048c9da0af09c7771b3f5c3eb3c369414
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B14
    statement: Guardian death during a boundary probe prevents iteration admission.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-boundary-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-guardian-admission.sh
        red_revision: e6fdd343b84726c29c9ac5fa992cac8b04e0d294
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B15
    statement: A retained guardian breach prevents the opening benchmark from starting without a state file.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-benchmark-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-benchmark-restart.sh
        red_revision: 37ec7f71dc077b03d75822bda88547a2fdd018fb
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B16
    statement: A disk sample below floor plus band prevents opening benchmark admission and records a protection failure.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-benchmark-band-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-benchmark-disk-admission.sh
        red_revision: 7f0f46923d9d972a95625fb3b8cdfc2ca2a8fc0c
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B17
    statement: The disk guardian observes a hard-floor breach while the opening benchmark remains active.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-benchmark-monitor-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-benchmark-disk-monitor.sh
        red_revision: 6e0b50f26be19f295c59eb233ac6eea297b9a72b
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B18
    statement: A guardian fault cancels a stalled benchmark client and preserves the protection failure before fixture release.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-benchmark-supervision-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-benchmark-cancellation.sh
        red_revision: 59b90568c435a53c04ee357c2f1f669775c5c219
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B19
    statement: Guardian death during a valid disk probe prevents opening and interleaved benchmark admission.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-benchmark-supervision-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-benchmark-guardian-admission.sh
        red_binding: B18-green-source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        reused_model: GuardianAdmission
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B20
    statement: A live guardian without progress causes active benchmark cancellation before fixture release.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-guardian-progress-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-guardian-progress.sh
        scenario: benchmark-cancel-stall
        red_revision: 8ab599e5cbc24d05ba89a6064abdc00acfc98100
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B21
    statement: A live guardian without progress causes active iteration cancellation before fixture release.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-guardian-progress-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-guardian-progress.sh
        scenario: guardian-stall
        red_binding: B20-green-source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        reused_model: GuardianProgress
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B22
    statement: Stale guardian progress prevents benchmark and iteration admission after a scheduling pause.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-guardian-progress-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-guardian-progress.sh
        scenarios: [guardian-progress-boundary, benchmark-progress-boundary]
        red_binding: B21-green-source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B23
    statement: Stalled disk hygiene causes client cancellation and failure publication before fixture release.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-hygiene-2026-09-09/manifest.jsonc
        test: scripts/bench/test-soak-disk-hygiene-deadline.sh
        red_revision: 5549561e1027438945dedabd1b7e08244883f62d
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        reused_model: DiskStopDeadline
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B24
    statement: Disk settings outside the signed arithmetic range cannot permit workload admission.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-settings-2026-09-10/manifest.jsonc
        test: scripts/bench/test-soak-disk-settings.sh
        red_revision: 9c99de84e492acab09d71df62642bbe67e4eb75a
        scenarios: [disk-floor-range, disk-band-range, disk-sum-range]
        characterization_cases: [disk-max-floor, disk-max-band]
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        green_driver_exit: 2
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B25
    statement: Disk hygiene preserves an unowned temporary session while its writer remains active.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-cleanup-session-2026-09-10/manifest.jsonc
        test: scripts/bench/test-soak-cleanup-ownership.sh
        red_revision: 8eba1e4a719fccac8f45a7968c8836013bc9f26e
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B26
    statement: A failed cleanup command prevents admission even when the later disk sample is sufficient.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-cleanup-outcome-2026-09-10/manifest.jsonc
        test: scripts/bench/test-soak-cleanup-outcomes.sh
        red_revision: d64ae3bbf757de6089300191c23cc1796245c60c
        scenarios: [list, remove, network, image, builder]
        characterization_cases: [partial, sufficient]
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
---

# Soak Gate Development Cycles

The user approved the [prevention plan](../plans/soak-recurrence-prevention-2026-09-08.md). This checklist covers G0, D1, and local D2 fault cases without changing consensus behavior or soak workloads.

## Boundaries

The tests execute the real formal gate script. A verifier-process fixture supplies tool results to test the gate's result classification.

The fixture is not proof authority. Separate runs use the pinned TLA+ model checker (TLC) against the actual carrier configurations.

## Behavior checklist

- [x] B1: The gate rejects clean negative controls, incorrect invariant violations, tool failures, timeouts, and missing configurations.
- [x] B2: Local tests and hosted bounded verification pass. Required-check enforcement remains pending.
- [x] B3: The claim inventory binds source digests and distinguishes baseline evidence from pending obligations.
- [x] B4: Historical production and formal counterexamples match. The existing admission correction passes both local checks.
- [x] B5: Missing samples before and after hygiene prevent admission and produce a recorded failure.
- [x] B6: A field with a numeric prefix followed by text prevents admission and produces a recorded failure.
- [x] B7: An unavailable active-iteration sample triggers interruption and a breach record.
- [x] B8: The disk breach record precedes the Docker stop command and does not claim confirmed termination.
- [x] B9: The iteration watcher detects guardian death and records failure.
- [x] B10: A probe timeout rejects valid partial output from a stalled command.
- [x] B11: Stalled attribution stops within an aggregate fixture deadline across 32 session roots.
- [x] B12: A retained breach prevents restart admission and preserves a failure outcome.
- [x] B13: Stalled disk stop clients exit before the fixture deadline, and the driver publishes a local failure.
- [x] B14: Guardian death during a valid boundary probe prevents iteration admission and produces one recorded failure.
- [x] B15: A retained breach prevents the opening benchmark without a state file and preserves the failure.
- [x] B16: A 7000 MiB sample prevents opening benchmark admission below the 8192 MiB threshold and records one protection failure.

- [x] B17: The guardian records a hard-floor breach and requests a stop before the opening benchmark returns.

## Cycle evidence

B1 completed one RED/GREEN cycle. The [evidence record](../cbc-evidence/scripts-ci-check-tla-invariants-sh.md) retains the failure, correction, tests, and separate bounded TLC results.

The correction changes result classification only. It does not change the model, production consensus behavior, or soak workload.

B2 completed its local RED/GREEN cycle. The [B2 evidence](../cbc-evidence/github-workflows-slashing-tests-yml.md) retains the workflow failure and the real bounded gate results.

B3 completed its inventory RED/GREEN cycle. The [B3 evidence](../cbc-evidence/scripts-ci-test-soak-claim-inventory-sh.md) records the missing inventory and checks that incomplete or stale records fail.

The hosted formal job passed on PR head `43af06da` with synthetic checkout `bad72c4c`. The retained observation includes the run, attempt, job, source digests, and artifact digest.

Required-check enforcement and complete acceptance identity remain open. `CLAIM-SOAK-GATE-001` and `CLAIM-FINALITY-002` remain pending.

B4 completes the local D1 cycle. Its container fixture replaces external disk, Docker, and workload commands, not the driver's admission logic.

The first fixture attempt failed before driver execution because of file permissions. That attempt is not RED evidence. The final fixture preserves the complete required helper set.

The model checks one admission decision with four possible free-space values. It permits zero reclamation and checks both sufficient-reclamation admission and recorded refusal.

Hosted D1 execution and maintainer review remain pending. G0 is not complete. Inventory validity does not discharge a claim or authorize a soak.

B5 completes one local D2 cycle. Both missing-sample production traces failed before the correction. The matching formal control violated `AdmissionRequiresSample` with exit 12.

The corrected driver refused both admissions and recorded failure. The corrected model completed with 22 distinct states. The existing D1 and driver regressions also passed.

The first formal attempt stopped on a sample encoding error with exit 75. That attempt is not behavioral RED. The evidence retains it separately.

B5 does not complete D2. Emergency timing, failed guardians, writer termination, durability, and the other fault cases remain pending.

B6 reproduces admission after `df` returns `16384junk` with exit zero. The old parser converts that malformed field to `16384`. The matching formal control violates `AdmissionRequiresValidSample` with exit 12.

The one-line correction validates the original field before the existing numeric conversion. Production GREEN records zero iterations and one failure. Formal GREEN completes with 17 distinct states.

D1, B5, and the existing driver suite also pass. The bounded gate passes five positive configurations and five exact negative controls. The classifier covers 35 cases.

B6 completes only this local cycle. Other malformed values, active-iteration response, guardian failure, aggregate deadlines, writer termination, and restart preservation remained pending at that point.

B7 through B12 complete six additional local cycles. Their [evidence](../cbc-evidence/soak-d2-emergency-2026-09-09/README.md) retains separate production and formal RED/GREEN pairs.

A driver-suite fixture race required a readiness correction during B9. Two verification commands also reached tool limits. None of these results supplies behavioral RED evidence.

The corrected full driver suite passes. Thirty corrected first-scenario repetitions also pass. The combined formal gate passes 11 positive configurations and 11 exact controls.

B13 adds a separate matched cycle for stalled disk stop clients. Its [evidence](../cbc-evidence/soak-d2-stop-2026-09-09/README.md) retains the first unsuccessful correction.

The first correction returned but left two clients alive. The corrected timeout wrapper keeps its shell alive for process-group cancellation.

The combined gate now passes 12 positive configurations and 12 exact controls. The classifier covers 84 cases. Eight emergency fixture scenarios pass.

Concurrent verification initially shared fixed temporary log paths. Those runs cannot supply isolated gate evidence. Subsequent serial verification passes.

B14 adds a matched cycle for guardian death during the boundary probe. The [evidence](../cbc-evidence/soak-d2-boundary-2026-09-09/README.md) retains the rejected admission and four-state corrected model.

The combined gate now passes 13 positive configurations and 13 exact controls. The classifier covers 91 cases. Nine emergency fixture scenarios pass.

B14 does not cover benchmark admission, live but stalled guardians, or a crash after the final liveness check.

B15 prevents the opening benchmark from bypassing a retained breach when the state file is absent. Its [evidence](../cbc-evidence/soak-d2-benchmark-2026-09-09/README.md) retains matched production and formal results.

The bounded gate passes 14 positive configurations and 14 exact controls. The classifier covers 98 cases. All ten emergency scenarios pass.

B16 adds the [benchmark disk-admission cycle](../cbc-evidence/soak-d2-benchmark-band-2026-09-09/README.md). The production fixture and formal counterexample use 7000 MiB free and an 8192 MiB threshold.

Production GREEN refuses both benchmark and iteration admission and records one protection failure. Formal GREEN completes with 12 distinct states.

The bounded gate passes 15 positive configurations and 15 exact controls. The classifier covers 105 cases. All eleven emergency scenarios pass.

B17 moves opening benchmark execution after guardian startup. Its [evidence](../cbc-evidence/soak-d2-benchmark-monitor-2026-09-09/README.md) retains the unobserved breach and five-state corrected model.

Sixteen positive configurations, sixteen exact controls, 112 classifier cases, six routing scenarios, and twelve emergency scenarios pass.

The first fixture attempt lacked executable snapshot modes and returned a degraded summary. That setup failure does not supply behavioral RED.

B17 covers this local monitoring path only. Guardian failure, benchmark cancellation, writer termination, durable publication, and the composed deadline remain open.

Four additional [opening-admission cases](../cbc-evidence/soak-d2-benchmark-cases-2026-09-09/README.md) pass without a production correction. These results extend coverage, not the repair-cycle count.

The expanded fixture preserves the B17 counterexample. The combined emergency suite now passes sixteen scenarios. The formal inventory remains at sixteen positive configurations and controls.

B18 and B19 add [benchmark supervision evidence](../cbc-evidence/soak-d2-benchmark-supervision-2026-09-09/README.md). B18 cancels stalled clients after guardian death or a disk breach.

B19 refuses opening and interleaved benchmark admission when the guardian dies during the disk probe. It reuses the unchanged guardian-admission model and control.

Seventeen positive configurations, eighteen exact controls, 126 classifier cases, six routing scenarios, and twenty emergency scenarios pass.

The combined verification command reached its tool limit during release regressions. The remaining checks passed separately, and the partial run remains recorded.

B20–B22 add [guardian progress evidence](../cbc-evidence/soak-d2-guardian-progress-2026-09-09/README.md). Suspended guardians now cause active cancellation and prevent tested stale admission.

Nineteen positive configurations, twenty exact controls, 140 classifier cases, six routing scenarios, and twenty-four emergency scenarios pass.

B23 adds [disk hygiene deadline evidence](../cbc-evidence/soak-d2-hygiene-2026-09-09/README.md). The driver cancels the stalled cleanup client and publishes refusal before fixture release.

Twenty positive configurations, twenty-one exact controls, 147 classifier cases, six routing scenarios, and twenty-five emergency scenarios pass.

B24 adds [disk setting validation evidence](../cbc-evidence/soak-d2-settings-2026-09-10/README.md). Three configurations that previously admitted work now fail before workload startup.

Two valid maximum-value cases pass without changing baseline behavior. These results add coverage, not repair cycles.

The first positive model reached its verification limit. The retained replacement uses explicit decimal column steps and passes with 88 distinct states.

Twenty-one positive configurations, twenty-two exact controls, 154 classifier cases, six routing scenarios, and thirty emergency scenarios pass.

B25 adds [temporary session preservation evidence](../cbc-evidence/soak-d2-cleanup-session-2026-09-10/README.md). An old directory remains intact while its fixture writer holds the data file open.

The driver removes its age-only sweep and retains admission refusal when space remains insufficient. An existing regression now requires preservation instead of deletion.

Twenty-two positive configurations, twenty-three exact controls, 161 classifier cases, six routing scenarios, and thirty-one emergency scenarios pass.

B26 adds [cleanup failure evidence](../cbc-evidence/soak-d2-cleanup-outcome-2026-09-10/README.md). Five command errors now prevent admission despite a later sufficient disk sample.

Two successful-cleanup cases preserve baseline behavior with partial and sufficient reclamation. The corrected model has 42 distinct states.

Twenty-three positive configurations, twenty-four exact controls, 168 classifier cases, six routing scenarios, and thirty-eight emergency scenarios pass.

## Remaining D2 work

- [x] Verify opening admission at equality, sufficient space, unavailable samples, and disabled disk protection.
- [x] Cancel stalled benchmark clients after guardian death or a recorded disk breach.
- [x] Refuse opening and interleaved benchmark admission after guardian death during the probe.
- [x] Detect suspended guardians during active benchmarks and iterations.
- [x] Refuse opening benchmark and iteration admission after tested stale-progress pauses.
- [ ] Verify interleaved sample cases, remaining progress-record faults, later scheduling races, and other admission and cancellation faults.

- [x] Bound the tested hygiene command group and refuse work when the group fails.
- [ ] Verify other stop paths, detached and uninterruptible commands, and daemon-side cleanup operations.
- [ ] Verify soft-floor sampling, cleanup outcomes, and guardian events at every admission boundary.
- [ ] Verify active-session and image preservation through the cleanup ownership contract.
- [ ] Verify confirmed termination and durable evidence publication.
- [ ] Establish one composed emergency deadline, including iteration shutdown and evidence handling.
- [x] Reject the tested oversized disk settings and overflowing admission sum.
- [x] Preserve the tested valid maximum settings.
- [ ] Complete other numeric-range, status, location, and crash-recovery cases outside the recorded fixture domains.
- [ ] Obtain D3 writer-growth and reserve evidence, hosted results, and maintainer review.

The completed local cycles do not complete D2 or discharge a claim. O1 and another soak remain on hold.

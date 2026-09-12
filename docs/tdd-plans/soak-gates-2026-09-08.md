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
  - id: B27
    statement: Disk hygiene preserves the tested unrelated Docker resources and reserved fixture image.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-real-system-2026-09-10/manifest.jsonc
        test: scripts/bench/test-soak-real-docker-ownership.sh
        red_revision: 1e2dc07f4c19c70fb56d9b861be2a0394fcfe721
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B28
    statement: After active-iteration SIGTERM, the post-exit check confirms termination of the selected Docker writer.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-real-system-2026-09-10/manifest.jsonc
        test: scripts/bench/test-soak-real-docker-shutdown.sh
        baseline: B27 correction
        binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B29
    statement: Two restarts refuse new work and retain one failure after the tested process-group crash.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-real-system-2026-09-10/manifest.jsonc
        test: scripts/bench/test-soak-real-crash-recovery.sh
        baseline: B28 correction
        binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        initial_formal_green: incomplete-successor
        hosted_confirmation: pending
        claim_discharge: pending
  - id: B30
    statement: Driver exit stops the workload Docker writer and preserves an unrelated writer with a matching name prefix.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-owned-stop-2026-09-10/manifest.jsonc
        test: scripts/bench/test-soak-real-stop-ownership.sh
        red_revision: 52c91cfe4f677b9b9a8b6e718a03e0c29d083d85
        red_exit: 1
        formal_red_exit: 12
        green_binding: source-sha256
        green_exit: 0
        formal_green_exit: 0
        launch_cases: [run, compose]
        hosted_confirmation: pending
        model_review: pending
        claim_discharge: pending
  - id: B31
    statement: Driver exit retains a rejected Docker stop and reports unconfirmed writer termination.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-owned-stop-2026-09-10/manifest.jsonc
        test: scripts/bench/test-soak-real-stop-failure.sh
        red_binding: b30-corrected-source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_binding: source-sha256
        green_exit: 0
        formal_green_exit: 0
        hosted_confirmation: pending
        model_review: pending
        claim_discharge: pending
  - id: B32
    statement: Driver exit stops its detached host writer and preserves unrelated node and client writers.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-host-stop-2026-09-11/manifest.jsonc
        test: scripts/bench/test-soak-host-stop-ownership.sh
        scenario: exit
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        claim_discharge: pending
  - id: B33
    statement: Memory protection stops the workload host writer and preserves unrelated host writers.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-host-stop-2026-09-11/manifest.jsonc
        test: scripts/bench/test-soak-host-stop-ownership.sh
        scenario: memory
        red_binding: b32-corrected-source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        claim_discharge: pending
  - id: B34
    statement: Memory protection changes native-process termination preferences only for workload-owned processes.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-oom-ownership-2026-09-11/manifest.jsonc
        test: scripts/bench/test-soak-host-stop-ownership.sh
        scenario: oom
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        claim_discharge: pending
  - id: B35
    statement: Restart refuses new work after an unconfirmed benchmark writer stop and retains the failure.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-container-preference-2026-09-11/manifest.jsonc
        test: scripts/bench/test-soak-real-benchmark-stop-recovery.sh
        kind: unchanged-production-characterization
        baseline_exit: 0
        current_exit: 0
        production_repair: false
        claim_discharge: pending
  - id: B36
    statement: Memory protection changes container termination preferences only for workload-owned containers.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-container-preference-2026-09-11/manifest.jsonc
        test: scripts/bench/test-soak-real-stop-ownership.sh
        scenarios: [run, compose]
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        claim_discharge: pending
  - id: B37
    statement: Restart refuses work and retains one interrupted benchmark failure after a driver crash before outcome publication.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-benchmark-crash-2026-09-11/manifest.jsonc
        test: scripts/bench/test-soak-real-benchmark-crash-recovery.sh
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        claim_discharge: pending
  - id: B38
    statement: Production crash response stops the owned Docker writer and preserves the unrelated writer without fixture intervention.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-crash-stop-2026-09-11/manifest.jsonc
        test: scripts/bench/test-soak-real-crash-stop.sh
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        composed_regression_resolved_by: B39
        construction: not-applicable
        claim_discharge: pending
  - id: B39
    statement: The crash monitor does not repeat a stop after the driver completes exit handling.
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-crash-stop-2026-09-11/manifest.jsonc
        test: scripts/bench/test-soak-disk-stop-deadline.sh
        red_origin: composed-b38-regression
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        construction: not-applicable
        claim_discharge: pending
  - id: B40
    statement: Monitor death during an iteration stops the owned host writer, preserves an unrelated writer, and retains one failure across restarts.
    priority: must
    deep_module: false
    done: true
    construction: not-applicable
    claim_discharge: pending
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-monitor-death-2026-09-11/manifest.jsonc
        test: scripts/bench/test-soak-crash-monitor-death.sh
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
  - id: B41
    statement: Monitor death during a benchmark stops the owned writer, preserves an unrelated writer, and retains one failure across restarts.
    priority: must
    deep_module: false
    done: true
    construction: not-applicable
    claim_discharge: pending
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-shutdown-2026-09-11/manifest.jsonc
        test: scripts/bench/test-soak-benchmark-monitor-death.sh
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
  - id: B42
    statement: Monitor death during an admission probe prevents benchmark and iteration admission and retains one failure across restarts.
    priority: must
    deep_module: false
    done: true
    construction: not-applicable
    claim_discharge: pending
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-shutdown-2026-09-11/manifest.jsonc
        test: scripts/bench/test-soak-monitor-admission.sh
        modes: [benchmark, iteration]
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
  - id: B43
    statement: Interrupted iteration shutdown stops the owned descriptor holder before waiting for output EOF.
    priority: must
    deep_module: false
    done: true
    construction: not-applicable
    claim_discharge: pending
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-shutdown-2026-09-11/manifest.jsonc
        test: scripts/bench/test-soak-monitor-inherited-pipe.sh
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
  - id: B44
    statement: Combined driver and crash-monitor loss must stop the owned writer without stopping an unrelated writer.
    priority: must
    deep_module: true
    done: false
    construction: not-applicable
    claim_discharge: pending
    blocked_on: Private Docker containment, creation fencing, and verified production admission remain incomplete.
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-shutdown-2026-09-11/manifest.jsonc
        test: scripts/bench/test-soak-controller-loss.sh
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: null
        formal_green_exit: null
      - evidence: docs/cbc-evidence/soak-d2-native-containment-2026-09-11/manifest.jsonc
        scope: native-controller-loss-only
        test: scripts/bench/test-soak-native-containment.py
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        production_integration: false
        completes_behavior: false
  - id: B45
    statement: When containment is required, admission refuses work unless a trusted run-domain record matches the driver's own placement.
    priority: must
    deep_module: false
    done: true
    construction: not-applicable
    claim_discharge: pending
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-run-domain-2026-09-12/manifest.jsonc
        test: scripts/bench/test-soak-run-domain-admission.sh
        modes: [benchmark, iteration]
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        production_integration: false
  - id: B46
    statement: The run-domain record check trusts only a record it opened through a root-owned, unwritable directory chain and rejects a record beyond its size bound.
    priority: must
    deep_module: false
    done: true
    construction: not-applicable
    claim_discharge: pending
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-record-identity-2026-09-12/manifest.jsonc
        test: scripts/bench/test-soak-record-identity.sh
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        production_integration: false
  - id: B47
    statement: The native launcher verifies manager placement and gate identity before it releases the driver, so a stalled service-status query prevents native admission.
    priority: must
    deep_module: true
    done: true
    construction: not-applicable
    claim_discharge: pending
    implemented_by: pi-session-native-admission
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-native-admission-2026-09-12/manifest.jsonc
        scope: native-launch-admission
        test: scripts/bench/test-soak-native-admission.py
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        native_regression: scripts/bench/test-soak-native-containment.py
        production_integration: false
  - id: B48
    statement: The driver publishes its minimal breach record before any disk attribution starts, and a stalled attribution cannot delay that record or failure publication.
    priority: must
    deep_module: false
    done: true
    construction: not-applicable
    claim_discharge: pending
    cycle_log:
      - evidence: docs/cbc-evidence/soak-d2-breach-record-2026-09-12/manifest.jsonc
        test: scripts/bench/test-soak-breach-record-order.sh
        red_binding: source-sha256
        red_exit: 1
        formal_red_exit: 12
        green_exit: 0
        formal_green_exit: 0
        production_integration: false
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

B41–B43 add [shutdown evidence](../cbc-evidence/soak-d2-shutdown-2026-09-11/README.md) for three selected defects.
B41 detects monitor death during an active benchmark.
B42 refuses opening benchmark and iteration admission after confirmed monitor death during a valid probe.
B43 stops the owned descriptor holder before the interrupted iteration waits for output end of file (EOF).
Each unchanged production fixture and matching corrected model passes.

B45 adds [run-domain admission evidence](../cbc-evidence/soak-d2-run-domain-2026-09-12/README.md).
When containment is required, the driver refuses benchmark and iteration admission unless a trusted [run-domain record](../../formal/tlaplus/soak_disk/RunDomainAdmission.md) matches its own cgroup and uid.
An absent or mismatched record retains one failure across two refused restarts, and a matching record admits work.
The native launcher does not write the record yet, and the normal workflow does not require containment.

B46 adds [record identity evidence](../cbc-evidence/soak-d2-record-identity-2026-09-12/README.md).
The [record check](../../formal/tlaplus/soak_disk/RunDomainRecordIdentity.md) now opens the record through descriptors from the filesystem root.
It rejects an untrusted ancestor, a symbolic link component, or a record beyond its size bound.

B44 is an open [controller-loss counterexample](../../formal/tlaplus/soak_disk/ControllerLoss.md).
Both controllers exit, but the owned native writer continues without fixture intervention.
The current model violates `ControllerLossStopsOwnedWriter` with exit 12.
The original direct-launch counterexample remains RED.
A separate [native-only subcycle](../../formal/tlaplus/soak_disk/NativeControllerLoss.md) passes with an unchanged fixture and matching three-state model.
The normal workflow does not use that prototype.
Private Docker containment, creation fencing, and verified pre-admission placement remain open.
B44, D2, and claim discharge remain pending.

B40 adds [monitor-death evidence](../cbc-evidence/soak-d2-monitor-death-2026-09-11/README.md).
The baseline driver continues its owned native writer after confirmed monitor death during an iteration.
The correction stops that writer, preserves an unrelated writer, and retains one failure across two refused restarts.
The unchanged fixture and matching five-state model pass.

The combined checks pass 42 emergency cases, 11 real-system cases, 35 positive configurations, 37 exact controls, 259 classifier cases, and six routing scenarios.
Supporting regressions pass, and the disposable runner was observed terminated after evidence retrieval.
Monitor death during benchmarks, admission boundaries, inherited output pipes, simultaneous failures, storage faults, deadlines, and reserve bounds remain open.
Reruns remain digest-only under the new packaging rule.
D2 and claim discharge remain pending.

B38 and B39 add [crash monitor evidence](../cbc-evidence/soak-d2-crash-stop-2026-09-11/README.md).
B38 stops the owned Docker writer after a driver process-group crash and preserves an unrelated writer without fixture intervention.
B39 removes a duplicate monitor stop after completed exit handling.
The failed intermediate regression retained its failure summary and identified the extra stop client in a process snapshot.

Both cycles retain matching production and formal RED/GREEN results.
The final source passes 11 real-system cases, 41 emergency cases, 34 positive configurations, 36 exact controls, 252 classifier cases, and supporting checks.
Both diagnostic virtual machines were observed terminated after archive retrieval and validation.
Storage faults, other ownership and crash windows, aggregate deadlines, reserve bounds, hosted checks, and maintainer review remain open.
D2 and claim discharge remain pending.

B37 adds [benchmark crash evidence](../cbc-evidence/soak-d2-benchmark-crash-2026-09-11/README.md).
The baseline admits new work after a crash before any benchmark outcome or stop record.
The correction persists the interruption before launch and retains one failure across two refused restarts.
The four-state model preserves the exact `BenchmarkCrashRequiresRefusal` control.

Ten real-system GREEN cases, 32 positive configurations, 34 controls, 238 classifier cases, six routing scenarios, and 41 emergency cases pass.
Supporting regressions also pass, including failed benchmark-stop recovery.
The archive contains 376 verified regular files, and the diagnostic virtual machine was observed terminated.
The original Docker writer remains active until fixture cleanup, so this cycle does not prove production crash-time termination.
Storage faults, other crash windows, durable publication, aggregate deadlines, reserve bounds, hosted checks, and maintainer review remain pending.

B35 and B36 add [benchmark recovery and Docker preference evidence](../cbc-evidence/soak-d2-container-preference-2026-09-11/README.md).
B35 passes unchanged-production characterization and retains one failed benchmark stop across two refused restarts.
It does not require a production correction or a fabricated formal RED.
B36 replaces periodic Docker preference writes with creation configuration for the tested cooperative workload paths.
Matched `run` and Compose cases preserve the unrelated preference while they still prefer and stop the workload writer.

All 12 real-system cases pass their expected checks, including current-driver B27–B31 revalidation.
The composed gate passes 31 positive configurations, 33 exact controls, 231 classifier cases, and six routing scenarios.
All 41 emergency cases and supporting regressions also pass.
The archive contains 421 verified regular files, and both new diagnostic VMs were observed terminated.
Storage faults, remaining crash windows, aggregate deadlines, D3 reserve bounds, hosted checks, and maintainer review remain pending.
These selected local results do not complete D2.

B34 adds [native memory preference evidence](../cbc-evidence/soak-d2-oom-ownership-2026-09-11/README.md).
The baseline changes an unrelated node preference from zero to 1000.
The correction preserves that preference and still changes the workload preference.
The two-state model retains an exact ownership control.

The composed gate passes 30 positive configurations and 32 exact controls.
All 224 classifier cases, six routing scenarios, 41 emergency cases, and supporting regressions pass.
B35 and B36 were unchecked at the B34 evidence stage.
The later results appear above, and the broader D2 requirements remain pending.

B32 and B33 add [host stop evidence](../cbc-evidence/soak-d2-host-stop-2026-09-11/README.md) for driver exit and memory protection.
Both cases stop a detached workload writer and preserve unrelated writers in a private process namespace.
The corrected model has two states and separate exact controls for the two unsafe selections.

The composed gate passes 29 positive configurations and 31 exact controls.
The classifier covers 217 cases, routing covers six scenarios, and all 40 emergency cases pass.
The updated benchmark fixture accepts observed client termination instead of requiring a callback after termination.

B34 and B35 record the next identified local gaps.
Durability, complete response bounds, D3 reserve evidence, real-Docker revalidation, hosted checks, and maintainer review remain pending.
D2 is not complete.

B30 and B31 add [Docker stop evidence](../cbc-evidence/soak-d2-owned-stop-2026-09-10/README.md) from a guarded disposable VM.
B30 passes with Docker `run` and Compose while preserving the unrelated writer.
B31 retains one failed interruption after a rejected Docker stop and reports unconfirmed writer termination.

Their two-state models retain exact negative controls.
The composed gate passes 28 positive configurations and 29 exact controls.
The classifier covers 203 cases, routing covers six scenarios, and all 38 emergency shim scenarios pass.

Host-process ownership, memory-pressure paths, other launch forms, late creation, storage faults, and complete shutdown remain open.
These local cycles do not complete D2 or discharge any claim.

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

B27 through B29 add [real-system evidence](../cbc-evidence/soak-d2-real-system-2026-09-10/README.md) from a disposable virtual machine. Docker commands reach its real daemon.

B27 preserves the tested resources through read-only hygiene. B28 confirms the selected writer stops after `SIGTERM`. B29 retains one interruption failure across two restarts.

The final gate passes 26 positive configurations, 27 exact controls, 189 classifier cases, and six routing scenarios. All 38 emergency scenarios and three real-system fixtures pass.

The first B29 positive model had an incomplete successor. Its corrected RED/GREEN pair passes. Process-crash recovery does not establish power-loss durability.

- [x] Verify opening admission at equality, sufficient space, unavailable samples, and disabled disk protection.
- [x] Cancel stalled benchmark clients after guardian death or a recorded disk breach.
- [x] Refuse opening and interleaved benchmark admission after guardian death during the probe.
- [x] Detect suspended guardians during active benchmarks and iterations.
- [x] Refuse opening benchmark and iteration admission after tested stale-progress pauses.
- [ ] Verify interleaved sample cases, remaining progress-record faults, later scheduling races, and other admission and cancellation faults.

- [x] Bound the tested hygiene command group and refuse work when the group fails.
- [ ] Verify other stop paths, detached and uninterruptible commands, and daemon-side cleanup operations.
- [ ] Verify soft-floor sampling, cleanup outcomes, and guardian events at every admission boundary.
- [x] Preserve the tested Docker resources through read-only hygiene.
- [x] Confirm selected writer termination after active-iteration `SIGTERM`.
- [x] Refuse two restarts after the tested process crash with one retained interruption failure.
- [ ] Verify ownership-safe reclamation, ownership-safe stop selection, and the complete node-image lifecycle.
- [x] Verify selected benchmark monitor death, opening admission checks, and interrupted output-drain ordering.
- [ ] Correct combined controller loss with a reviewed containment design and matched RED/GREEN evidence.
- [ ] Verify other shutdown paths and durable evidence publication.
- [ ] Establish one composed emergency deadline, including iteration shutdown and evidence handling.
- [x] Reject the tested oversized disk settings and overflowing admission sum.
- [x] Preserve the tested valid maximum settings.
- [ ] Complete other numeric-range, status, location, and crash-recovery cases outside the recorded fixture domains.
- [ ] Obtain D3 writer-growth and reserve evidence, hosted results, and maintainer review.

The completed local cycles do not complete D2 or discharge a claim. O1 and another soak remain on hold.

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
        hosted_evidence: docs/cbc-evidence/soak-g0-b3-2026-09-08/hosted-observation.json
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
      - evidence: docs/cbc-evidence/soak-d2-probe-2026-09-08/manifest.json
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

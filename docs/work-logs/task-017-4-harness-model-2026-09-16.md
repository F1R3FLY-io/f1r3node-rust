---
task: TASK-017-4
related_task: TASK-017-2
branch: formal/soak-casper-consensus
claimed_by: pi-casper-harness
claimed_at: 2026-09-16T22:18:38Z
handoff_status: paused
next_steps:
  - Complete the driver and profile interface contracts.
  - Resolve prerequisite integration before changing shared CI or the soak driver.
  - Bind modeled actions to executable driver fixtures.
---

# Bounded Harness Model Implementation

## Scope

This increment implements an independent refutation slice under TASK-017-4. It does not complete TASK-017-2, TASK-017-3, or TASK-017-4.

The working-tree base is `cc7e84b482887f0647277ccbacd6f65ae3cf749d`. The evidence package records actual input digests for uncommitted files.

The implementation includes one TLA+ lifecycle model, one clean configuration, ten negative controls, a local runner, and twelve runner unit tests.

No Rust, live driver, node configuration, or shared workflow changed. Construction remains not applicable.

## Prerequisite check

PR #431 remains open at `0e176e486a028d10704b96add6e5eb50525682cb`.

PR #432 remains open at `e0380392bcc66d9774e403edb8a08415798c1e0e`.

Their integration still requires separate authorization. This independent model does not assume that their driver or CI changes are present.

The 15-minute versus two-minute shared-tier question remains unresolved. The local runner uses a declared per-configuration cap, not a new shared CI budget.

## Implemented behavior

The model covers admission, workload launch, observation, checkpoint resume, stop, evidence capture, report, cleanup, policy isolation, and negative-control classification.

Specification history detects erased failures and overwritten iterations. Ten separate defect knobs expose the corresponding invariant violations.

The runner checks exact invariant registration and defect-knob settings. It rejects missing configurations, unexpected success, wrong violations, missing traces, timeouts, and input changes.

Cancellation cannot pass. Existing output directories cannot be overwritten. Reports retain configuration constants, input digests, log digests, resource caps, and observed tool versions.

## Results

The [retained package](../casper/cbc-evidence/runs/casper-harness-controls-20260916-01/report.json) contains the report, TLC traces, unit-test output, and an initial tool failure.

| Check | Result |
| --- | --- |
| Runner unit tests | 12 passed |
| Clean configuration | Exit 0, 43,424 distinct states, 66,208 generated states |
| Ten negative controls | Each returned exit 12 with its exact named invariant and trace |
| TLC | Version 2.19, seed 1, one worker |
| Java heap cap | 512 MB |
| Per-configuration timeout | 120 seconds |
| Construction | Not applicable |
| Live driver bindings | Pending |
| Profile models and fixtures | Pending |
| Shared CI integration | Pending |
| Soak campaign | Not run |

The initial default Java launcher failed with an architecture error. The successful run selected a native Java binary explicitly.

The retained log records that failure. Log paths are redacted, and the package retains both original and retained-log digests.

## Reproduction

```bash
python3 -m unittest discover -s scripts/ci/tests -p 'test_casper_soak_model_runner.py' -v
python3 scripts/ci/check_casper_soak_models.py --java "$JAVA" --output-dir target/casper-soak-formal/new-run
```

Set `JAVA` to a working native Java executable. Set `TLA_TOOLS_JAR` when the jar is not at the runner's default location.

Use a new output directory for each run. Raw local outputs stay under `target/casper-soak-formal/`.

## Limits

The model has two candidate identities, two segments, four total iterations, and one active child. Evidence completeness is an abstract Boolean.

The model checks bounded safety, not eventual termination, actual filesystem durability, resource monitoring accuracy, or node correctness.

Runner unit tests validate the control runner. They are not the real-driver fixtures required by H01 through H10.

The full claim and every artifact ledger status remain pending. A passing refutation slice does not discharge the harness or profiles.

## Next work

Complete exact driver-interface contracts and prerequisite review. Then implement driver fault fixtures and integrate the runner into the reviewed workflow.

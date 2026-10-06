# TASK-017-7 Recovery and Custody Profile

## Current review

The [combined review](./task-017-5-7-binding-review.md) records fresh native and isolated Linux checks, model controls, and shared regressions.

TASK-017-7 is complete for its bounded pre-merge scope. CLAIM-004 is discharged, and the workflow tag is ratified.

The [acceptance record](./task-017-5-7-acceptance.md) identifies the source-bound evidence and strict completion checks. External commit `d60f54544` separately restored CLAIM-001 discharge.

D-07 interpretation remains unresolved. Controlled fixtures do not establish node support. Live, experimental-policy, and post-merge requests remain blocked.

The sections below retain historical results, including the unavailable Linux execution and failed shared source audit.

## Scope

The controlled-transcript implementation is complete. TASK-017-7 remains in progress because binding acceptance, workflow-tag ratification, and shared source verification remain unresolved.

The implementation base is `a94c5655ee2284e228667a02e357a6ef6cd10ebf`. This task preserves lifecycle, authority/finality, and publication sources and acceptance records.

The [claim](../claims/casper-soak-recovery.md) remains pending. The [implementation contract](../../formal/tlaplus/casper_soak/profiles/recovery/README.md) defines its inputs, measurements, and commands.

## Implementation

- [x] Bind source identities, configuration, capabilities, and pinned expectations.
- [x] Keep stale recovery, convergence, frontier follow, readiness, and backstop lanes separate.
- [x] Preserve source occurrence identities, multiplicity, custody, reasons, tombstones, leases, and terminal outcomes.
- [x] Require observed pause and delivery receipts. Keep unknown durations and telemetry distinct from zero.
- [x] Exercise complete, blocked, deterministic, and negative fixtures through the profile binary.
- [x] Run the bounded model and named negative controls.
- [x] Retain evidence and report acceptance gates.

The separate `casper-recovery` binary launches no node. Its generator, collector, and classifier preserve immutable input and raw-observation references.

The manifest and qualification records bind the occurrence schema. Candidate-scoped source identifiers remain independent of deploy signatures and sample ordering.

The collector deduplicates transport events before lane quarantine. Conflicting copies cannot pass merely because one copy changes a payload correlation field.

Pause and delivery coverage require correlated state receipts. Ordered receipts and recovery snapshots must respect their declared observer clock and sequence.

The classifier compares pinned expectations for custody, reason inputs, joined reasons, causal references, tombstones, leases, body availability, objective height, lifespan, retry authorization, and terminal outcomes.

Single and split frontier fixtures retain separate one-parent and collective coverage observations. No fixture constructs valid node blocks or grants collective-coverage authority.

Missing terminal and custody observations produce unknown aggregates, not zero. Missing occurrence samples cannot erase independent telemetry failures.

## Verification

The retained package is [casper-recovery-20260918-01](../casper/cbc-evidence/runs/casper-recovery-20260918-01/report.json).

| Check | Result |
| --- | --- |
| Native recovery tests | Seven passed. |
| Controlled fixtures | 52 cases and 55 binary invocations passed their expected-result checks. |
| Clean TLC control | Exit 0, 841 generated states, and 441 distinct states. |
| Lane negative control | Exit 12 with `LaneLabelsPreserved` and a counterexample. |
| Occurrence negative control | Exit 12 with `OccurrenceCountsPreserved` and a counterexample. |
| Pause negative control | Exit 12 with `PauseCoverageAcknowledged` and a counterexample. |
| Existing profile and shared host regressions | 24 passed and one failed. The complete command exited 101. |
| Linux-musl cross-build | Passed for the final source. |
| Linux execution | Not run because the Docker socket is unavailable. |
| Active language-server checks | Five files checked with zero diagnostics. |
| Strict CLAIM-001 and CLAIM-004 audits | Both exited 2 because the shared discharged source differs. |

Native tests use optimization level 0. The Linux cross-build uses level 1. The runner records the pinned toolchain and exact TLC JAR digest.

The model has two scenarios and three occurrence sample slots. It assumes valid auxiliary measurements and excludes independent product failures and unbounded histories.

The final runner directory is `final-source`. Earlier runs remain historical evidence for their recorded sources.

No production-driver fixture ran on the host. No live node, native containment campaign, hosted workflow, or extended property campaign ran.

## Failure history

The initial compilation used an incorrect helper argument count. The corrected build passed without changing the shared helper.

Five defects have retained RED evidence and corrected executable checks:

1. Missing terminal observations incorrectly became zero aggregate counts.
2. A snapshot captured before acknowledged faults incorrectly passed.
3. Missing occurrences hid an independent telemetry mismatch.
4. Qualification accepted a different occurrence identity definition.
5. A conflicting event copy escaped detection after its frontier label changed.

One focused test command timed out after 120 seconds. It produced no final Rust test result or observed exit status.

The failed run retains its partial fixture directory. A fresh-directory retry reproduced the intended failure with exit 101.

The initial helper-arity failure has a compiler log but no complete source snapshot. Each behavioral RED phase has an exact three-file source archive.

## Shared source blocker

The published base changed `scripts/run-merge-recovery-soak.sh` outside this task. Its current bytes differ from the source accepted for CLAIM-001.

| Identity | SHA-256 |
| --- | --- |
| Accepted driver source | `7f4ba9b1c9078c984251317a4cb5032772a092645bfb83d07f6950c0b43542b2` |
| Current driver source | `5552bb673d0735ac04a10566753d80b5eaf6db7d238be6a943c15c384f5187a0` |

The shared audit reports `The discharged source or specification differs.` This also blocks the selected CLAIM-004 audit before it emits a report.

The failing regression is `pending_claims_are_visible_and_strict_discharge_refuses`. Its source audit expected exit 0 and received exit 2.

A complete regression retry used `--no-fail-fast`. It recorded all 25 existing tests rather than hiding the failed source audit.

The evidence retains the upstream driver diff and failed audit invocations. This task neither changes the driver nor rewrites the accepted ledger.

TASK-017-4 remains historically complete for its accepted source. The current driver requires separate verification and source-bound acceptance before the shared gate can pass again.

## Remaining boundaries

D-07 identifies uncertainty about exact-occurrence and reason-join support on `dev`. This task does not resolve that ratification question.

Controlled fixtures use an explicit synthetic occurrence schema. They do not claim that the current node exposes that schema or those rules.

Pinned expectations define diagnostic comparisons. The classifier does not implement node retry authorization or calculate a new canonical reason-join rule.

Cross-clock duration normalization remains unsupported. Different clock domains retain unknown duration even if additional synchronization metadata is supplied.

Live interfaces, experimental policy activation, and post-merge execution remain blocked. No real baseline or comparative node result is available.

The proposed `cbc=mandatory` tag for `.github/workflows/casper-recovery.yml` remains unratified and unapplied. Existing broad tags cover the new Rust, Bash, and formal artifacts.

CLAIM-004 requires human binding acceptance. Qualification, workload pins, resource approval, and node campaigns remain under TASK-017-12.

Publication receipt-conflict handling needs separate review before CLAIM-003 acceptance. Its collector checks payload correlation before duplicate conflicts. This task does not change that collector.

CLAIM-002 and CLAIM-003 acceptance remain pending. No claim waiver or automatic task completion applies.

The Git index changed outside this session's commands. Those entries remain untouched. This assistant did not stage, commit, or push.

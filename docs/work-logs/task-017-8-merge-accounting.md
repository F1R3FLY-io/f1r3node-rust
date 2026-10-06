# TASK-017-8 Merge and Accounting Profile

## Acceptance and completion

The user accepted TASK-017-8 on 2026-09-19 with the message “I accpet TASK-017-8”.

The [acceptance report](../casper/cbc-evidence/runs/casper-merge-accounting-acceptance-20260919-01/report.json) discharges CLAIM-CASPER-SOAK-005 for the reviewed bounded pre-merge sources.

The workflow tag is ratified and applied. All twelve reviewed artifact hashes remain unchanged.

The pre-acceptance validator checked 1,476 nested references. Accepted-state native tests, all four model controls, and eleven shared tests passed again.

Claims 001–005 pass strict discharge. The full bundle remains pending for claims 006–008.

The pinned completion helper returned full integrity with no gaps. TASK-017-8 is complete, and EPIC-017 is eight of fourteen tasks complete.

The helper changed only TASK-017-8 on a tracker copy. The reviewed patch preserved unrelated task records, the tracker backup, and the TASK-017-4 adapter.

The original package and pending ledgers remain historical evidence. The original validator expects pending acceptance and does not represent the current accepted state.

This acceptance does not authorize live execution, additive activation, post-merge execution, or a passing soak. No Git publication was performed by this assistant.

The new acceptance package contains only a report, validation, and digest lists. Existing bulk evidence remains available for the TASK-017-14 retention owner.

The following verification narrative records the earlier implementation and review state.

## Scope

This task implements CLAIM-CASPER-SOAK-005 against controlled transcripts. It does not implement node accounting or activate additive merge semantics.

The starting revision is `09b0a60063815b750979864225a0a56387d6ed80`. The task started on 2026-09-19 with a clean worktree.

## Plan

- [x] Implement pinned request generation, qualification, collection, and verdict classification.
- [x] Preserve execution multiplicity, admission rejection, failed-body settlement, causal inputs, and compatibility labels.
- [x] Add controlled fixtures and bounded model controls.
- [x] Run native and isolated Linux tests, model checks, and shared regressions.
- [x] Accept the bounded binding and ratify the workflow tag.

## Boundaries

The existing accepted claims and their source artifacts remain unchanged. This profile uses a separate Rust binary and Bash runner.

Live, conditional-additive, and post-merge requests remain blocked. D-08 activation conditions do not grant this harness authority to activate a protocol.

Fixture expectations are pinned data. The profile does not recompute node execution, least rejection closure, or accounting conservation.

Claim acceptance and the proposed workflow tag require separate human decisions. Final workload qualification and node campaigns remain under TASK-017-12.

## Results

The [evidence package](../casper/cbc-evidence/runs/casper-merge-accounting-20260919-01/report.json) records source identities, executable identities, transcripts, model results, and failures.

| Check | Result |
| --- | --- |
| Native Rust tests | Seven tests passed, with 68 cases and 73 invocations. |
| Isolated ARM64 Linux tests | Seven tests passed, with 68 cases and 73 invocations. |
| Shared binding, claim, manifest, and model tests | Eleven tests passed. |
| Clean TLC control | 841 generated states and 441 distinct states, exit 0. |
| Multiplicity negative control | 255 generated states and 147 distinct states, named counterexample, exit 12. |
| Settlement negative control | 255 generated states and 147 distinct states, named counterexample, exit 12. |
| Compatibility negative control | 226 generated states and 130 distinct states, named counterexample, exit 12. |
| Strict claim audits | Claims 001–004 returned 0. Claim 005 returned 4 because binding acceptance remains pending. |

Linux execution used the pinned disposable image, an unprivileged user, no network or mounts, and resource limits. Evidence capture preceded container removal.

The native build used optimization level 0. The static Linux build used optimization level 1 with the pinned nightly toolchain.

No hosted workflow or node campaign ran. The workflow tag remains proposed, not applied.

## Retained failures

Two initial model attempts failed setup before TLC execution. The configurations used plural invariant declarations that the strict shared runner does not accept.

The corrected configurations declare one invariant per line. Each negative configuration declares only its named property and `TypeOK`.

The checks retain both failed attempts. Initial model sources are reconstructed from external commit `442e93faa` and identified separately from runtime captures.

An executable regression exposed lost failure evidence. An invalid aggregate amount stopped classification before a separately observed effect mismatch.

The RED assertion failed with Cargo exit 101. The fix isolates measurement errors, preserves independent failures, and leaves conflicting execution counts unknown.

The focused GREEN regression passed. Both final platform suites include that regression without weaker assertions.

## Binding review map

- `identity`, `prepare`, and `generate` bind compiled sources, executable identity, exact inputs, qualification labels, and deterministic generation.
- `key` separates execution identity from signatures, contexts, and admission rejection.
- `collect` verifies artifact identity and compares immutable event copies before quarantine.
- `classify` checks applied receipts, dependency order, snapshots, counts, settlement data, causal edges, and pinned rejection inventories.
- `required_accounting_fixtures` binds the three model properties to executable fixtures.
- `malformed_fields_preserve_independent_failures` binds invalid-input precedence to retained failure evidence.

The finite model assumes valid auxiliary fields and no independent product failures. Executable fixtures cover those additional cases without extending the model proof.

## Concurrent repository changes

An external publisher committed initial files in `442e93faa`. HEAD later advanced to `134e1deaa78d65dda9dd5df49404115044fb9b1e` during verification.

External staging and unrelated task updates remain intact. This assistant did not stage, commit, push, or synchronize the branch.

## Historical pending acceptance

TASK-017-8 remains in progress. CLAIM-CASPER-SOAK-005 remains pending, with bounded refutation results and no binding discharge.

The requested review covers only this controlled-transcript binding and `.github/workflows/casper-merge-accounting.yml` tagging. It does not authorize additive activation or live execution.

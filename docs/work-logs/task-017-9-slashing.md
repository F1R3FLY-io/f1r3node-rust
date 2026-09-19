# TASK-017-9 Slashing Verification

## Scope and ownership

The user authorized TASK-017-9 implementation on 2026-09-19. The owner is `pi-casper-slashing`.

The starting revision is `79d09441fb8a8824dfac53d152dca18636efd5ce`. The worktree was clean before the claim update.

This task implements the controlled-transcript profile for CLAIM-CASPER-SOAK-006. It does not implement node slash authorization, evidence reconstruction, or bisimilarity proofs.

## Coordination

The carrier-index agent retains TASK-017-10. The preparation agent retains TASK-017-12 and TASK-017-14 preparation. TASK-017-11 remains unclaimed.

The claim update changes only TASK-017-9 ownership fields and this work log. Claim publication precedes executable implementation under the coordination agreement.

The implementation request alone did not authorize a commit or push. The user subsequently published the claim before implementation began.

A carrier-only inventory update can proceed separately. It must preserve TASK-017-9, slashing artifacts, accepted claims, and shared compiled sources.

## Requirements

The [claim](../claims/casper-soak-slashing.md) defines delivery, epoch correlation, and authorization measurements.

[D-09](../casper/design/decision-ledger/09-slashing-authorization.md) preserves the current truth table, rejected-slash recovery, activation-epoch protection, and inactive economic neglect slashing.

The profile compares observations with pinned fixture expectations. It does not promote supplementary reconstruction or deferred evidence formats to protocol authority.

## Implementation plan

- [x] Read the task, claim, interface requirements, and ratified decision.
- [x] Prepare the local ownership claim and notify the carrier-index agent.
- [x] Confirm publication of this two-file claim.
- [x] Implement separate Rust generation, collection, correlation, and verdict classification.
- [x] Add controlled transcripts for merge-lost slash, delivery permutations, rebond, stale epochs, missing evidence, forged deploys, and restart.
- [x] Verify observed delivery order instead of substituting requested order.
- [x] Quarantine unrelated epochs without suppressing independent product failures.
- [x] Detect authorization differences from pinned expectations without granting slash authority.
- [x] Add the bounded model and three named negative controls.
- [x] Run native and isolated Linux fixtures, exact inventory checks, model controls, and shared regressions.
- [x] Retain source-bound evidence and failures under the current retention rule.
- [ ] Request separate binding acceptance and workflow-tag ratification.

## Intended files

- `scripts/casper-soak/src/profiles/slashing.rs`
- `scripts/casper-soak/src/bin/casper-slashing.rs`
- `scripts/casper-soak/tests/slashing.rs`
- `scripts/casper-soak/check-slashing.sh`
- `.github/workflows/casper-slashing.yml`
- `formal/tlaplus/casper_soak/profiles/slashing/`
- `docs/claims/casper-soak-slashing.md`

Existing accepted profiles, shared compiled sources, Cargo configuration, and the common auditor remain unchanged.

## Verification boundary

The initial model bound is two scenarios and three observations. Executable malformed-evidence checks supplement that finite model without expanding its proof scope.

Requested faults, applied receipts, candidate identity, evidence identity, parent pre-state, and epoch identity remain separate.

Expected rejection of stale or forged evidence can pass a scenario. A verifier counterexample instead demonstrates an intentionally defective harness rule.

Live and post-merge requests remain blocked. Synthetic qualification does not establish actual node interface support.

CLAIM-CASPER-SOAK-006 remains pending. No workflow tag, waiver, node campaign, or task completion is implied.

## Current state

The claim was published at `51615255aa4ad24779639120ea2a8c5ec0500d78`. Local HEAD and the remote branch matched before implementation.

An external glossary commit subsequently changed HEAD to `590dd1f7e0d201dc13b90ccc2e4db6740f13584b`. This task did not modify the glossary.

The controlled-transcript implementation, bounded model checks, and evidence validator passed. The task remains in progress pending binding review and workflow-tag ratification.

An external commit later captured an implementation snapshot at `137b74fdb903969d186aef9241380eef24dd4833`. That snapshot preceded completion of evidence packaging and validation.

This session did not stage, commit, or push the implementation.

## Verification results

Native macOS and isolated ARM64 Linux fixtures each passed seven Rust tests, 66 named cases, and 72 CLI invocations.

The Linux container used an unprivileged user, no network, no host mounts, no added capabilities, and explicit process, memory, CPU, and timeout limits.

The native runner checked exact invocation identities, exit codes, verdicts, source stability, and all four model controls.

| Model control | Exit | Generated states | Distinct states |
| --- | ---: | ---: | ---: |
| Clean | 0 | 3281 | 1681 |
| Requested-order substitution | 12 | 891 | 483 |
| Epoch identity omission | 12 | 834 | 450 |
| Authorization mismatch suppression | 12 | 948 | 516 |

Each negative control violated only its configured named property. The clean search completed within the declared bound.

The shared binding, claim, manifest, and model regression tests passed. Clippy passed for the new binary and integration test.

The renamed-case runner control exited 1. The interrupted runner control exited 143. Both controls retained failed summaries rather than claiming completion.

The language-server check found no diagnostics. It confirmed five paths clean and left one path inconclusive.

## Retained failures and corrections

The initial suite rejected the test-only label `live_node` as invalid input. The shared manifest contract requires `node_observation`.

The corrected live fixture uses that label and produces a blocked verdict. The original failed run and source archive remain intact.

A regression exposed missing deadline binding. The request extended the manifest deadline, and the original implementation incorrectly returned a passing scenario.

The RED run exited 101. Its assertion expected CLI exit 2 but observed exit 0. The source archive preserves the failing implementation.

The fix requires exact equality between the request deadline and manifest deadline. The final suite verifies the refusal without changing shared source.

Additional checks reject future rebond epochs and unauthorized positive fixture expectations. Independent failures survive malformed sibling fields, missing receipts, and multiple snapshots.

## Evidence package

The [review report](../casper/cbc-evidence/runs/casper-slashing-20260919-02/report.json) binds 24 source files and archives 12614 evidence files.

The [validation result](../casper/cbc-evidence/runs/casper-slashing-20260919-02/validation.json) checks 1430 nested references, both platform inventories, and 12 pending canonical ledgers.

Twelve compatibility links point to the new canonical ledgers. Existing independent compatibility records remain unchanged.

Claims 001 through 005 still pass strict discharge audits. Claim006 and the full bundle exit 4. Claims006, 007, and 008 remain pending.

The first retention attempt stopped because its privacy scan matched the scan expression itself. Its source, evidence, and failure report remain intact.

The first archive validator compared original TLC log hashes directly with redacted log bytes and exited 1.

The corrected validator checks a pinned redaction manifest. It accepts only exact path, original-hash, and redacted-hash matches.

The original validator, failed exit, corrected run, and audit outputs remain in the supplementary validation archive. No assertion or model property was weakened.

## Review boundary

The synthetic fixtures test pinned comparisons. They do not establish a reviewed node truth table, actual interface qualification, or an observed node slash.

No accepted profile or shared compiled source changed. No node campaign, policy activation, evidence upload, claim discharge, or workflow tag is authorized by these checks.

## Handoff

```yaml
handoff_status: ready
next_steps:
  - Review the bounded Claim006 binding and pinned synthetic expectations.
  - Obtain separate human acceptance and workflow-tag ratification.
  - Preserve both evidence packages and the supplementary validation archive.
  - Publish only with separate user authorization.
```

The proposed tag is `.github/workflows/casper-slashing.yml cbc=mandatory cbc-weight=high`. The tag remains unapplied.

TASK-017-14 owns later evidence publication and reduction. This task neither uploads nor removes evidence.

## Hosted verification and handoff

Hosted run `35452747041` passed at published revision `fcc2fb22270d403757785f6955915e40dec3625d`. Artifact `10587177940` passed ZIP digest verification.

All 21 hosted source hashes match the current tree. Seven tests, 66 cases, 72 invocations, four model controls, and 715 nested artifact references passed verification.

The [hosted report](../casper/cbc-evidence/runs/casper-slashing-hosted-20260919-01/report.json) records these checks without changing the prior review package or pending ledgers.

The user asked to complete the remaining slashing steps and hand off other work. Explicit human binding acceptance and workflow-tag ratification were requested separately.

The [carrier handoff](../handoffs/pi-casper-slashing--pi-soak-carrier-index-linux--20260919T154942Z.md) preserves TASK-017-10 ownership and requests confirmation of a future TASK-017-11 owner.

The [preparation handoff](../handoffs/pi-casper-slashing--claude-session-9f19b46c--20260919T154942Z.md) transfers retention and remaining preparation context. The preparation agent has no verified mesh route.

The existing evidence-store draft release ID is `391939637`. The authenticated read-only lookup confirmed its tag, target, and draft status.

No release asset was uploaded. No claim was discharged, workflow tag applied, task closed, or commit created during this hosted-verification step.

## Bounded acceptance on 2026-09-19

The user subsequently accepted Claim006's bounded binding and assigned TASK-017-11 to the carrier agent:

> accepted for Claim006's bounded binding and go ahead with the assignement for 017-11 to the other agent

The [acceptance report](../casper/cbc-evidence/runs/casper-slashing-acceptance-20260919-01/report.json) records that exact approval and its limited scope.

All 12 reviewed artifact hashes remain unchanged. Twelve canonical ledgers now discharge the bounded pre-merge binding and preserve the previous pending records in a small metadata archive.

The previous review package and hosted report remain unchanged. Their pending-state validators are historical after this acceptance.

The accepted-state native runner passed seven tests, 66 cases, 72 invocations, and all four model controls. Eleven shared regression tests also passed.

The [acceptance validation](../casper/cbc-evidence/runs/casper-slashing-acceptance-20260919-01/validation.json) confirms strict discharge for Claims001 through 006. The full bundle exits 4 for Claims007 and 008.

The reviewed isolated Linux evidence uses identical implementation bytes. This acceptance step did not repeat the isolated Linux run.

Workflow-tag ratification was not explicit in this approval. The tag remains unapplied, and TASK-017-9 stays in progress pending that decision.

The carrier agent received the explicit TASK-017-11 assignment and the existing claim-publication requirement. This session did not claim that task or authorize a commit.

TASK-017-14 must retain `target/task-017-9-acceptance/` before scratch cleanup. Those files contain acceptance scripts, preflight checks, fresh fixtures, model logs, and strict audits.

No node execution, policy activation, evidence upload, Git commit, or push occurred during acceptance recording.

## Workflow tag ratification and closure

The user ratified the workflow tag on 2026-09-19 with the message "017-9 should now be closed out". The session `claude-session-9f19b46c` recorded the ratification and closed the task.

The `.gitattributes` file gains one line. It marks `.github/workflows/casper-slashing.yml` as `cbc=mandatory` with high weight. The entry matches the four profile workflows that earlier tasks ratified.

The claims checker binds the claim specification digest into every ledger record and into the discharge evidence report. Editing the specification therefore requires new evidence bytes.

The acceptance report keeps its original bytes. A separate ratification package records the new decision, and the twelve ledger records now cite that package.

The ratification package is `docs/casper/cbc-evidence/runs/casper-slashing-ratification-20260919-01/`. It holds the report, the validation, the digest list of the twelve canonical records, and an archive of those records as accepted.

Each record drops `workflow-tag-ratification` from its pending list and keeps `external-evidence-publication`. Each record points its previous-ledger reference at the new archive.

The strict audit after ratification returns exit 0 for Claims001 through 006. Claims007 and 008 stay pending, and the full bundle returns 4.

The reviewed artifact digests did not change. The workflow file itself was not edited.

Closure does not qualify a live adapter, activate a deferred policy, or authorize node execution or a post-merge run. The soak tier stays pending.

External evidence publication stays pending. TASK-017-14 must retain `target/task-017-9-acceptance/` and `target/task-017-9-hosted-35452747041/` before scratch cleanup.

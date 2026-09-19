# TASK-017-10: Carrier Index Profile

## Ownership

- Task: TASK-017-10.
- Epic: EPIC-017.
- Implementer: `pi-soak-carrier-index-linux`.
- Started: 2026-09-19T06:11:36Z.
- Branch: `formal/soak-casper-consensus`.
- Starting revision: `442e93faa`.
- Claim: `CLAIM-CASPER-SOAK-008`.

## Scope

This task implements and verifies the carrier-index harness profile. It does not change node behavior or discharge `CLAIM-FINALITY-002`.

The profile must compare matched candidate, DAG, scan-window, and availability inputs. Missing path receipts and counters must remain unknown.

Unavailable node interfaces block affected live scenarios. Controlled transcripts do not supply node evidence or authorize a soak run.

## Plan

- [x] Record and publish task ownership before implementation.
- [x] Implement the profile generator, collector, classifier, and executable fixtures.
- [x] Run bounded model controls and matching implementation tests.
- [x] Record source-specific evidence and remaining interface limits.
- [ ] Obtain the required binding acceptance before task completion.

## Coordination

The remote machine completed TASK-017-8 and now owns TASK-017-9. TASK-017-11 remains unclaimed.

TASK-017-12 owns node interface qualification and approved live deployment. This task does not dispatch either operation.

Profile-specific files belong to this task. Shared inventory and workflow changes require coordination before editing.

Exchange published commit IDs between machines. Use fast-forward-only pulls from clean checkpoints. Each new commit requires separate user approval.

## Current state

The profile implementation and bounded verification are ready for binding review. TASK-017-10 remains in progress, and CLAIM-CASPER-SOAK-008 remains pending.

The profile checks paired inputs, path engagement, counters, artifact identities, fault receipts, and restart links. Live, post-merge, and typed-identity execution remain blocked.

## Verification

| Check | Result |
| --- | --- |
| Host release fixtures | 23 tests, 88 cases, and 91 invocations passed. |
| Isolated Linux fixtures | The same 23 tests, 88 cases, and 91 invocations passed. |
| Clean bounded model | TLC generated 1,201 states and found 625 distinct states. |
| Three model defect controls | Each produced exit 12 with its registered invariant violation. |
| Shared manifest, model, inventory, and claim tests | 11 tests passed. |
| Renamed required fixture control | The runner rejected the replacement with exit 1. |
| Interrupted runner control | The runner recorded failure with exit 143, not a passing summary. |
| Formatting, targeted Clippy, shell syntax, and diff checks | Passed. |
| Accepted claims 001 through 004 | Each strict audit returned exit 0. |
| Claim 008 | The strict audit returned exit 4 because acceptance remains pending. |

The container used a read-only root, no network, no capabilities, an unprivileged user, and explicit resource limits. The evidence records its image and executable hashes.

Review tests exposed four defects before repair. These concerned malformed measurements, contradictory predecessor copies, absent fault schedules, and comparison without complete counters.

The repaired tests preserve independent product failures and reject unsupported comparisons. The final evidence validator checked 2,182 nested references.

## Retained failures

The first compile found an unsupported `Result` method. Its replacement compiled successfully. The initial debug fixture run exceeded its 180-second tool limit.

The timeout is not a passing test result. Subsequent release runs completed. The initial commit hook also rejected formatting before targeted formatting corrected the files.

TLC 1.8.0 could not run on the installed Java 8 runtime. The verified campaign uses the same pinned TLC 1.7.4 JAR as TASK-017-8.

The evidence retains failure logs and the four failing review tests. Not every earlier attempt has a retained source snapshot.

## Evidence and handoff

The [report](../casper/cbc-evidence/runs/casper-carrier-index-20260919-01/report.json) binds 22 source digests and 12 profile artifacts.

The package retains only its report, validation result, bundle digest, and redaction list in Git. Twelve canonical ledger records retain pending status without waivers.

The bulk bundle remains local at `/tmp/carrier-index-checks/casper-carrier-index-20260919-01.external.tar.gz`. Its SHA-256 is `51e7b6baae754ca393abe4f042dafefb86228a4c2d9a7b34a85ddcbbf43c1543`.

The original bundle contains workstation account paths. Do not publish that bundle. The publication copy removes those paths from text and withholds two native binaries.

The native executable hashes remain recorded. Fixture inputs, fixture reports, source files, and their nested hashes remain unchanged.

The user requested the remaining carrier work, including evidence publication, on 2026-09-19. Binding acceptance and workflow-tag ratification remain separate decisions.

## Hosted verification and publication

The [publication report](../casper/cbc-evidence/runs/casper-carrier-index-publication-20260919-01/report.json) records hosted verification and the upload blocker.

[Hosted run 35429044639](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/35429044639) passed for revision `51615255a`. Its artifact contains 23 passing tests, 88 cases, 91 invocations, and four passing model controls.

The downloaded ZIP matches the GitHub artifact digest. Its 21 source hashes match the reviewed profile inputs. Validation checked 3,274 nested evidence references across native, isolated, and hosted fixtures.

The publication archive contains 14,607 members. Its SHA-256 is `941c3c95a05e972e24bea9b33ba6c08b3da439dda7a87ceb63fa35da78dd0ca0`.

The archive is local at `/tmp/carrier-index-checks/publication-review/casper-carrier-index-publication-20260919-01.external.tar.gz`. The current API credential cannot find the planned draft release `cbc-evidence-epic-017`. The release-tag endpoint returns HTTP 404.

No upload occurred. The shared release, its index, and its checksum list remain unchanged. The remote agent received a request for the release ID and the shared inventory update.

The historical review report changed only to remove the workstation prefix from its isolated-run identity field. All 12 pending ledger records now reference the redacted report hash.

The source hashes and historical results remain unchanged. The publication report records the old and new report hashes. Git history still contains the original account path.

Strict audits for Claims 001 through 005 passed after the report redaction. Claim 008 still returns exit 4 because binding acceptance remains pending.

## Requested binding review

The proposed acceptance covers only the published carrier source and the bounded controlled-transcript checks. The finite model has two scenarios and three observations per scenario.

The profile requires matched comparison inputs, observed traversal paths, known counters, and applied fault receipts. Missing observations cannot become measured zero values or passing coverage.

Independent product failures remain recorded when observations are incomplete or malformed. Filesystem checks retain their cooperating-writer assumptions and do not prove durability or hostile-process containment.

The proposed workflow tag is `.github/workflows/casper-carrier-index.yml cbc=mandatory cbc-weight=high`. Human ratification must precede this attribute change.

Acceptance does not qualify live interfaces, prove node carrier equivalence, activate a protocol, or authorize a soak. CLAIM-FINALITY-002 remains outside this task.

Evidence upload and remote retrieval remain completion requirements. Binding review must precede claim discharge and task closure.

## Carrier inventory and compatibility links

The user relayed approval for a carrier-only tracker update. `docs/ToDos.md` now lists all 12 carrier artifacts in the epic and task inventories.

TASK-017-10 remains in progress. Claim 008 remains pending, and its workflow tag remains unratified. The slashing and preparation task sections remain byte-identical.

Twelve relative symlinks now expose the canonical records through `docs/cbc-evidence/`. This corrects a packaging omission under `docs/casper/README.md`. The links introduce no separate status or duplicate evidence.

The default-directory gate resolves all 11 currently mandatory carrier artifacts to pending records. The unratified workflow is outside that mandatory set. Strict claim audits still pass for Claims 001 through 005 and return the expected pending exit for Claim 008.

YAML parsing, both carrier artifact inventories, symlink targets, and the diff check passed. These checks do not discharge any claim.

The requested six-workflow bindings inventory change is separate. Its script is an accepted Claim-001 artifact. An edit invalidates its accepted digest and requires renewed binding acceptance.

The user pulled revision `137b74fdb`, which adds the slashing workflow and the preparation agent's handoff. All six profile workflows are now available.

The carrier and slashing workflow attributes are still unspecified. Inventory inclusion must not imply workflow-tag ratification.

The shared inventory script, accepted claims, shared compiled sources, and `.gitattributes` remain unchanged. Its proposed batch change awaits explicit expanded scope.

No node campaign, claim discharge, or task closure has occurred.

## Approval and transfer checkpoint

The Mac publisher relayed explicit user approval in message `01a0ba8f-fc95-7575-99d9-835d4b9b5b79` on 2026-09-19. The approval covers the bounded Claim 008 binding and the carrier workflow tag.

The approved tag is `.github/workflows/casper-carrier-index.yml cbc=mandatory cbc-weight=high`. The approval also permits upload of the sanitized carrier archive to the existing draft release.

These decisions supersede the earlier pending human decisions. Source-specific acceptance records, strict audits, and task closure will follow verified evidence publication.

The existing draft release has ID `391939637`. Both locally available token selections return HTTP 403 for that release. The earlier HTTP 404 did not establish that the release was absent.

The Mac publisher reports access through its existing stored credential. No credential transfer, replacement release, shared index change, or draft publication is authorized.

Fresh checks at revision `4a7bf8960800d76be82d887c205a898169bdb8fe` confirmed all 21 carrier source digests. The sanitized archive remains 2,529,945 bytes with the recorded `941c3c95...` SHA-256.

All 14,606 manifest entries match their archived bytes. The archive has 14,607 regular-file members with safe relative paths. Both native binaries remain withheld.

The account-path and credential-pattern checks found no matches. The validation record is `/tmp/carrier-index-checks/publication-review/transfer-validation-01.json`.

The publisher received the archive path, complete digest, size, and source-freshness result. An existing authorized file-transfer route remains to be confirmed.

No upload or remote retrieval has occurred in this session. Carrier tag integration must preserve the separate slashing tag and TASK-017-9 closure work.

TASK-017-11 implementation changes remain separate. This approval does not authorize Git publication, a shared Claim 001 change, or node execution.

## Verified publication checkpoint

The Mac publisher uploaded the sanitized archive and verified its remote download at `2026-09-19T16:53:38Z`. The asset ID is `575126186` on draft release `391939637`.

The download was byte-identical to the sanitized archive. Its SHA-256 is `941c3c95a05e972e24bea9b33ba6c08b3da439dda7a87ceb63fa35da78dd0ca0`, and its size is 2,529,945 bytes.

The publisher used an existing SSH alias and stored GitHub login. No credentials were transferred. The release remains draft, and its 24 earlier assets, shared index, and checksum list remain unchanged.

This session verified the transferred receipt and all 12 entries in `receipt-transfer.sha256`. The receipt SHA-256 is `7ca046ac32e63d014e315f9a52d817aa533fef1f8612cd5a7bb3b6474744753e`.

The receipt and supporting files remain in `/tmp/carrier-index-checks/mac-publication-receipt-20260919-01/`. The publication report remains unchanged as a historical record of the earlier blocked attempt.

The approved carrier workflow tag is now applied in `.gitattributes`. The tracker records the ratified tag and verified publication. Claim 008 and its canonical ledgers remain pending until source-specific acceptance records and strict audits are complete.

TASK-017-10 remains in progress. This checkpoint does not close either task, change Claim 001, or authorize node execution.

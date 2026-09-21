# Casper pre-merge evidence handoff

## Status and authority

This handoff is prepared for review, not accepted. TASK-017-13 remains in progress and blocked on TASK-017-12 results and the remaining acceptance requirements.

The latest review uses commit `2530b83853e946dd32a850d78dbbc9491fc23c3f`. The [TASK-017-13 work log](../work-logs/task-017-13-gate-handoff-2026-09-21.md) records the ownership transfer, current gates, and delivery blockers.

The previous preparation used commit `5cc4e665b52f0cc5279ec577ac5075b5b9897caf`. Its [review report](../casper/cbc-evidence/runs/casper-handoff-preparation-20260919-02/report.json) retains that checkpoint's source identities and gate outcomes.

The [initial review report](../casper/cbc-evidence/runs/casper-pre-merge-review-20260919-01/report.json) retains the checkpoint at `3aa79d0c1c91988be781c8380d8f6952b7d72868`. Historical sections below retain their original counts and outcomes.

On 2026-09-19, GitHub reported that PR #436 targets `dev` at `6940a5beb4aa806d3d75f6df3be9f238512fcc2f`. PR #436 was open and not a draft.

Earlier records of the stack parent and draft status are historical. This review does not change the PR base or its status.

PR #216 remains open at `619beb4a4a7ad3f8967d4586daf0f5c552bd150e`, with no merge commit.

The approved amendment in `5cc4e665b` permits an EPIC-018 branch stacked on PR #216 after handoff acceptance. It does not require a merge before branch creation.

The owner must record each candidate revision, rebase after candidate updates, and retarget the follow-on PR to `dev` after the merge.

Post-merge claim discharge still requires the actual merge revision. Neither the branch amendment nor this draft authorizes a campaign or satisfies that evidence requirement.

## Evidence boundaries

| Evidence class | Current result | Limit |
| --- | --- | --- |
| Bounded models | The eight accepted claim inventories cite passing finite safety checks and named negative controls. | The models do not prove node correctness or unbounded liveness. |
| Executable bindings | The current audit reports Claim001 pending and seven profile claims discharged. All soak fields remain pending. | Controlled fixtures do not qualify live adapters. |
| Product observations | TASK-017-12 has not supplied completed baseline evidence to this review. | No campaign result, seed, or run identity is inferred. |
| Full changed scope | The default gate has 29 gaps among 149 artifacts. The canonical diagnostic has 28 gaps. Both exit 4. | Campaign claims, missing records, and the independent governance claim require current evidence. |

The claim audit checks metadata, not node behavior. The separate gate-only candidate has controlled fixture checks, not hosted verification or protection enforcement.

Earlier execution revisions remain unchanged.

The accepted claims use `bounded-safety-pass`, `not-applicable`, and `passed` for refutation, construction, and binding. Every claim retains `soak: pending`.

## Accepted source inventory

The [claim index](../claims/casper-soak-harness.md) links all eight specifications. The source-bound audit checks 123 declared artifacts across these claims.

The [previous source manifest](../casper/cbc-evidence/runs/casper-handoff-preparation-20260919-02/source-artifacts.sha256) records 127 artifact hashes. It covers the accepted inventories and the expanded mandatory scope.

The [previous input manifest](../casper/cbc-evidence/runs/casper-handoff-preparation-20260919-02/reviewed-inputs.sha256) records the reviewed specifications, reports, and coordination documents at that earlier revision.

The initial review package retains its earlier 125-source and 142-input manifests. Those historical manifests do not describe the new campaign helpers.

| Claim | Accepted report | Post-merge task |
| --- | --- | --- |
| 001 | [Hosted workflow renewal](../casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/report.json) | TASK-018-1, TASK-018-2, TASK-018-5, TASK-018-6 |
| 002 | [Three-profile acceptance](../casper/cbc-evidence/runs/casper-profile-acceptance-20260919-01/report.json) | TASK-018-3 |
| 003 | [Three-profile acceptance](../casper/cbc-evidence/runs/casper-profile-acceptance-20260919-01/report.json) | TASK-018-3 |
| 004 | [Three-profile acceptance](../casper/cbc-evidence/runs/casper-profile-acceptance-20260919-01/report.json) | TASK-018-3, TASK-018-5 |
| 005 | [Accounting acceptance](../casper/cbc-evidence/runs/casper-merge-accounting-acceptance-20260919-01/report.json) | TASK-018-4 |
| 006 | [Slashing ratification](../casper/cbc-evidence/runs/casper-slashing-ratification-20260919-01/report.json) | TASK-018-3 |
| 007 | [Protocol and Phlo acceptance](../casper/cbc-evidence/runs/casper-version-phlo-acceptance-20260919-01/report.json) | TASK-018-4 |
| 008 | [Carrier acceptance](../casper/cbc-evidence/runs/casper-carrier-index-acceptance-20260919-01/report.json) | TASK-018-4 |

### Model identities

These SHA-256 values identify the current accepted model sources. Configuration and verification-plan hashes are in the source manifest.

| Model | SHA-256 |
| --- | --- |
| CasperSoakHarness | `e08ce354f1e09b9f0873e6f730e509c4be6b394acc0bce7ff5ef53805d91b0c4` |
| AuthorityFinality | `3a10b0683796a16feaa7fb90df679c5338e1f01bf5fcc5bffca44cce41633e43` |
| Publication | `4b51f9c8aee88ac6526b686edfb2a985fb6e6de0230639ca81567eda9543f287` |
| Recovery | `d030a4ede0f47c1ebd2e3b5a93433c14d22924ef98b5a0ea0a84e9fd306d659d` |
| MergeAccounting | `ba26aa25b0b7e0d4b74cc18541433c0ca67dd1e737de9cbc30f7c1a519d05de5` |
| Slashing | `42dcaf827385e79506bee7087323f640d3f756e60c01b684731c361f6f5c2ab3` |
| VersionPhlo | `8e62735f8a321cc6ed2687558f1913f9bdf3a747a6c0119a4c3da77ec531f570` |
| CarrierIndex | `db2e55be76a8ba4221282a47695b3b50a8d536b912e4b7cd10650ede9521fd59` |

The lifecycle bounds remain two candidates, two segments, four iterations, and one active child. Each profile retains its own recorded finite bounds.

TLC controls require completed clean exploration or exit 12 with the named invariant and trace. Tool errors and timeouts remain non-passing.

## Initial pre-merge gate review

At the initial review, the PR diff contains 1,609 files and 123 mandatory artifacts. It has 259,106 insertions and 643 deletions before these preparation documents.

The exact claim inventories cover 121 of those mandatory artifacts. Two additional mandatory artifacts retain pending canonical records:

- `formal/tlaplus/casper_soak/README.md`
- `formal/tlaplus/casper_soak/verification-plan.jsonc`

These artifacts are outside the eight declared inventories. Their stale scaffold records require a separate, current-source review and evidence decision before closure.

The default gate reports seven pending records and exits 4. The canonical-directory diagnostic reports the two records above and also exits 4.

Five additional default records concern the two legacy workflows, the driver fixture, the shared TLC runner, and the driver. Their canonical records pass.

The Claim001-only compatibility check previously reported six pending records. One covers the unchanged summary writer, which is absent from this PR diff.

Do not overwrite historical records or remove tags to make these checks pass. Resolve ledger routing and current-source evidence explicitly.

The disk-admission fixture remains declared but untagged. The summary writer remains declared and mandatory but unchanged against this PR base.

## Historical canonical record resolution

The [formal-area review](../casper/cbc-evidence/runs/casper-formal-area-records-20260919-01/report.json) resolves both pending canonical documentation records.

The README now states a separate documentation contract. The plan metadata reflects accepted bounded bindings without changing model inputs or executable claim inventories.

Fresh lifecycle controls pass for the updated plan digest. One consistency check and eight exact-exit refusal checks verify metadata, references, and preserved obligations.

The canonical gate now passes for all 123 changed mandatory artifacts. The default gate still reports five independent historical gaps and exits 4.

Existing compatibility symlinks expose the two updated canonical records. No compatibility record or symlink was overwritten.

The eight executable claim audits still pass, with all soak fields pending. TASK-017-13 remains open for compatibility routing, baseline evidence, owners, and handoff acceptance.

## Historical compatibility routing review

The [routing report](../casper/cbc-evidence/runs/casper-compatibility-routing-20260919-01/report.json) resolves four stale lookup paths. Each path now links to its unchanged canonical record.

Exact historical copies remain beside those links with the suffix `.historical-f8623a54e.md`. Their relative references and pending statuses remain unchanged.

The default gate now reports one gap and exits 4. The canonical gate and eight executable claim audits still pass.

The auditor covers CLAIM-CASPER-SOAK-001 through 008 only. A zero exit means those eight claims discharge, not that every claim under `docs/claims/` discharges. The [tier document](../cbc-verification-tiers.md#auditor-coverage) names the claims governed separately and explains why extending the auditor is costly.

The remaining workflow record cites `CLAIM-SOAK-GATE-001`, not a Casper profile claim. Its historical specification includes required-check enforcement and workflow-control identity obligations.

The prerequisite record describes retirement with the digest inventory but retains open obligations. This review infers no blanket supersession or waiver.

A maintainer must resolve that applicability question and review current evidence. Historical enforcement observations are not fresh checks of current repository settings.

## Current closure requirements

The earlier [hosted renewal](../work-logs/soak-formal-gate-hosted-renewal.md) restored Claim001 and both formal-area documentation records. The current review reports Claim001 pending and seven profile claims discharged.

At the earlier checkpoint, the default gate reported these gaps:

| Artifact | Current requirement | Responsible role |
| --- | --- | --- |
| `.github/workflows/slashing-tests.yml` | Complete the independent governance claim after baseline availability, rule activation, and enforcement tests. | Authorized gate operator, with TASK-017-13 evidence review |
| `scripts/casper-soak/campaign.sh` | Register and discharge the source-bound campaign-helper claim. | TASK-017-12 owner |
| `scripts/casper-soak/test-campaign.sh` | Register and discharge the mandatory fixture artifact. | TASK-017-12 owner |

The two campaign helpers are mandatory and high-weight. Their synthetic fixture results do not add them to an accepted claim inventory.

The [authorization record](../work-logs/soak-formal-gate-authorization.md) approves conditional protection changes, enforcement tests, and evidence-backed final acceptance. It is not an enforcement result.

At the latest recorded check, `dev` lacks `Formal verification gate`. Baseline availability remains the activation prerequisite, and Git publication or merge requires separate authorization.

No deferral of this governance requirement has been approved. The user now requests independent gate delivery to `dev` and the approved protection change.

The [current work log](../work-logs/task-017-13-gate-handoff-2026-09-21.md) records the gate-only candidate and its limits. No merge, protection update, or acceptance has occurred.

## Baseline inputs still required

The user transferred TASK-017-12 to another agent. The replacement owner's identity has not been supplied to this session.

The [dispatch preparation](../work-logs/task-017-12-preparation.md) retains the resource approval and qualification requirements.

The approved baseline budget now covers one four-hour preflight runner and two 26-hour baseline runners. Each runner has 64 GB of memory.

Each candidate must receive a full 24-hour workload window. The Linux owner still must resolve workflow integration, launch limits, candidate selection, and live qualification.

The separate 60-hour campaign is approved after a passing baseline. Its candidate count and runner lifetime limits still need an explicit decision.

The proposed two additional 64-hour runners are not approved by the baseline budget. Additional repetitions remain unapproved.

Before final review, record whether the 60-hour phase is a required TASK-017-13 input or a separately tracked delivery. Approval alone does not decide that requirement.

The final handoff still needs these baseline fields:

- Immutable node, harness, external harness, image, workload, and configuration identities.
- Qualified adapter records, with unsupported capabilities and the recovery merge dependency stated separately.
- Preflight run identity and outcome.
- Each campaign seed, run identity, repetition, duration, terminal outcome, and retained product failure.
- Metrics definitions and comparability limits.
- Scan benchmark and User Contract Concurrency baseline evidence.

These campaign fields remain unknown in this review. A fixture seed cannot substitute for a campaign seed.

D-07 Reading A is ratified. Recovery qualification waits for the actual PR #216 merge because the selected pre-merge node lacks its occurrence store.

Do not report recovery as passed or require an unavailable recovery observation to appear in the pre-merge baseline. Authority and publication qualification retain their own requirements.

[D-11](../casper/design/decision-ledger/11-cbc-fv-governance.md) retains the scan benchmark requirement. No baseline benchmark result has been supplied here.

The existing `ucc_tests` job invokes User Contract Concurrency tests for Docker and subprocess providers on AMD64. Workflow presence does not establish a baseline result.

TASK-018-5 explicitly owns both post-merge reruns. That assignment does not waive missing pre-merge evidence.

### Linux evidence delivery checklist

Deliver the following records for each candidate. This checklist does not authorize dispatch or assign additional work outside TASK-017-12.

| Required record | Contents needed for review | Current delivery status |
| --- | --- | --- |
| Admission | Current source-bound helper claims, complete inventory, qualification results, immutable workload and candidate pins. | Not supplied |
| Preflight | Run and attempt IDs, inputs, image identity, independent outcome, logs, and evidence checksums. | Not supplied |
| Baseline | Platform, seeds, exact workload start and end, full duration, metrics definitions, run IDs, and terminal verdict. | Not supplied |
| Failure history | Product failures, infrastructure stops, partial evidence, independent exits, and any approved repetition. | Not supplied |
| Resources | Actual machine count, memory, launch count, lifetime limits, capture, and cleanup results. | Not supplied |
| Governance baselines | Scan benchmark and User Contract Concurrency results with revisions, commands, counts, and digests. | Not supplied |
| Follow-on campaign | Explicit phase scope, approved candidate and runner limits, baseline prerequisite, and result or pending state. | Not supplied |

A non-passing result must remain visible. Submitting a failure report does not automatically satisfy acceptance or authorize the 60-hour phase.

## Post-merge ownership

The tracker names tasks but no assigned implementers for TASK-018-1 through TASK-018-6. Owner confirmation remains a handoff blocker.

The amended epic contract permits earlier branch preparation. TASK-018-1 still requires actual merge evidence for completion, and its dependent tasks retain their recorded completion gates.

The tracker also retains an older prose start condition that conflicts with the amendment. This preparation follows the explicit amendment and flags that prose for reconciliation.

| Task | Required delivery | Assigned implementer |
| --- | --- | --- |
| TASK-018-1 | Verify the actual merge ancestry and accepted handoff. | Unassigned |
| TASK-018-2 | Identify changed interfaces and renew affected bindings. | Unassigned |
| TASK-018-3 | Reverify authority, publication, recovery, and slashing profiles. | Unassigned |
| TASK-018-4 | Reverify accounting, carrier, and Phlo profiles. | Unassigned |
| TASK-018-5 | Run approved comparisons and both governance reruns. | Unassigned |
| TASK-018-6 | Audit the new source-bound evidence and submit the separate PR. | Unassigned |

## Assumptions and retention

The accepted containment limits remain in force. B44 can leave writers after simultaneous driver and crash-monitor death.

Host controls depend on Linux pidfds, procfs, trusted paths, and available kernel operations. Transition locks constrain cooperating writers, not every future process.

Docker-daemon behavior, storage availability, and enforced child termination remain explicit assumptions. A host failure cannot produce a passing result from incomplete evidence.

Live observation interfaces remain separately qualified. D-07 Reading A is resolved, but recovery qualification still depends on the merged implementation.

Historical missing references and unavailable independent exits remain unresolved where recorded.

PR #441 diagnostic claims remain separate from the eight claims in this handoff. Diagnostic test results do not discharge those claims.

TASK-017-14 owns retention and reduction after this task closes. Preserve prior records, failed attempts, private execution archives, and exact report bytes.

TASK-017-15 owns separately authorized release publication. Draft-release consumers use numeric release identifier `391939637` until publication.

## Handoff acceptance checklist

- [x] Identify current accepted source-bound reports and preserve historical checkpoints.
- [x] Review the actual PR scope and list all current mandatory-artifact gaps.
- [x] Record the Linux delivery fields, approved resource boundaries, and missing results.
- [x] Distinguish stacked branch preparation from post-merge claim discharge.
- [ ] Receive and review required TASK-017-12 evidence and resolve the campaign completion scope.
- [ ] Resolve the campaign-helper and independent governance claim gaps.
- [ ] Review scan and concurrency baseline evidence.
- [ ] Confirm named TASK-018 implementers and receive handoff acceptance.
- [ ] Repeat current-source, full-scope, and strict task-completion checks.

Prepared by `pi-casper-handoff-mac`. Acceptance recipient, acceptance timestamp, and accepted evidence revision remain unassigned.

TASK-017-14 reduction and TASK-017-15 publication remain gated on their recorded prerequisites. This draft does not change those dependencies.

This preparation authorizes no dispatch, policy activation, upload, deletion, release publication, waiver, or post-merge execution.

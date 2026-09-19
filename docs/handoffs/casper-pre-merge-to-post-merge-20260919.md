# Casper pre-merge evidence handoff

## Status and authority

This handoff is a draft for TASK-017-13. It does not close EPIC-017 or authorize EPIC-018.

The review uses commit `3aa79d0c1c91988be781c8380d8f6952b7d72868`. The [review report](../casper/cbc-evidence/runs/casper-pre-merge-review-20260919-01/report.json) records exact identities and gate outcomes.

On 2026-09-19, GitHub reported that PR #436 targets `dev` at `6940a5beb4aa806d3d75f6df3be9f238512fcc2f`. PR #436 was open and not a draft.

Earlier records of the stack parent and draft status are historical. This review does not change the PR base or its status.

PR #216 remains open, with no merge commit. EPIC-018 requires its actual merge in the selected `dev` history and acceptance of this completed handoff.

## Evidence boundaries

| Evidence class | Current result | Limit |
| --- | --- | --- |
| Bounded models | The eight accepted claim inventories cite passing finite safety checks and named negative controls. | The models do not prove node correctness or unbounded liveness. |
| Executable bindings | All eight source-bound claim audits pass. | Controlled fixtures do not qualify live adapters. |
| Product observations | TASK-017-12 has not supplied completed baseline evidence to this review. | No campaign result, seed, or run identity is inferred. |
| Full changed scope | Both artifact-gate checks fail. | Claim-level success cannot cover undeclared mandatory artifacts. |

This review executes metadata audits, not models, fixtures, or nodes. Earlier execution revisions remain unchanged.

The accepted claims use `bounded-safety-pass`, `not-applicable`, and `passed` for refutation, construction, and binding. Every claim retains `soak: pending`.

## Accepted source inventory

The [claim index](../claims/casper-soak-harness.md) links all eight specifications. The source-bound audit checks 123 declared artifacts across these claims.

The [source manifest](../casper/cbc-evidence/runs/casper-pre-merge-review-20260919-01/source-artifacts.sha256) records 125 artifact hashes. These cover both the claim inventories and the changed mandatory scope.

The [evidence manifest](../casper/cbc-evidence/runs/casper-pre-merge-review-20260919-01/evidence-inputs.sha256) binds the specifications, canonical ledgers, accepted reports, attributes, and governance inputs.

| Claim | Accepted report | Post-merge task |
| --- | --- | --- |
| 001 | [Inventory renewal](../casper/cbc-evidence/runs/casper-binding-inventory-renewal-20260919-01/report.json) | TASK-018-1, TASK-018-2, TASK-018-5, TASK-018-6 |
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

## Canonical record resolution

The [formal-area review](../casper/cbc-evidence/runs/casper-formal-area-records-20260919-01/report.json) resolves both pending canonical documentation records.

The README now states a separate documentation contract. The plan metadata reflects accepted bounded bindings without changing model inputs or executable claim inventories.

Fresh lifecycle controls pass for the updated plan digest. One consistency check and eight exact-exit refusal checks verify metadata, references, and preserved obligations.

The canonical gate now passes for all 123 changed mandatory artifacts. The default gate still reports five independent historical gaps and exits 4.

Existing compatibility symlinks expose the two updated canonical records. No compatibility record or symlink was overwritten.

The eight executable claim audits still pass, with all soak fields pending. TASK-017-13 remains open for compatibility routing, baseline evidence, owners, and handoff acceptance.

## Baseline inputs still required

TASK-017-12 remains with `pi-soak-carrier-index-linux`. The [dispatch preparation](../work-logs/task-017-12-preparation.md) defines its resource approval and qualification requirements.

Claim001 renewal is now committed and passes. Earlier preparation text that calls that renewal pending is historical.

The final handoff still needs these baseline fields:

- Immutable node, harness, external harness, image, workload, and configuration identities.
- Qualified adapter records, including the unresolved D-07 recovery prerequisite where applicable.
- Preflight run identity and outcome.
- Each campaign seed, run identity, repetition, duration, terminal outcome, and retained product failure.
- Metrics definitions and comparability limits.
- Scan benchmark and User Contract Concurrency baseline evidence.

These campaign fields remain unknown in this review. A fixture seed cannot substitute for a campaign seed.

[D-11](../casper/design/decision-ledger/11-cbc-fv-governance.md) retains the scan benchmark requirement. No baseline benchmark result has been supplied here.

The existing `ucc_tests` job invokes User Contract Concurrency tests for Docker and subprocess providers on AMD64. Workflow presence does not establish a baseline result.

TASK-018-5 explicitly owns both post-merge reruns. That assignment does not waive missing pre-merge evidence.

## Post-merge ownership

The tracker names tasks but no assigned implementers for TASK-018-1 through TASK-018-6. Owner confirmation remains a handoff blocker.

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

Live observation interfaces remain separately qualified. D-07 semantics, historical missing references, and unavailable independent exits remain unresolved where recorded.

PR #441 diagnostic claims remain separate from the eight claims in this handoff. Diagnostic test results do not discharge those claims.

TASK-017-14 owns retention and reduction after this task closes. Preserve prior records, failed attempts, private execution archives, and exact report bytes.

TASK-017-15 owns separately authorized release publication. Draft-release consumers use numeric release identifier `391939637` until publication.

This preparation authorizes no dispatch, policy activation, upload, deletion, release publication, waiver, or post-merge execution.

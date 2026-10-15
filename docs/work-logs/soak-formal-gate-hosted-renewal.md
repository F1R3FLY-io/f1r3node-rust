# Formal gate hosted verification and bounded renewal

## Scope and authority

The user requested fresh hosted verification and Claim001/documentation renewal. Protection-rule changes remain separately authorized.

The user then confirmed publication: `it is pushed. Procced with next steps`.

This work starts from `191e184be556c1f190748143377ab369586c53b6`. It does not commit, push, dispatch a workflow, change a protection rule, or execute a node campaign.

## Hosted execution

[Run 35473280388](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/35473280388), attempt 1, completed successfully for the published PR head.

The TLA job, Rocq job, isolated driver fixture job, and `Formal verification gate` all succeeded. The gate belongs to the existing GitHub Actions workflow.

| Identity | Value |
| --- | --- |
| PR head | `191e184be556c1f190748143377ab369586c53b6` |
| Tested and workflow commit | `c6db95ceb950ad8ebfcd31c0411964b71ff31387` |
| Tested first parent | `4ce87b0bb751653a750049934f91318667a265c8` |
| PR API base | `6940a5beb4aa806d3d75f6df3be9f238512fcc2f` |
| Workflow reference | `F1R3FLY-io/f1r3node-rust/.github/workflows/slashing-tests.yml@refs/pull/436/merge` |

The tested commit is a synthetic merge commit. Its first parent is two commits ahead of the PR API base, with no divergence.

The review preserves that distinction. It does not relabel the run as execution directly at the PR head or API base.

## Independent verification

Both producer receipts match independently constructed canonical JSON, byte for byte. Each input record matches its receipt before the success field is added.

The review checked 572 source identities against local Git content and the GitHub tree for the tested commit. The tree response was complete.

Checkout logs independently identify the tested commit in all four jobs. The reviewed source bytes match the published branch snapshot.

Four artifact ZIP downloads match their GitHub SHA256 digests. Their run, repository, branch, and head identities match the expected run.

The driver evidence archive also passes its internal checksum inventory. All 314 binding source hashes match the published branch snapshot.

No downloaded binary was executed. A locally built binding auditor independently checked 48 registered cases, 91 invocations, and their expected exits.

The isolated fixture job passed 29 tests. Its unprivileged host-control suite intentionally skipped one root-only test.

The previous accepted root-only execution remains historical evidence for unchanged sources. This hosted job does not supply a new root-only result.

The lifecycle model passed one completed clean search and ten exact named counterexamples. The clean search generated 66,208 states and found 43,424 distinct states.

Each negative model control returned exit 12 with its expected invariant and trace. Configuration, model, log, and TLC identities match the retained report.

The new formal-gate fixture suite passed all 47 refusal controls on hosted CI. The gate then accepted the two actual producer receipts.

## Renewal

The [Claim001 renewal report](../casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/report.json) records the verified source and execution identities.

Only `slashing-tests.yml` changed among the 39 accepted Claim001 artifacts. Runtime sources, model bounds, fixture assertions, and the artifact inventory remain unchanged.

The user-authorized renewal restores Claim001 and updates its 39 canonical ledgers. The previous records remain byte-preserved in the linked metadata archive.

A fresh post-renewal isolated Linux run also passes 29 tests, 48 cases, and 91 invocations. It checks the renewed claim records inside the copied fixture environment.

The [documentation report](../casper/cbc-evidence/runs/casper-formal-gate-documentation-20260919-01/report.json) renews the two separate formal-area documentation records.

The plan changes only its lifecycle evidence and documentation-review references. The README distinguishes the new review from historical reports and pending governance enforcement.

Fresh local lifecycle checks bind the revised plan metadata. All eleven model controls pass, and eight documentation mutations fail with exact exit 1.

The hosted model report retains its original plan digest. The local report identifies the revised plan, rather than rewriting the hosted execution identity.

All eight executable claims pass the strict source-bound audit. Their soaks remain pending, live qualification remains separate, and post-merge work remains blocked.

## Retained unsuccessful checks

The first independent verifier expected an explicit checkout `ref` input in all four jobs. The unchanged driver job uses the action default.

That verifier exited 1. Its trace remains retained, and the corrected verifier still checks the exact tested checkout identity in every job.

An initial ledger selection found 41 records instead of 39 and refused before edits. Exact artifact selection excluded two historical Python records without deleting them.

The metadata archive retains all 41 selected historical records. Only the 39 current artifact records receive renewal edits.

The earlier implementation timeout remains in its original evidence package. The current hosted attempt has no failed required formal job.

## Remaining governance requirements

This section records the renewal checkpoint. The later [authorization record](soak-formal-gate-authorization.md) approves conditional protection changes, enforcement tests, and evidence-backed final acceptance.

`CLAIM-SOAK-GATE-001` remains pending. The protected `dev` workflow does not yet contain `Formal verification gate`, and the effective rules do not require it.

1. Make the reviewed gate available on the protected baseline through an authorized Git workflow.
2. Obtain separate authorization for the precise protection-rule update.
3. Preserve every existing required check and its integration identity.
4. Require the verified gate from the expected GitHub Actions integration.
5. Verify rejection of missing, stale, skipped, canceled, and failing results under the effective configuration.
6. Record current enforcement evidence and obtain final acceptance.

No protection rule changed. Local refusal controls and a passing hosted run do not establish repository enforcement.

Finalized-floor verification remains local-only. Its proof sources and local verification requirements remain unchanged.

TASK-017-13 remains in progress. Baseline results, scan and concurrency evidence, TASK-018 owners, and handoff acceptance remain separate requirements.

## Retention

Local execution files and raw API responses remain under `target/formal-gate-hosted-35473280388/`. The private retention archive preserves logs, downloads, unsuccessful checks, and isolated execution evidence.

Public metadata excludes raw runner logs and downloaded binaries. No release asset was uploaded or replaced.

Automated STE checks do not constitute human STE review. Temporary diagnostic unavailability is not evidence of a clean result.

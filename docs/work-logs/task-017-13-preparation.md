# TASK-017-13 preparation

## Status

TASK-017-13 is in progress. This session owns preparation of the evidence review and post-merge handoff.

Closure remains blocked. TASK-017-12 still owns baseline execution, and this review found additional artifact-gate and handoff gaps.

The reviewed commit is `3aa79d0c1c91988be781c8380d8f6952b7d72868`. The checkout was clean before preparation.

The [draft handoff](../handoffs/casper-pre-merge-to-post-merge-20260919.md) records obligations, identities, assumptions, and missing inputs. It is not an accepted handoff.

## Review method

The review queried PR #436 and PR #216 through the GitHub CLI. It did not change either PR.

PR #436 now targets `dev` at `6940a5beb4aa806d3d75f6df3be9f238512fcc2f`. That revision is also the local merge base.

The review enumerated the actual three-dot PR diff and checked `cbc` attributes for every changed path. It did not reuse the historical stack-parent scope.

The review then ran the strict eight-claim audit and both directory variants of the artifact gate. No verifier, fixture, or node execution occurred.

The [review package](../casper/cbc-evidence/runs/casper-pre-merge-review-20260919-01/report.json) retains complete gate results, source hashes, specification hashes, and accepted report identities.

## Results

| Check | Result |
| --- | --- |
| PR diff | 1,609 files, 259,106 insertions, 643 deletions. |
| Changed mandatory artifacts | 123 artifacts. |
| Eight-claim strict audit | Exit 0. All eight bounded claims are discharged, with soak status pending. |
| Declared source inventory | 123 artifacts. It overlaps 121 changed mandatory artifacts. |
| Default artifact gate | Exit 4. Seven pending records remain. |
| Canonical-directory diagnostic | Exit 4. Two pending records remain. |
| Source manifest | 125 current artifact hashes match. |
| Evidence manifest | 142 input hashes match. Six unique accepted reports match their ledger hashes. |
| PR #216 gate | Open, with no merge commit. |

The two pending canonical records cover the harness README and verification plan. Neither artifact appears in the eight exact claim inventories.

The five remaining default-only gaps come from independent historical compatibility records. Canonical status does not silently replace their history.

The Claim001-only compatibility result of six gaps remains valid for that earlier scope. This PR diff excludes its unchanged summary writer.

The initial set comparison used inconsistent locale settings. The corrected comparison uses `LC_ALL=C` for both sorting and comparison.

Both initial outputs remain in local scratch. Only the corrected sets determine the scope results above.

## Acceptance review

| TASK-017-13 criterion | State | Remaining requirement |
| --- | --- | --- |
| Current evidence for every changed mandatory artifact | Blocked | Review and resolve the two pending canonical records. |
| Strict discharge for the actual changed scope | Blocked | Resolve current evidence and ledger routing without overwriting history. |
| Exact claim IDs, digests, phase, and tiers | Passed for the eight declared inventories | Repeat after any source or specification change. |
| Scan and concurrency baseline evidence with named reruns | Partial | TASK-018-5 names the reruns. Baseline evidence is not supplied. |
| Complete handoff identities and owners | Partial | Add TASK-017-12 execution fields and confirm TASK-018 implementers. |
| Separate models, fixtures, and product observations | Recorded | Preserve the distinction in final results. |
| No required claim deferred for closure | Preserved | Keep the task open while required evidence remains absent. |

The review does not discharge the two pending artifacts. It does not expand Claim001 or change accepted source inventories.

The review preserves all ledger records, claim specifications, workflows, artifact tags, and completion helpers. It changes only the TASK-017-13 tracker block and preparation documents.

## Reproduction

Run these read-only checks from the reviewed checkout. Use a fresh output directory for the claim audit.

```bash
git diff --name-only 6940a5beb4aa806d3d75f6df3be9f238512fcc2f...3aa79d0c1c91988be781c8380d8f6952b7d72868
git check-attr --stdin cbc cbc-weight < changed-paths.txt
target/release/check-casper-claims --root . --output fresh-review/claims.json --strict
bash "$SA_GITLAB_PROFILE/AItools/agent-resources/skills/cbc/scripts/cbc.sh" \
  discharge --files "$(tr '\n' ' ' < mandatory-paths.txt)" --strict --json
CBC_EVIDENCE_DIR=docs/casper/cbc-evidence \
  bash "$SA_GITLAB_PROFILE/AItools/agent-resources/skills/cbc/scripts/cbc.sh" \
  discharge --files "$(tr '\n' ' ' < mandatory-paths.txt)" --strict --json
```

`changed-paths.txt` contains the first command's output. `mandatory-paths.txt` contains exactly the paths whose `cbc` attribute equals `mandatory`.

The report records both non-passing artifact gates. A canonical-directory override is diagnostic, not a waiver or a routing repair.

## Handoff and next steps

1. Obtain TASK-017-12 results without duplicating its Linux execution work.
2. Review the two pending artifacts against current sources and obtain the required evidence decision.
3. Resolve compatibility routing while preserving independent historical records.
4. Obtain scan benchmark and concurrency baseline evidence.
5. Confirm TASK-018 implementers and complete the missing handoff fields.
6. Repeat the full changed-scope gates and request handoff acceptance.
7. Use the strict completion procedure only after every acceptance criterion passes.

Local scratch is `target/task-017-13-review-20260919-01/`. TASK-017-14 must retain required evidence before cleanup.

Another session committed the tracker and review package as `ec226588a` during preparation. The handoff and work log were still untracked at that checkpoint.

This session did not commit, push, dispatch, upload, publish a release, or complete the task.

## Preparation validation

Source and evidence hashes match. The strict eight-claim audit still passes, while both full-scope artifact gates retain their recorded failures.

The STE Check passes for the tracker against its pre-change baseline. Both new documents pass without a baseline.

An initial combined STE check refused the new documents because the legacy baseline did not cover them. Separate checks preserve full coverage without exemptions.

Active diagnostics checked the tracker, both new documents, and the report. The only finding was a spelling warning inside an unchanged historical Git hash.

That finding is a false positive. Automated language checks do not establish full ASD-STE100 conformance or a human STE Review.

## Canonical record resolution

The user requested: `proceed with resolving the canonical records`.

The [formal-area report](../casper/cbc-evidence/runs/casper-formal-area-records-20260919-01/report.json) resolves both documentation records at base `1f749aa831f54f2c5b3a7c79be27581c89e55f46`.

The README and plan still described completed lifecycle and profile bindings as pending. Their metadata now reflects the accepted source-bound evidence.

The README states `CLAIM-CASPER-SOAK-FORMAL-AREA-DOCS` for these two artifacts. This documentation contract is separate from the eight executable harness claims.

The review preserves executable model inputs, controls, bounds, assumptions, registration policy, and all 123 declared executable artifacts. It changes no claim specification or tag.

The plan links each implemented profile plan and its accepted report. Soaks and live adapter qualification remain pending, and post-merge verification remains blocked.

The previous document and ledger bytes remain in Git at the recorded base revision. The report records their paths and exact hashes.

### Verification

| Check | Result |
| --- | --- |
| Fresh lifecycle controls | Eleven expected verdicts pass. The clean run visits 66,208 generated and 43,424 distinct states. |
| Negative lifecycle controls | Ten exact exit-12 named violations pass. |
| Documentation consistency | The positive check passes. Eight deliberate metadata defects return exact exit 1. |
| Source-bound executable claims | All eight audits pass. Every soak status remains pending. |
| Canonical changed-scope gate | All 123 mandatory artifacts pass, with zero gaps. |
| Default changed-scope gate | Five historical compatibility gaps remain, with exit 4. |

The fresh model run binds the updated plan digest. It does not relabel earlier execution or replace earlier accepted runtime evidence.

The first missing-file refusal check returned exit 2 instead of expected exit 1. A compound shell check allowed the verifier to reach `jq`.

The corrected verifier checks each required file separately. All eight controls then returned exact exit 1, without changing the expected outcomes.

The initial failed attempt remains retained. An ambiguous text edit also required a targeted retry before verification, with no executable change.

### Remaining scope and retention

The two compatibility paths already link to the canonical records. Their results change through those existing links, not through overwritten historical records.

The five independent default records remain untouched. Compatibility routing remains a separate blocker, as do baseline results and final handoff acceptance.

Local verification evidence is under `target/task-017-13-canonical-20260919-01/`. Its private retention archive preserves model logs, check scripts, and failed attempts.

TASK-017-14 must retain that archive before cleanup. This review performs no upload or deletion.

TASK-017-13 remains in progress. This session creates no commit and does not dispatch nodes or authorize post-merge work.

## Compatibility routing resolution

The user requested step 2 after committing the canonical review as `f8623a54e`. The [routing report](../casper/cbc-evidence/runs/casper-compatibility-routing-20260919-01/report.json) records this review.

Four default records are stale snapshots of the Casper harness claims. Their current canonical records bind accepted sources and passing bounded evidence.

Each historical file now has a byte-identical sibling with the suffix `.historical-f8623a54e.md`. The original lookup path becomes a relative symlink to the canonical record.

Keeping the copies in the same directory preserves their relative references. Their pending statuses, partial results, and unsuccessful outcomes remain unchanged.

The review changes no canonical record, executable source, claim specification, artifact tag, or shared tool. It leaves the unchanged summary-writer record outside the changed scope.

### Remaining independent claim

The fifth record covers `.github/workflows/slashing-tests.yml` under `CLAIM-SOAK-GATE-001`. It is not another stale Casper claim snapshot.

The historical specification at `2388a8eedf33d07018f0630bced51a6e054ba439` includes required-check enforcement, workflow-control identity, and a separate Rocq coverage gap.

The [prerequisite record](../cbc-evidence/prerequisites/pr432-formal-gate.md) describes retirement with the digest inventory. It also retains open enforcement obligations and a pending status.

The current Casper discharge cannot settle those different obligations. Neither record supplies a blanket waiver or an unambiguous decision to exclude every remaining obligation.

This review preserves the fifth lookup path and its bytes. It does not change repository rulesets or acquire node-proof work.

A maintainer must resolve applicability and review current enforcement evidence. The historical settings observations do not establish the current repository settings.

### Checks

| Check | Result |
| --- | --- |
| Historical preservation | All four copies match the exact bytes at the base revision. |
| Current routing | All four relative links select the expected canonical files. |
| Lookup fixtures | Sixteen checks pass across current, pending, refuted, and missing canonical targets. |
| Protected inputs | All canonical records, claim specifications, tags, and independent prerequisite records remain unchanged. |
| Eight executable claims | The strict audit passes, with all soak fields pending. |
| Canonical changed scope | All 123 mandatory artifacts pass. |
| Default changed scope | One independent claim remains pending, with exit 4. |

Local verification evidence is under `target/task-017-13-routing-20260919-01/`. The report and hash lists preserve the routing identities.

Step 2 is partially complete. The remaining gate failure is an explicit governance decision, not a reason to substitute a different claim.

TASK-017-13 remains open. This session performs no commit, push, waiver, dispatch, upload, or release publication.

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

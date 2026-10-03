# TASK-017-13 formal gate and handoff

## Ownership and authority

The user assigned TASK-017-13 to `pi-session-01a0afde-d35c-70a2-b8c9-39aa11cbdfca` on 2026-09-21. The previous owner was `pi-casper-handoff-mac`.

The user assigned TASK-017-12 to another agent. This session does not change its campaign implementation, resource budget, or execution status.

The user requested delivery of the formal gate to `dev`, the approved protection change, and completion of `CLAIM-SOAK-GATE-001`.

The [protection authorization](soak-formal-gate-authorization.md) still requires verified source availability, preservation of existing protections, and actual enforcement evidence before acceptance.

The user also requested baseline review after TASK-017-12 delivers evidence, followed by acceptance from named EPIC-018 owners.

The [earlier preparation](task-017-13-preparation.md) and [draft handoff](../handoffs/casper-pre-merge-to-post-merge-20260919.md) remain historical inputs. The handoff is not accepted.

## Initial checks

The checkout was clean at `2530b83853e946dd32a850d78dbbc9491fc23c3f`. Its branch was `formal/soak-casper-consensus`.

Remote `dev` remains at `6940a5beb4aa806d3d75f6df3be9f238512fcc2f`. It does not contain `Formal verification gate`.

PR #436 remains open against `feature/casper-node-observation`. PR #447 remains independently open against `dev` at `799e2136adc6e0100b289945d9a5a6851e81c91f`.

No separate formal-gate pull request appeared in the open pull request inventory. This session does not interpret gate delivery as permission to merge either complete prerequisite branch.

The proposed gate files cannot be copied unchanged to `dev`. The current workflow also invokes the Casper harness, which `dev` does not contain.

The TLA runner also registers Casper models absent from `dev`. A separate gate change must preserve existing verification without importing the unfinished campaign.

PR #216 remains open at `619beb4a4a7ad3f8967d4586daf0f5c552bd150e`. This review does not establish a merge or permit post-merge discharge.

## Protection review

The effective rules endpoint reports three active repository rulesets:

| Ruleset | Identifier | Relevant scope |
| --- | --- | --- |
| `dev-nodelete` | `20148338` | `dev` only. |
| `devProtect` | `15773875` | `dev` only. |
| `masterProtect` | `14299997` | `master`, the default branch, and `dev`. |

The two check rulesets retain strict status-check requirements. Neither requires `Formal verification gate`.

The proposed update will add only `Formal verification gate` with integration `15368` to `devProtect`. It will preserve all other rules and bypass settings.

This update remains conditional on verified gate availability on `dev`. The shared `masterProtect` ruleset must not change for this request.

Repository metadata reports administrator authority for the authenticated account. The classic protection endpoint returned HTTP 403: `Resource not accessible by personal access token`.

That response does not prove that classic protection is absent. Repository role information does not establish sufficient token permissions for the required operations.

No protection rule changed. No enforcement test ran.

## Work sequence

1. Review a gate-only change against the exact `dev` revision.
2. Preserve current workflow coverage and separate Casper harness additions.
3. Run source-bound refusal tests and retain failed attempts.
4. Obtain the specific Git authorization needed to publish the isolated change.
5. Verify the published gate and its dependencies on `dev`.
6. Resolve complete protection visibility before the authorized update.
7. Apply the exact approved change and verify its effective configuration.
8. Test missing, stale, skipped, canceled, failing, and successful results without merging test changes.
9. Complete the claim only after every required evidence check passes.
10. Review delivered baseline, scan benchmark, and concurrency evidence.
11. Obtain acceptance from the named TASK-018 owners.
12. Run the unchanged completion helper and the separate changed-artifact gate before closure.

## Open dependencies

- TASK-017-12 has not delivered baseline results to this session.
- The required scope of the approved 60-hour phase remains unresolved.
- TASK-018-1 through TASK-018-6 still have no named implementers in the tracker.
- The agent network reports no peers. No ownership acknowledgment or recipient acceptance is inferred.
- Complete protection visibility and the required write permissions remain unverified.
- The earlier normal push failed because `cargo-nextest` is unavailable. The user subsequently published with `--no-verify`.

## Current changed-scope audit

A fresh release build of `check-casper-claims` audits the current source. It reports Claim001 pending and claims 002 through 008 discharged.

All eight soak fields remain pending. The audit exits 4 and reports `proof_execution: false`.

The actual PR comparison uses base `799e2136adc6e0100b289945d9a5a6851e81c91f` and head `2530b83853e946dd32a850d78dbbc9491fc23c3f`.

The changed scope contains 149 mandatory artifacts. The default gate reports 29 gaps. The canonical diagnostic reports 28 gaps. Both gates exit 4.

The additional gaps include campaign controls, their model configurations, the crate manifest, and pending reservation records. TASK-017-12 must supply their current claim evidence.

The independent governance claim remains separate. Historical eight-claim success does not describe this checkout or discharge the complete changed scope.

## Gate-only candidate

The candidate patch is `target/task-017-13-intake-2530b8385-ysPHFS/gate-only.patch`. Its source manifest is `gate-only-sources.sha256` in the same directory.

The patch changes five executable artifacts against the exact remote `dev` revision:

- `.github/workflows/slashing-tests.yml`
- `scripts/ci/check-formal-gate.sh`
- `scripts/ci/test-check-formal-gate.sh`
- `scripts/ci/check-tla-invariants.sh`
- `scripts/ci/test-check-tla-invariants.sh`

It adds 296 lines and removes seven lines. It preserves the existing model registry, registered negative controls, workflow jobs, proof sources, and verification limits.

It adds the independent gate and its source-bound receipts. It does not import Casper harness jobs, campaign files, or unavailable model registrations.

The candidate exists only in an ignored fixture directory inside this checkout. It is not a Git worktree, branch, commit, published pull request, or deployed gate.

### Checks

| Check | Result |
| --- | --- |
| Exact baseline application | The five-file patch applies to copied `dev` files and reproduces every candidate byte. |
| Receipt controls | Positive round trips, workflow checks, and 47 exact-exit refusal controls pass. |
| TLA runner fixtures | All 61 registered negative controls classify correctly. Routing and unregistered-control checks pass. |
| Source preservation | The baseline model registry and registered control lists remain byte-identical. |
| Syntax | Bash syntax checks and Actionlint 1.7.12 pass. |
| Hosted proof and enforcement | Neither ran for this candidate. Both remain required. |

The candidate needs complete claim metadata before publication. Historical hosted success cannot replace verification of this separate source revision.

### Retained failures

The first two TLA fixture attempts failed. A retained failure incorrectly reported an existing negative control as unregistered.

The pipeline reproduction observed producer exit 141 and matcher exit 0 on attempt 131. Early `grep` termination caused a false refusal under `pipefail`.

The candidate consumes the complete registry input before returning a match result. Its fixture selection follows the same rule.

The third fixture attempt reached its external 240-second timeout. The fourth attempt passed with an external 600-second limit.

The fourth attempt used the final candidate bytes and retained shell tracing. Its longer external limit does not change workflow or per-model verification limits.

Both failures and the timeout remain separate evidence. The initial attempts did not retain separate source snapshots.

A source archive and manifest bind the successful final attempt. The report does not relabel earlier attempts as executions of that source.

## Publication blockers

The shared checkout remains on the TASK-017-12 branch. This session has not switched branches or created a worktree.

A separate worktree requires explicit authorization under `CLAUDE.md`. The gate-only commit and pull request also need a specific publication decision.

The account can read all three ruleset definitions. The classic protection lookup remains forbidden, so complete protection visibility is still unverified.

Named TASK-018 owners and the required scope of the 60-hour delivery also remain unresolved. No recipient has accepted this handoff.

## Evidence and status

Raw read-only responses, source comparisons, fixture logs, and the candidate patch remain under `target/task-017-13-intake-2530b8385-ysPHFS/`.

The retained classic protection failure remains part of the evidence. Earlier accepted packages and failed attempts remain unchanged.

TASK-017-13 remains in progress. `CLAIM-SOAK-GATE-001` remains pending. This review supplies no campaign or post-merge result.

## Parallel review on 2026-09-28

The user requested TASK-017-13 work in parallel with TASK-017-12. This review updates the existing handoff and preserves all historical evidence.

The checkout remains `formal/soak-casper-consensus` at `211a4e73a6c1c8c4e4d3de35f2debdf1869ab74b`. PR #436 remains open against `feature/casper-node-observation` at `670037c2511abd5f576063b3153681a873244a18`.

### Current checks

| Check | Result |
| --- | --- |
| Committed comparison | 1,917 changed paths and 181 mandatory artifacts. |
| Eight-claim audit | Exit 4. Claim001 remains pending. Claims 002 through 008 remain discharged. All eight soak fields remain pending. |
| Default artifact gate | Exit 4, with 50 pending records. |
| Casper-directory diagnostic | Exit 4, with 39 pending records and 22 missing records. |
| Audited artifact identities | All 181 source hashes match the reviewed commit. |
| Protected baseline | `dev` is `0b9ae5bcec94a2df8f6112bbbc6c950ad2e603b2`. Its slashing workflow lacks `Formal verification gate`. |
| Effective rulesets | No required check names `Formal verification gate`. |
| Classic protection visibility | HTTP 403, `Resource not accessible by personal access token`. |

The claim auditor was rebuilt from the reviewed source. The auditor checks ledger consistency and source bindings without executing proofs or accepting claims.

The artifact gates check the same committed comparison. Concurrent TASK-017-12 additions require a new scope calculation and audit before acceptance.

The default gaps include campaign artifacts, shared verification scripts, and node observation records. This review does not assign another agent's node work to TASK-017-13.

The Casper-directory diagnostic cannot replace the default lookup. Twenty-two artifacts lack records in that directory because the comparison includes other evidence domains.

The dev workflow snapshot has SHA-256 `be6741fcad7ac67437bb43ccc247d433e1f32f5ad73109d615564843f4f45207`. Gate availability and complete protection visibility remain prerequisites for the approved enforcement change.

The September 21 gate-only patch is unavailable at its recorded local path. That historical result does not establish a current, reviewable candidate.

### Handoff corrections

The handoff now records the September 22 stability budget. One 64 GB runner per architecture may operate for at most 64 hours.

Both full 24-hour baselines must pass before either 60-hour workload. The record still requires a closure decision for the 60-hour phase.

Publication and recovery qualification belong to EPIC-018 after PR #216 integration. The pre-merge baseline requires authority/finality qualification and preserves pending verdicts for deferred profiles.

The handoff separates confirmed TASK-018 implementers from acceptance of the completed handoff. Either `@jeffrey-l-turner` or `@jltatbeach` may own each follow-on task.

The handoff also labels accepted source inventories and previous gate results as historical. Current verification must bind the final delivered revision.

### Remaining work

1. Prepare the independent gate against current `dev` and preserve existing verification coverage.
2. Obtain specific Git publication authorization after the candidate and its checks are ready for review.
3. Verify hosted execution and gate availability on `dev`.
4. Resolve protection visibility before the approved rule change.
5. Retain positive and negative enforcement results before governance claim acceptance.
6. Review current campaign claims, qualification records, preflight results, and both full baselines from TASK-017-12.
7. Review scan benchmark and User Contract Concurrency baseline evidence.
8. Record the 60-hour closure scope and obtain handoff acceptance.
9. Repeat final source, claim, changed-artifact, and task-completion checks.

### Retention and limits

Local review outputs remain under `target/task-017-13-parallel-20260928/`. These outputs include the audit results, source snapshot, workflow snapshot, and effective rules.

The review changed only this log and the handoff. It made no branch, Git index, workflow, claim, protection, or deployment change.

TASK-017-13 remains in progress. TASK-017-14 reduction and TASK-017-15 publication retain their recorded prerequisites.

The STE Check passes against the pre-edit baseline. Local Markdown targets and `git diff --check` pass. No human STE Review is claimed.

## Ownership and scope decisions on 2026-09-28

<!-- claude-session-f3cbc961 -->

The maintainer assigned TASK-017-13 to `claude-session-f3cbc961` on 2026-09-28. The previous owner was `pi-session-01a0afde-d35c-70a2-b8c9-39aa11cbdfca`.

TASK-017-12 continues in a different session in the same checkout. The two tasks commit together after the two sessions complete their work.

### Independent confirmation

This session ran the same checks at `211a4e73a6c1c8c4e4d3de35f2debdf1869ab74b` before it read the parallel review above. The two sets of counts are equal.

The [review report](../casper/cbc-evidence/runs/casper-pre-merge-review-20260928-01/report.json) records the results. All 181 source digests match the bytes at the reviewed commit.

### Gap owners

| Group | Default gaps | Owner | Necessary action |
| --- | --- | --- | --- |
| Campaign scripts, supervisor, models, and the soak workflow | 25 | TASK-017-12 | Current claim evidence and acceptance. |
| Campaign control code in the soak crate | 13 | TASK-017-12 | Current claim evidence and acceptance. |
| Node observation files | 10 | EPIC-019 | Named maintainer acceptance of the cycle 03 package. |
| Formal gate workflow and TLA runner | 2 | TASK-017-13 | Gate on `dev`, the protection change, and enforcement results. |

The 10 node observation records have current digests. Their claim is `CLAIM-CASPER-NODE-OBSERVATION-002`, and their evidence is the package `casper-node-claim-gate-38e576041-01`.

The named maintainer accepted cycle 02. No person accepted cycle 03, which added the B11 files. More verification does not close these gaps.

The record for `.github/workflows/merge-recovery-soak.yml` has an old digest. The system-integration pin changed after the record, through the node branch merge of 2026-09-23.

### The 60-hour phase

The maintainer decided the scope on 2026-09-28. The 60-hour phase is not a closure requirement for this branch or for PR #436.

The phase cannot run before the branch is merged. TASK-018-7 owns the run and its record after the changes are in `master`.

This decision replaces item 8 of the remaining work above. Handoff acceptance stays a requirement.

The maintainer then requested a record on GitHub. This session created [issue 473](https://github.com/F1R3FLY-io/f1r3node-rust/issues/473) and the label `post-merge-obligation`.

The maintainer then decided that one issue records all post-merge obligations of the stack. Issue 473 now lists eight obligations, and the 60-hour phase is obligation O1.

The maintainer confirmed the stage `campaign-stability-60h` for obligation O1. The issue closes only when each obligation has passing evidence or a recorded waiver.

The tracker maps each obligation to its task in the `post_merge_obligations` field of EPIC-018.

The automatic update of the issue is planned work. The maintainer decided that its branch, `ci/soak-obligation-gate`, goes on top of the stack, from `feature/randomized-exercise-soak`. This session created no workflow and no branch.

The maintainer stated the purpose of that branch. It accumulates all obligations that must be recorded and discharged after the stack merges into `master`. The 60-hour phase is the first recorded obligation.

### Gate-only candidate against the current `dev`

The candidate of 2026-09-21 is not available, and `dev` moved. This session built a new candidate against `dev` at `0b9ae5bcec94a2df8f6112bbbc6c950ad2e603b2`.

The candidate is under `target/task-017-13-gate-candidate-20260928-01/`. Its patch has the SHA-256 prefix `ca9ac2cb3539b4a1`. It is not a branch, a commit, or a pull request.

| File | Added | Removed |
| --- | --- | --- |
| `.github/workflows/slashing-tests.yml` | 55 | 0 |
| `scripts/ci/check-formal-gate.sh` | 63 | 0 |
| `scripts/ci/test-check-formal-gate.sh` | 115 | 0 |
| `scripts/ci/check-tla-invariants.sh` | 18 | 5 |
| `scripts/ci/test-check-tla-invariants.sh` | 47 | 3 |

| Check | Result |
| --- | --- |
| Patch application to the `dev` bytes | Passed. The result is equal to each candidate file. A second, independent application gave the same result. |
| Gate fixture suite | Passed, with 47 refusal controls. |
| TLA runner fixture suite | Passed, with 61 negative controls. |
| Control run with the `dev` runner | Failed as expected. The `dev` runner accepts a result with no trace. |
| Model registry and negative control lists | Byte-identical to `dev`. |
| Action pins and cache guards | No unpinned action and no invalid guard. |
| Bash syntax | Passed for the four scripts. |
| Actionlint and ShellCheck | Not run. The tools are not installed on this machine. |
| Repository supply-chain test | Not run on the candidate. Its two workflow rules were applied with a separate check. |
| TLC and Rocq execution | Not run. The fixtures replace TLC. |
| Hosted execution and enforcement | Not run. Both remain required. |

The gate inventory names only paths that `dev` has. Four paths of the branch inventory are absent: the Casper model check, the Casper soak script, the soak crate, and the node observation proofs.

The candidate carries the `pipefail` corrections and the TLC output checks. It carries no Casper or node observation registration, and `check-formal-invariants.sh` stays at the `dev` bytes.

#### Decisions for the maintainer

1. The TLC output checks change the verdict rule for all `dev` models. A smaller candidate with only the `pipefail` corrections is possible.
2. The `dev` gate does not cover node observation proofs, Casper models, driver bindings, or wire correspondence. The branch gate covers them.
3. The TLA test script has an inline `python3` block that `dev` already has. The candidate changes that file and keeps the block.
4. The two new scripts need the file mode `100755`. The patch has no mode data.
5. The soak branch changes the same five files. The stack merge can conflict after the gate is on `dev`.

Publication of the candidate needs a branch and a specific Git authorization. This session requested neither.

#### Maintainer decisions on 2026-09-28

| Decision | Result |
| --- | --- |
| TLC output checks | The candidate keeps all checks. |
| Coverage of the `dev` gate | Accepted for a candidate against `dev`, with a record of the difference. |
| Inline `python3` block | The candidate keeps the block. Its rewrite in bash is a separate follow-up task. |
| Publication | The maintainer creates the branch `ci/formal-verification-gate` from `feature/randomized-exercise-soak`, on top of the stack. |

The publication decision changes the use of this candidate. The top of the stack already has the gate job, the gate script, and the full gate inventory.

The patch for `dev` does not apply to the top of the stack. Its hunks fail or are already present in all five files.

The formal gate thus reaches `dev` when the stack merges, with the full coverage of the branch gate. No gate-only change goes to `dev` before that merge.

The candidate stays a record of the difference between `dev` and the branch gate. The first three decisions apply only if a later decision sends a gate-only change to `dev`.

The ruleset change and the enforcement tests of `CLAIM-SOAK-GATE-001` follow the stack merge. They are obligation O7 in issue 473.

The named maintainer for the handoff acceptance is `@jltatbeach`.

### Concurrent changes

TASK-017-12 added files under `scripts/casper-soak` after this review. The mandatory tag applies to that directory.

The counts in this review describe `211a4e73a` only. The final review must use the delivery revision.

### Retention and limits

Local outputs of this session are under `target/task-017-13-verification-20260928-01/`. The package keeps the report, the validation record, and the digest lists.

The bulk outputs are not in the external evidence store. TASK-017-14 owns their retention.

This session changed the tracker, this log, the handoff, and the new review package. It made no branch, Git index, workflow, claim, protection, or deployment change.

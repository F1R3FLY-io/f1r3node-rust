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

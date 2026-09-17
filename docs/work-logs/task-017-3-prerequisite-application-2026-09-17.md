---
task: TASK-017-3
branch: formal/soak-casper-consensus
claimed_by: pi-casper-harness
handoff_status: in_progress
execution_scope: authorized-working-tree-application
---

# TASK-017-3 Prerequisite Application

The user authorized working-tree application of the reviewed prerequisites and completion of the candidate matrix. This assistant has no authorization for staging, commits, pushes, or merges.

The other agent committed the prerequisite application as `f29c59d010a32b523246bf2c980a48dd7f49d152` during validation. This assistant preserved that commit.

The starting revision is `2c1ce13d0cf291d3b80ca40f3c8d1b7f6b4b9d12`. The starting working tree and index were clean.

The [review log](task-017-3-prerequisite-review-2026-09-17.md) retains the source review, fixture results, inherited evidence audit, and unresolved limits.

## Application order

| PR | Selected revision | State |
| --- | --- | --- |
| 430 | `dedb3add172098efcbc63d72b2ebdc612f2fddcd` | Applied first |
| 431 | `0e176e486a028d10704b96add6e5eb50525682cb` | Applied second, with preservation edits |
| 432 | `e0380392bcc66d9774e403edb8a08415798c1e0e` | Applied third, with evidence preservation |
| 433 | `65f7f6daa832c0acb6fddf2b462db1b9d5461729` | Applied fourth |

## Preservation requirements

Preserve the Casper contracts, claims, models, evidence records, compatibility links, and unrelated task states. Keep imported prerequisite claims separate from Casper claims.

Do not infer upstream merge status from local file application. PR #216 remains an optional candidate, not the default authority.

B44 containment and current profile-binding limits remain open. Imported bounded models do not establish node correctness.

## Applied verification

The [evidence report](../casper/cbc-evidence/runs/casper-prerequisite-application-20260917-01/report.json) retains 407 artifacts and a manifest of 238 source digests.

| Check | Result |
| --- | --- |
| Isolated disk fixture | All 42 scenarios passed |
| Shared TLC tier | All 13 positives completed and all 61 negatives returned the expected violation |
| Trace audit | Every negative included its exact invariant and a counterexample trace |
| Gate fixture | All 61 acceptance controls, 18 rejection cases, routing checks, and registration checks passed |
| Driver fixture | All three scenarios passed |
| Summary fixture | Five data cases and the keyword scan passed |
| Casper model | The clean configuration and ten negative controls passed their required checks |
| Casper runner | All 12 tests passed |
| Static checks | Shell syntax passed for 29 files. Three workflow files parsed. Linux-targeted Pyright passed for four files |
| Strict EPIC-017 CbC gate | Exit 4. Full claim evidence remains pending |

The shared TLC tier took 67 seconds inside a two-CPU container with a 384 MB Java heap. This measurement is not a hosted-workflow budget guarantee.

The workflow permits 15 minutes for PR/push jobs and 240 minutes for scheduled/manual jobs. The PR tier permits two minutes per configuration and uses two workers.

The timeout sends TERM, then permits a 60-second KILL grace. The suite does not have a two-minute total bound.

The shared gate still accepts positives by exit status and does not require negative traces. The separate transcript audit checked stronger criteria for this run.

The first TLC attempt failed because copied directories belonged to root. All 74 configurations reported tool errors, not successful verification.

The retry used an unprivileged writable copy of the same sources. Both attempts remain in the evidence package.

An initial fixture-image build also failed. Its retry used the pinned Debian manifest, and both outcomes remain explicit.

All process fixtures ran without host mounts, network access, or a Docker socket. Existing host TLC logs remained untouched.

Linux-only native fixtures received static checks, not execution. Darwin diagnostics for Linux `pidfd` APIs do not establish Linux type errors.

The diagnostic review classified nine platform-API findings as false positives. Six parser findings miss the entrypoint exception handler, which returns setup-error exit 2.

The Boolean identity check deliberately requires JSON `false`. No suppression comment or implementation change was necessary.

## Evidence preservation

The original shared driver and gate records retain their pending Casper claims. Imported prerequisite records remain separate:

- [Inherited driver evidence](../cbc-evidence/prerequisites/pr431-soak-driver.md).
- [Inherited gate evidence](../cbc-evidence/prerequisites/pr432-formal-gate.md).

Casper glossary terms, model controls, contracts, claim records, and compatibility links remain intact. No Rust source, external suite pin, or unrelated task state changed.

Historical records do not inherit verification from these new fixture results. No node ran, and no full claim received discharge.

Retained logs redact local paths. Five shell dumps omit trailing spaces, with original and retained digests recorded separately.

Artifact hashes, source hashes, fixture input hashes, matrix identities, JSON parsing, compatibility links, and scope-preservation checks passed. The deterministic STE Check passed.

## Candidate artifacts

The [candidate matrix](../casper/design/soak-candidate-matrix.jsonc) records current `dev` at `a2fe60c7255bf4ba035d41fb65b6d6f1c0f02632` for AMD64 and ARM64.

GitHub Actions run `35114194633`, attempt 1, supplies both image archives. The run used a push to `dev` at that exact revision.

Archive digests matched GitHub metadata. Every image blob matched its digest, and each platform manifest identified the expected architecture.

The matrix records platform-manifest digests, image-configuration digests, and the final `opt/docker/bin/node` digest. No mutable image tag supplies identity.

This provenance uses GitHub run metadata and workflow source. Artifact attestations were not verified. Downloaded archives remain under `target/task-017-3-validation/`.

The latest canary release selected a different source revision. Its artifacts were not substituted for current `dev`.

PR #216 remains an unselected optional reference at `619beb4a4a7ad3f8967d4586daf0f5c552bd150e`. Its open state does not satisfy the post-merge gate.

## Remaining completion requirements

TASK-017-3 remains in progress. The prerequisite application and image/binary identity checks are complete, but executable profile workload configurations remain unpinned.

The matrix records existing configuration-source digests without presenting them as complete workload configurations. Missing workload digests remain null and prevent dispatch.

Profile adapters, required capabilities, resource approval, and full harness verification remain dispatch blockers. B44 remains open.

The seven profile implementations belong to TASK-017-5 through TASK-017-11. Their dependency on TASK-017-3 requires a tracking decision before those tasks can supply final workload configurations.

The strict gate still reports pending claims. This assistant did not waive claims or change the task to complete.

A human STE Review remains necessary.

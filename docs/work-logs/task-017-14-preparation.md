# TASK-017-14 Preparation

---
handoff_status: paused
next_steps:
  - Wait for TASK-017-13. Then run the rehearsed reduction against the live tree and redirect the one dangling work-log link.
  - Consolidate the work logs of TASK-017-1 through TASK-017-7 in the same commit.
  - Publish the draft release when the reduction commit lands, so the tag resolves for readers without write access.
---

## Scope

This log records preparation for the diff reduction. It covers steps 0 through 5 of the implementation plan in `docs/ToDos.md`.

No file leaves the tree during preparation. The reduction commit waits for TASK-017-13.

## Measurement

PR #436 now targets `dev` after PR #433 merged. At `09b0a6006` it shows 1,345 files and about 206,700 added lines.

The evidence tree under `docs/casper/cbc-evidence/runs/` holds 23 packages, 1,049 files, and 185,612 text lines. Seven packages are live because a canonical ledger record cites them. Sixteen are historical.

## Consumers of the evidence tree

The shared CbC driver reads only the ledger record. It does not open the evidence files that a record cites.

The claims audit in `scripts/casper-soak/src/bin/check-casper-claims.rs` opens the cited `report.json` and reads source digests from it. That report must stay in the tree.

The bindings inventory in `scripts/ci/check-casper-soak-bindings.sh` hashes every `runs/*/report.json`. Removing other files changes no gate result.

## Refined keep rule

The plan rule keeps four kinds of file. Nested per-file digest lists add one line per archived file, so they belong inside the bundle.

A package keeps only these top-level files in the tree: `report.json`, `validation.json`, `review-validation.json`, `redactions.tsv`, and `artifacts.sha256`.

Under this rule 56 files, about 14,500 lines, and 1.0 MB stay. 993 files, about 171,000 text lines, and 37.7 MB leave.

## Compatibility symlinks

Nothing in this repository's CI reads `docs/cbc-evidence/`. The shared driver does a flat lookup there for mixed-epic runs and has no module routing.

The 80 symlinks stay until the workspace driver gains module routing. That change belongs to the workspace profile, not this branch.

## External store

The store is a draft GitHub release on this repository, tag `cbc-evidence-epic-017`, target `09b0a6006`. It was created on 2026-09-19 with 24 assets. A draft is visible only to people with write access, and its tag resolves only after publication.

Each package has one bundle, `<package>.external.tar.gz`, that contains every file the keep rule excludes plus an inner `external-manifest.json` with each member's path, size, and SHA-256. Bundles are deterministic: sorted members, zero timestamps, root ownership.

The release also carries `SHA256SUMS` for the bundles and `index.json` with package, asset, size, member count, and live flag. A package record will cite the tag, asset name, and bundle SHA-256.

The stack-integration package has no file to externalize and no bundle.

## Rehearsal (2026-09-19)

The reduction ran against an exported copy of `442e93faa` in the session scratchpad. No live file changed.

| Measure | Before | After |
| --- | --- | --- |
| Evidence files under `runs/` | 1,049 | 78 |
| Evidence text lines under `runs/` | 185,612 | 19,777 |
| Strict audit, claims 001 to 004 | exit 0 | exit 0 |
| Strict audit, full bundle | exit 4 | exit 4 |

The tool removed 993 files, wrote 22 `external.json` pointers, and rewrote 39 `previous_ledger` references. Each rewritten reference keeps the inner-file digest and adds the release tag, asset name, asset digest, member path, and pointer path.

The offline link check reports one more error than the live tree. One work log links to a removed fixture result. Step 3 redirects that link to the package pointer. Code-span mentions of removed files are not links and stay as history.

`report.json` stays byte-identical in every package because the audit binds its digest. The external pointer is a sibling file, not a report field. The retention rule in the plan document should say so for existing packages.

## Packages added after the rehearsal

TASK-017-8 closed on 2026-09-19 with two packages. The review package `casper-merge-accounting-20260919-01` is 3.8 MB with archives in-tree. The acceptance package `casper-merge-accounting-acceptance-20260919-01` holds the report, the validation, and digest lists only.

Both enter the retention inventory. The bundle set and the store assets are rebuilt from the tree at reduction time. The counts in this log are a snapshot, not the final list.

The handoff from `pi-casper-merge-accounting` asks that the accepted report bytes, source hashes, and previous-ledger references stay unchanged. The rehearsed tool already preserves report bytes and rewrites references in place. Its open question is where the supplemental acceptance checks under `target/` go. The agent exports them as one archive, and the reduction uploads it to the release as a separate asset. Nothing under `target/` enters the tree.

## Status

- Step 0, retention rule: recorded in the plan document on 2026-09-19.
- Step 1, store and bundles: complete. 22 bundles, `SHA256SUMS`, and `index.json` uploaded to the draft release. Download verification: `SHA256SUMS`, `index.json`, and the profile-acceptance bundle match the local digests, and all 22 asset sizes match.
- Step 4, symlinks: reviewed. No change on this branch.
- Steps 2, 3, 6: rehearsed in the scratch copy. Not applied.
- Steps 5, 7: not started.

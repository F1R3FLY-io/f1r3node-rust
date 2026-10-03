# TASK-017-14 Preparation

---
handoff_status: complete
next_steps:
  - None for this task. Step 5, the work-log consolidation, was not needed for the maintainer confirmation.
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

## Reduction on 2026-10-03

`claude-session-aa467dea` took over the task on 2026-10-03 at the request of the user. PR #451 and PR #447 merged, so PR #436 is the bottom of the stack, and no lower branch can conflict with a removal. The upper branches change no file under `runs/`.

The reduction keeps every ledger record and every `report.json` byte-identical. Each package that loses files gets a sibling `external.json`. It names the release tag, the asset, and the asset digest, and it lists the path, size, and SHA-256 of each moved file. Records keep their original paths, so a cited path resolves through the `external.json` of its package.

The keep rule of the rehearsal applies: `report.json`, `validation.json`, `review-validation.json`, `redactions.tsv`, and `artifacts.sha256` stay in each package. `casper-rust-migration-20260917-01/bindings.tar.gz` also stays, because `scripts/ci/check-casper-soak-bindings.sh` and `scripts/casper-soak/tests/bindings.rs` read it.

The existing assets of 22 packages contain every moved file of those packages, byte for byte. The other 23 packages need new deterministic bundles, 8.0 MB in total. Three Markdown links that pointed to moved files now point to the `external.json` of their package.

| Measure | Before | After |
| --- | --- | --- |
| PR #436 diff against `master` | 2,048 files, 307,232 added lines | 970 files, about 102,000 added lines |
| Files under `docs/casper/cbc-evidence/runs/` | 1,287 files, 53 MB | 164 kept files and 45 pointers |
| Claims audit, default and strict | exit 0 and exit 4 | exit 0 and exit 4, same output |
| Bindings inventory | exit 0 | exit 0, same report |
| Offline link check of `docs/` | 0 errors | 0 errors |
| `scripts/casper-soak` tests | not measured | 146 passed, 0 failed |

- Steps 2 and 3: applied to 45 packages. 1,123 files moved to the release.
- Step 4: no change. The compatibility symlinks stay.
- Step 5: not started.
- Step 6: complete for the reduction. The table above records the results.
- Step 7: waits for the bundle upload and the user commit.

## Second reduction on 2026-10-03

The 23 new bundles of the first reduction are uploaded to the draft release. A fresh download of each asset matches the digest in its pointer.

The user then asked for two more cuts. No gate reads `validation.json`, `artifacts.sha256`, `redactions.tsv`, or `review-validation.json`, so 93 such files in 45 packages moved to new `<package>.metadata.tar.gz` assets. Each new asset has an inner `external-manifest.json`. No upload replaced an existing asset.

The pointers now use schema version 2. A pointer names each release asset of its package with the asset digest, the size, and the member count. The member list stays in the inner `external-manifest.json` of each bundle. Two older bundles have no inner manifest, `casper-version-phlo-20260919-01` and `casper-version-phlo-verification-20260919-01`, so their pointers keep the member list.

A package now keeps only `report.json` and its `external.json` in the tree. `casper-rust-migration-20260917-01/bindings.tar.gz` also stays. Three more Markdown links now point to package pointers.

| Measure | After the first reduction | After the second reduction |
| --- | --- | --- |
| PR #436 diff against `master` | 970 files, about 102,000 added lines | 889 files, about 92,200 added lines |
| Files under `docs/casper/cbc-evidence/runs/` | 164 kept files and 45 pointers | 71 kept files and 57 pointers |
| Release assets | 50 | 95 |
| Claims audit, default and strict | exit 0 and exit 4 | exit 0 and exit 4, same output as the baseline |
| Bindings inventory | exit 0 | exit 0 |
| Offline link check of `docs/` | 0 errors | 0 errors |
| `scripts/casper-soak` tests | 146 passed | 146 passed |

## Completion on 2026-10-03

The release `cbc-evidence-epic-017` was published on 2026-10-03 at 16:20 UTC with 95 assets. The tag resolves for readers without write access.

The maintainer `jltatbeach` confirmed the reduced diff of PR #436 at revision `3bdd523cc` in [comment 5971043357](https://github.com/F1R3FLY-io/f1r3node-rust/pull/436#issuecomment-5971043357). GitHub reports 889 files, 92,225 added lines, and 825 removed lines.


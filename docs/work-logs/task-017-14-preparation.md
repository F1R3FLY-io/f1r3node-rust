# TASK-017-14 Preparation

---
handoff_status: paused
next_steps:
  - Create the draft release and upload the bundles, SHA256SUMS, and index.json. Then verify one bundle digest by download.
  - Consolidate the work logs of TASK-017-1 through TASK-017-7.
  - Wait for TASK-017-13 before any file leaves the tree.
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

The store is a draft GitHub release on this repository, tag `cbc-evidence-epic-017`, target `09b0a6006`. A draft is not public and can be deleted. The bundles wait in the session scratchpad until the release exists.

Each package has one bundle, `<package>.external.tar.gz`, that contains every file the keep rule excludes plus an inner `external-manifest.json` with each member's path, size, and SHA-256. Bundles are deterministic: sorted members, zero timestamps, root ownership.

The release also carries `SHA256SUMS` for the bundles and `index.json` with package, asset, size, member count, and live flag. A package record will cite the tag, asset name, and bundle SHA-256.

The stack-integration package has no file to externalize and no bundle.

## Status

- Step 0, retention rule: recorded in the plan document on 2026-09-19.
- Step 1, store and bundles: 22 bundles built and digested locally, 22.0 MB. The draft release does not exist yet. Its creation needs a permission that this session does not hold.
- Step 4, symlinks: reviewed. No change on this branch.
- Steps 2, 3, 5, 6, 7: not started.

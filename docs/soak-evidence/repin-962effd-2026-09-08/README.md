# Merged Harness Repin

## Revision selection

The user confirmed that the system-integration changes reached `dev` and `main`. This repin follows the failed-run inspection.

GitHub reports both PR #139 and PR #138 as merged. The retained API records identify `962effd17708192627bd249362761c0ccb1fd5fa` as their merge revision.

At verification, `main` referenced `962effd17708192627bd249362761c0ccb1fd5fa`. The `dev` branch referenced `70e2e40d2f96f21b1d555de4a33a03a75f0f7656`.

Git ancestry checks confirmed that the selected revision contains:

- The previous pin, `022ae6d304ff47a112d7db76be956736cf056e0b`.
- PR #139's head, `3eb9b5fa2c0477868e50f2cfe283735617163547`.
- The observed `dev` revision, `70e2e40d2f96f21b1d555de4a33a03a75f0f7656`.

The selected revision is a merged `main` revision, not a PR-head pin.

## Changes and checks

The repository helper updated all three pin sites:

- `.github/oci-validation.env`.
- `.github/workflows/_integration-pipeline.yml`.
- `.github/workflows/merge-recovery-soak.yml`.

The helper ran its normal remote check and workflow invariant check. No bypass flags were used for the actual repin.

The merged harness contains finalization-evidence checks, shared polling deadlines, and related test corrections. Its unit tests reject mixed-node outcomes, missing evidence, contradictory verdicts, and aggregate join-grace overruns.

The soak workload file, `metrics.py`, and `resource_monitor.py` are byte-identical across the old and new pins. The 45-second finalization wait remains unchanged.

| Check | Result |
| --- | --- |
| Merged harness unit suite | 313 passed, three dependency warnings |
| Repin helper regression | Passed |
| Workflow invariant check | Passed |
| Collector extension regression | Passed |
| Collector extension on merged harness | Applied, compiled, and idempotent |
| Release workflow regression | Passed |
| Release-gate regression | Passed |
| Formal negative-control classifier fixtures | Passed |
| Bounded formal-routing fixtures | Passed |
| Current claim inventory | Passed |

The first direct unit-suite attempt passed 311 tests and failed two CLI tests because Poetry was missing. This was not behavioral RED evidence.

The repeated suite used Poetry 2.1.3 with the existing Python 3.12.3 environment. All 313 tests then passed. No harness source changed between attempts.

The exported merged harness remained separate from the sibling repository's working files. Git fetch updated remote references but did not change the sibling checkout.

## Evidence and limits

[manifest.json](manifest.json) binds the merge observations, reviewed source files, final pin values, and local check logs. The current inventory also retains the failed-soak incident evidence.

The historical B3 manifest remains unchanged. Its source digests still identify the B3 commit, not these later documentation and pin changes.

These local checks do not constitute a node soak, formal discharge, or complete acceptance. No Rust suite, Rocq suite, or actual TLC model run was repeated for this repin.

D1 through A1 remain open. G0 still requires enforcement confirmation and complete acceptance identity. The exact-candidate 60-hour soak remains required.

No soak was dispatched, canceled, or restarted. No production correction, workload change, claim discharge, commit, or push was made.

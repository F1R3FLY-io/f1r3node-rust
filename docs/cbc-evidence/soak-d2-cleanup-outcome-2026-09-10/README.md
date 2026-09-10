# D2 Cleanup Failure Preservation

## Result

B26 starts from `d64ae3bbf757de6089300191c23cc1796245c60c`. Five production cases expose failed Docker cleanup commands followed by a sufficient disk sample.

The faults affect container listing, container removal, network pruning, image pruning, and builder pruning. Every baseline execution admits one iteration and records no protection failure.

The correction preserves cleanup failures and enables pipeline failure detection. Every corrected fault case refuses admission and records one protection failure.

The exact formal control exits 12 on `CleanupFailurePreventsAdmission`. The corrected configuration exits 0 with 42 distinct states.

Two successful-cleanup cases pass on both baseline and corrected source. Partial reclamation leaves 8000 MiB and prevents admission below the 8192 MiB threshold.

Sufficient reclamation leaves 16384 MiB and permits one iteration. These cases provide characterization coverage, not additional repair cycles.

## Verification

Twenty-three positive configurations, twenty-four exact controls, 168 classifier cases, six routing scenarios, and thirty-eight emergency scenarios pass.

The forty-seven actual TLC logs were retained before the classifier and routing tests ran. Supporting regressions pass, and executable inputs match the verification snapshot.

External commit `14ffb4d3a` contains the tested executable inputs. This session does not attest its commit hooks.

[The manifest](manifest.jsonc) binds the frozen baseline, source snapshots, original raw files, complete logs, and counted path substitutions.

## Limits

The fixtures control external Docker commands and disk samples. They do not start real nodes or prove Docker daemon completion.

The correction preserves command errors, not every possible cleanup failure. Docker absence, probe failures during cleanup, concurrent faults, and lost error publication require separate verification.

The model represents one failing command per execution and a sufficient final sample. It does not prove production pipeline timing or durable publication.

Docker ownership, image preservation, complete shutdown, durability, deadline and reserve bounds, hosted checks, maintainer review, D2, and acceptance remain pending.

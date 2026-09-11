# B37 benchmark crash evidence

B37 corrects restart admission after a driver crash during an active benchmark.
The baseline starts new work without retaining the interrupted benchmark.
The correction records the interruption before launch and preserves one failure across two refused restarts.

## Results

| Check | Result |
| --- | --- |
| Production baseline | Exit 1 with the exact restart-admission failure. |
| Formal control | Exit 12 for `BenchmarkCrashRequiresRefusal`. |
| Corrected production | Exit 0 with zero iterations, one benchmark, and one failure in each failure counter. |
| Corrected model | Exit 0 with four distinct states. |
| Real-system regressions | All ten GREEN cases pass, including B35 and B27–B31. |
| Composed formal gate | 32 positive configurations and 34 exact controls pass. |
| Classifier and routing | 238 classifier cases and six routing scenarios pass. |
| Emergency and supporting regressions | All 41 emergency cases and supporting checks pass. |

The [manifest](manifest.jsonc) binds source snapshots, original streams, published streams, substitutions, tools, and raw evidence digests.
The package retains complete selected streams, not only success messages.
The evidence audit requires an actual work-invocation record for RED.
A fixture deadline alone does not qualify.
The source archive records 217 verification inputs that still match the current source.
These snapshots identify bytes, not execution coverage for every captured file.

The diagnostic archive contains 376 unique regular files and 2,092,376 bytes.
Archive validation preceded extraction, and every extracted digest matched.
The diagnostic virtual machine was observed terminated after retrieval.
All nine diagnostic virtual machines used so far have termination observations.

## Source identity

The production baseline starts from `69332f716`.
External commit `64b02472c` contains the tested correction and formal registration.
This session does not attest the external commit hooks.
The bootstrap archive retains the earlier `980285d4c` snapshot.
Separate runtime payloads identify the actual baseline and corrected driver bytes.
The bootstrap archive is not the current documentation snapshot.

## Limits

The [correspondence document](../../../formal/tlaplus/soak_disk/BenchmarkCrashRecovery.md) defines the selected contract and its assumptions.
The fixture uses a restricted Docker writer, not a node workload.
The original writer remains running after the crash and both refused restarts.
Fixture cleanup does not establish production shutdown.

The test assumes surviving, uncorrupted storage.
It does not establish power-loss durability, successful synchronization, storage-full handling, durable upload, or complete crash coverage.
Creation ownership, aggregate deadlines, all-writer reserve bounds, hosted verification, and maintainer review remain open.
D2, claim discharge, and acceptance remain pending.
No assistant commit, push, hosted dispatch, or soak occurred in this cycle.

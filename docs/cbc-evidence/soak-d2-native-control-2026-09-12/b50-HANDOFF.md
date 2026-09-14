# Native control-directory refusal handoff

## Result

The selected control-directory refusal behavior has matched production and formal RED/GREEN evidence.
The fixture remains byte-identical across the matched pair.
The corrected helper also passes both existing native regressions with two driver snapshots.
This result does not complete B44 or D2.

The baseline is commit `f75af9ed3c5b4fbab049328326642f4484dbffb3`.
The corrected helper is `scripts/bench/soak-containment.py`, SHA-256 `2641d0ac90ee8d837e7849038a3ef1f8f66c64fd4a035f5f823a6c1f90287ea7`.
The new fixture is `scripts/bench/test-soak-native-control-path.py`, SHA-256 `d9a811b0bbfa4d1ca0b97e2218e0fc03aaab6598a4abb89ef7c002ea809f0fec`.
The integrated B48 driver digest is `94b180d0c4cb6b7607284ff111e24512af8109d689e20c674fe15b717a3b8680`.

## Runtime cases

| Stage | Source snapshot | Exit | Observation |
| --- | --- | ---: | --- |
| `red` | `red-source` | 1 | The workload initialization hook executes as UID 65534 below an untrusted control ancestor. |
| `green` | `green-source` | 0 | The launcher refuses before workload execution or control-directory creation. |
| `query-regression` | `green-source` | 0 | An unavailable manager query prevents admission. |
| `native-regression` | `green-source` | 0 | Native controller-loss handling and two normal refused restarts pass. |
| `integrated-green` | `integrated-source` | 0 | The control-path refusal passes with the B48 driver snapshot. |
| `integrated-query-regression` | `integrated-source` | 0 | The query refusal passes with the B48 driver snapshot. |
| `integrated-native-regression` | `integrated-source` | 0 | Native controller-loss handling and normal recovery pass with the B48 driver snapshot. |

The baseline driver subsequently refuses its run-domain record.
That refusal does not undo the earlier workload initialization hook.
The RED therefore establishes premature workload initialization, not an admitted iteration.
The unrelated writer remains alive and advances before fixture cleanup in every case.
Both native regressions retain two normal recovery summaries with counters `[1,1,0,0]`.

`runtime-files.txt` lists nine runtime files.
Each source snapshot has a matching `.sha256` file.
The initial archive is `runner/node-source.tar.gz`.
The corrected and integrated archives are `green-source.tar.gz` and `integrated-source.tar.gz`.
The baseline inventory check passed before provisioning.
`final-source-check.txt` verifies the immutable snapshots and current integrated runtime bytes.

## Formal cases

The model is `formal/tlaplus/soak_disk/NativeControlPath.tla`.
The exact control is `MC_NativeControlPath_parent_only_pre_fix`.
Its named invariant is `UntrustedControlPreventsWorkload`.
The control exits 12 with three distinct states.
`MC_NativeControlPath` and `MC_NativeControlPath_trusted` exit 0 with two and three distinct states.

`formal-input/` and `formal-input.sha256` retain the seven execution inputs.
The actual outputs are `formal-red.txt`, `MC_NativeControlPath.txt`, and `MC_NativeControlPath_trusted.txt`.
Each output has its own exit and error stream.
No test in this cycle uses the shared `/tmp/tlc-*` paths.

The separate `native-control-invariant-review/` directory examines the integration owner's B47 control-order concern.
The unchanged B47 control reports its named invariant with exit 12.
Removing only that invariant makes all remaining checks pass with exit 0.
With `QueryAvailable=FALSE`, the model cannot reach phase `admitted`, so `VerifiedReleaseAdmits` holds.
No B47 formal file was changed.

## Retrieval and infrastructure

The runner is `ci-eph-f1r3node-rust-amd64-d2-20260912-090605-c92d41`.
It used the existing identity, exclusive-use, SSH, and expiration guards.
It did not register with GitHub or start a soak.
`termination.txt` records `TERMINATED`, and `final-source-check.txt` verifies the termination identity.

The first retrieval attempt failed because its selected path list named an obsolete retrieval directory.
`retrieve.sh` and `retrieval-prepare.stderr` retain that error.
This is an archive preparation failure, not a behavioral RED.
`retrieve-reviewed.sh` uses a new destination and derives its selected directory from that destination.
The reviewed retrieval completed without rerunning any production fixture.

The archive is `native-control-results.tar.gz`.
Its SHA-256 is `7031d6bfbfaca62db377dea37be00eaee2c7eae0d114d00339d9de628a3d5f07`.
It contains 298 regular members, occupies 261,267 bytes, and expands to 870,688 bytes.
Two FIFO entries are excluded, and no private environment payload is included.
The archive is not a complete filesystem image.

`verify-retrieval.rb` verifies archive and member hashes, safe paths, bounded expansion, invocation exits, observations, executed source copies, and recovery summaries.
`retrieval-audit.txt` records the successful audit before termination.
`handoff-retrieval-audit.txt` records a later repeat audit.
Fixture cleanup and VM termination are not credited as production containment.

## Integration work and limits

The integration owner retains the shared TDD plan, gate registrations, evidence publication, and claim inventory.
The two positive configurations and exact control above are ready for registration.
Earlier B47 execution must retain its original helper digest rather than use this correction's digest.
Combined gate results must retain actual TLC logs before classifier or routing tests overwrite shared paths.

The runtime fault covers one non-root control ancestor.
Concurrent directory replacement, every permission combination, mount changes, inherited descriptors, and separate source-path trust need further evidence.
Trusted administrators must not replace the verified directories during launch.
Private Docker containment, creation fencing, durable publication, and aggregate deadline and reserve bounds remain open.

Construction is not applicable to this finite helper model, and no correctness claim is discharged.
The workload, finalization semantics, 45-second wait, and harness pins remain unchanged.
No privileged test ran on the development host.
This session made no Git index change, commit, or push.

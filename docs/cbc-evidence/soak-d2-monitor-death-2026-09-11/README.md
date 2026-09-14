# D2 Monitor Death Evidence

## Status

B40 addresses monitor death during an active iteration.
The production and formal RED/GREEN checks pass for this selected behavior.
D2 and claim discharge remain pending.
Construction proofs do not apply to this Bash-driver model.

The [manifest](manifest.jsonc) binds source files, verification inputs, raw artifacts, runner observations, and complete published streams.
The [correspondence](../../../formal/tlaplus/soak_disk/CrashMonitorDeath.md) defines the model scope and its production observations.

## Selected shutdown result

The unchanged baseline leaves the driver active after confirmed monitor death.
The owned writer grows from 388 to 408 bytes during the final observation interval.
The unrelated writer also continues writing.
This is an observed shutdown failure, not a timeout-only result.

The correction adds an active-iteration check against the driver's running child jobs.
A missing monitor job creates a protection-breach record.
The existing interruption path stops the owned writer and retains failure.
The unchanged fixture confirms stable owned-writer data and continued unrelated-writer growth.
Two restarts refuse work and retain `[iterations, failures, bench_segments, bench_failures] = [1, 1, 0, 0]`.

| Check | RED | GREEN |
| --- | --- | --- |
| Production fixture | Exit 1 with continued owned writes | Exit 0 with stopped owned writes and two refused restarts |
| Formal control | Exit 12 for `MonitorDeathStopsOwnedWriter` | The exact control remains active |
| Corrected model | Not a production baseline | Exit 0 with five distinct states |

## Verification and lifecycle

The combined verification includes actual TLC runs, classifier checks, routing checks, emergency fixtures, and supporting regressions.
The manifest records the final results.
Actual TLC logs were saved before verifier substitutes ran.
Source snapshots identify bytes, not execution coverage.

The separate real-system batch passes all 11 selected Docker regression cases.
The retrieved archive contains 418 unique regular files and 2,107,137 bytes.
The archive passed validation before extraction.
Every extracted digest matches the archive record.
The diagnostic virtual machine was observed `TERMINATED` after retrieval.

The earlier bootstrap archive identifies `980285d4c`.
The separate runtime payload identifies the tested driver through its SHA-256 digest.
External commit `ab9519a5d` contains the correction.
This session does not attest that commit's hooks.
This session did not commit, push, dispatch hosted checks, run a node workload, or start an acceptance soak.

## Evidence packaging

Reruns are digest-only.
The raw archive is `[EVIDENCE_ROOT]/raw-streams.tar.gz`.
Its manifest record lists every rerun stream with `raw_path`, `raw_sha256`, and `raw_bytes`.
Reruns have no published entry and no file in this package.

The rerun patterns are `composed-*`, `final-*`, `supporting-*`, `emergency-*`, and `tlc-*.log`.
Only the exact filename `final-real.txt` is an exception to the `final-*` rule.
Published cycle logs use `.txt` filenames without content changes.
This package has no local `.gitignore`.
The packaging work does not modify historical package definitions.

Current-cycle real-system results use the `b40-real-system--` prefix.
The package publishes 59 current-cycle batch and lifecycle streams under that prefix.
The 405 retrieved streams for B27 through B38 remain digest-only, although this batch ran those regression cases again.
Their raw records retain the original paths, SHA-256 digests, and byte counts.
No `final-real--*` files remain in this package.

The revision preserves the raw archive bytes and the original build-input records.
The `packaging_source_sha256` map identifies the revised packaging inputs.
The original draft remains at `[EVIDENCE_ROOT]/packaging-revision-cY2eN3k7/original-package/`.
The revision record binds its original manifest by digest and byte count.

A separate historical-byte audit found 878 concurrent staged deletions before this package was built.
Those missing files block completion of the workspace inventory update.

```bash
ruby scripts/ci/check-soak-evidence-package.rb \
  docs/cbc-evidence/soak-d2-monitor-death-2026-09-11
ruby scripts/ci/check-soak-evidence-package.rb \
  docs/cbc-evidence/soak-d2-monitor-death-2026-09-11 \
  --raw-archive '[EVIDENCE_ROOT]/raw-streams.tar.gz'
```

Replace `[EVIDENCE_ROOT]` with the retained raw evidence directory before running the archive check.
The export check rejects a missing published file but accepts digest-only reruns without local files.

## Limits

B40 uses real native processes in a restricted process namespace.
The fixture disables the optional memory and disk guardians.
The Docker boundary returns success without creating Docker resources.
The separate Docker regressions do not test monitor death during a Docker workload.

The detached native writer closes its workload output descriptors.
This fixture does not establish shutdown with an inherited output pipe.
Monitor death during benchmarks and admission boundaries remains open.
Suspension, simultaneous driver failure, late creation, hidden writers, and failed stops remain open.

Ordinary file writes do not establish durable publication, successful upload, or storage-fault recovery.
Atomic model actions and weak fairness do not establish implementation refinement or an aggregate deadline.
Complete ownership, reserve bounds, hosted enforcement, human review, and acceptance remain pending.
The workload and its 45-second finalization wait remain unchanged.

## Reproduction

Use an immutable fixture image built from `scripts/bench/soak-disk-test.Dockerfile`.
The fixture creates a restricted container without host mounts, network access, or a Docker socket.

```bash
SOAK_DISK_TEST_IMAGE=sha256:<fixture-image-digest> \
  bash scripts/bench/test-soak-crash-monitor-death.sh \
  <source-directory> <new-evidence-directory>

TLA_TOOLS_JAR=<pinned-tla2tools.jar> JAVA_TOOL_OPTIONS=-Xmx512m \
  bash scripts/ci/check-tla-invariants.sh --soak-pr
```

Save the actual TLC logs before running the classifier or routing substitutes.
Run destructive Docker regression fixtures only on a guarded disposable diagnostic runner.

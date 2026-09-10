# D2 Temporary Session Preservation

## Result

B25 reproduces deletion of an unowned temporary session while a fixture writer holds its data file open.

The frozen baseline is `8eba1e4a719fccac8f45a7968c8836013bc9f26e`. The baseline fixture exits 1 after observing the deletion.

The correction removes the age-only temporary-session sweep. The corrected fixture exits 0 and preserves the directory and its data.

Both driver executions refuse new work below the admission threshold. Each driver exits 1 and records one protection failure.

The exact formal control exits 12 on `UnownedSessionPreserved`. The corrected configuration exits 0 with four distinct states.

## Combined verification

Twenty-two positive configurations and twenty-three exact controls pass. All forty-five actual TLC logs were retained before the classifier and routing tests ran.

Thirty-one emergency scenarios, 161 classifier cases, and six routing scenarios pass. The supporting regressions also pass.

The first driver-suite run expected the old unowned directory to be deleted. A diagnostic trace identified that obsolete assertion.

The corrected regression requires both old and recent unowned directories to remain. Its previous failure and diagnostic files remain retained.

The B24 evidence commit formatted the shared fixture and left its current inventory binding stale. That binding failure is not behavioral RED.

The current emergency suite includes all five B24 scenarios. Historical B24 source identities remain unchanged.

External commit `d64ae3bbf` contains the tested executable inputs. This session does not attest its commit hooks.

## Evidence and limits

[The manifest](manifest.jsonc) binds source snapshots, original raw files, complete published logs, and counted path substitutions.

The fixture uses real files and a live fixture writer inside an isolated container. No real node starts, and no host directory is mounted.

The correction preserves more temporary data and can cause earlier disk refusal. It does not establish safe reclamation of completed sessions.

Docker ownership, image preservation, other cleanup failures, complete shutdown, durable publication, deadline bounds, and reserve bounds remain open.

D2, D3, hosted verification, maintainer review, and acceptance remain pending. No claim is discharged, and no soak is authorized by these results.

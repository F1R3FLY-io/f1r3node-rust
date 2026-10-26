# TASK-017-4 Rust and Bash Migration

## Scope

The user approved Rust and Bash for new harness and profile code. The existing external Python suite remains unchanged.

Nine newly introduced Python files were removed. The replacement package is `scripts/casper-soak`.

The Bash driver calls `scripts/bench/casper-soak.sh`. The lifecycle executor and verification entry points also use Bash.

The seven proposed profile modules now use Rust paths. Those modules remain unimplemented, and node-profile admission remains blocked.

## Verification

The following checks passed against the migration sources:

| Check | Result |
| --- | --- |
| Host Rust tests | Passed |
| Isolated Linux Rust tests | 13 passed, none ignored |
| Production-driver invocations | 88 expected exits matched |
| Actual TLC controls | One clean control and ten expected violations |
| Clean model exploration | 43,424 distinct states |
| Isolated disk fixtures | 42 passed |
| Legacy driver fixtures | Passed |

The driver fixtures run without network access, host mounts, or a Docker socket. The containers use UID 65534 and bounded resources.

The final Linux driver test binary took 377.17 seconds. This measurement does not establish hosted-CI duration.

The binding runner requires exactly 88 invocations. It rejects failed tests, ignored tests, container exit mismatches, and out-of-memory termination.

The runtime checks compiled source copies and the executable digest. It does not treat Python-era qualification as Rust qualification.

The source-bound evidence is in [the migration report](../casper/cbc-evidence/runs/casper-rust-migration-20260917-01/report.json).

The report retains source, binding, and verification archives. The source archive includes new files that are not yet tracked by Git.

Earlier Python evidence remains unchanged. That evidence verifies only its recorded Python sources.

## Remaining Gates

TASK-017-4 remains open. The migration does not discharge the complete harness claim.

The synthetic lifecycle adapter cannot produce node observations. Fixture verifier substitutes cannot qualify actual node execution.

The seven profiles still need implementations and qualified adapters. Candidate approval, capability qualification, and workload pinning remain separate gates.

The strict claim gate, completion-helper compatibility, and final binding review remain unresolved. No claim waiver was added.

Both epic inventories now contain 33 artifacts. New mandatory tags require human ratification before contribution.

The migration adds pending Casper evidence ledgers. The existing shared workflow ledger remains separate and unchanged.

No node campaign, node-runtime repair, commit, or push was performed.

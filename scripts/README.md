# Scripts

Helper scripts intended to be run from the repository root.

## Available Scripts

| Script | Purpose |
| --- | --- |
| `scripts/run_rust_tests.sh` | Runs the script's fixed ten-crate release test set. It excludes `rho-pure-eval` and formal Loom workspace members. |
| `scripts/check-cost-accounted-rho-ALL.sh` | Runs every discovered local cost-accounting verification gate and fails on any missing or skipped mandatory witness |
| `scripts/check-cost-accounted-rho-coverage.sh` | Runs release package coverage with two jobs by default, matches dense raw profiles to exact ELF build identities, contains LLVM branch-mapping failures, and writes per-source line/branch evidence under `target/verification/cost-accounted-rho-coverage/` |
| `scripts/check-cost-accounted-rho-documentation.sh` | Checks the cost-accounting, Casper, Rholang, finality, fork-choice, and slashing documentation for pgmcp-compatible GFM math delimiters and balanced fenced blocks |
| `scripts/check-uptime-documentation.sh` | Checks permanent uptime documentation for pgmcp-compatible GFM math, balanced fenced blocks, and current rendered diagrams |
| `scripts/check-uptime-ALL.sh` | Runs the local aggregate uptime verification gate |
| `scripts/check-parallel-validator-consensus.sh` | Exhausts independent-validator replay, support, atomic floor publication, and crash/restart schedules with TLC and requires every named defective control to reproduce its invariant violation |
| `scripts/check-finalized-floor-ALL.sh` | Runs the complete Rocq, TLC, Apalache, property, integration, and negative-control gate for finalized-floor semantics |
| `scripts/check-fork-choice-ALL.sh` | Runs the local aggregate fork-choice verification gate |
| `scripts/check-deploy-lifecycle-ALL.sh` | Runs the local aggregate deploy-lifecycle verification gate |
| `scripts/check-merge-algebra-ALL.sh` | Runs the local aggregate merge-algebra verification gate |
| `scripts/check-slashing-ALL.sh` | Runs the local aggregate slashing verification gate |
| `scripts/delete_data.sh` | Deletes `.log` and `.mdb` files under `docker/` |

## Usage

Examples:

```bash
./scripts/run_rust_tests.sh
./scripts/check-cost-accounted-rho-ALL.sh
./scripts/check-cost-accounted-rho-coverage.sh
./scripts/check-cost-accounted-rho-documentation.sh
./scripts/check-uptime-ALL.sh
./scripts/check-parallel-validator-consensus.sh
bash scripts/check-finalized-floor-ALL.sh
bash scripts/check-fork-choice-ALL.sh
bash scripts/check-deploy-lifecycle-ALL.sh
bash scripts/check-merge-algebra-ALL.sh
bash scripts/check-slashing-ALL.sh
./scripts/delete_data.sh
```

## Cost-accounting coverage gate

The cost-accounting coverage gate uses Rust branch instrumentation for
`crypto`, `models`, `rholang`, and `casper`. The gate uses stable line
instrumentation for RSpace. RSpace has no branch-gated source in this gate.

The gate groups raw profiles by LLVM module signature. It pairs each profile
only with an executable that has the same ELF build identity. The gate extracts
exact source records from successful exports. This prevents use of a profile
with a different test binary.

LLVM 22 can crash while constructing branch-instantiation groups for some
generic async Rust mappings. The gate treats those crashes as unavailable
reports, not successful coverage. It requires at least one exact record for
every branch-reportable source and separately reruns the affected Casper engine
and RSpace suites with stable line instrumentation. The RSpace interface file
contains only type and trait declarations, so it is recorded as
declaration-only and compile-checked instead of receiving fabricated executable
line counts.

Each run owns a unique scratch directory under
`target/verification/cost-accounted-rho-coverage/`. A guarded `EXIT` trap
removes the scratch directory. The default invocation also removes each
instrumented package build after it extracts the package reports. This behavior
bounds disk use to one instrumented package build.

Set `COVERAGE_RETAIN_PROFILES=1` to retain instrumented builds for a later
reporting-only run. Then, use this command to reuse the profiles:

```bash
COVERAGE_REUSE_PROFILES=1 ./scripts/check-cost-accounted-rho-coverage.sh
```

Reuse fails if a required profile or source record is absent. This local
recovery path is not part of the aggregate verification gate or CI.

## Casper test scratch lifecycle

Casper's shared test LMDB is rooted at `target/casper-test-scratch`, not the
system temporary directory. This prevents repeated test binaries from placing
LMDB pages on hosts where `/tmp` is tmpfs. Each process receives an exact,
PID-prefixed directory and registers normal-exit deletion because Rust does not
drop `lazy_static` values. The Casper test suite includes a subprocess
regression that initializes the environment, exits, and confirms removal.
Abnormally terminated runs can leave only disk-backed diagnostic scratch under
that target directory.

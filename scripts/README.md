# Scripts

Helper scripts intended to be run from the repository root.

## Available Scripts

| Script | Purpose |
| --- | --- |
| `scripts/run_rust_tests.sh` | Runs the release test suite crate by crate |
| `scripts/check-cost-accounted-rho-ALL.sh` | Runs every discovered local cost-accounting verification gate and fails on any missing or skipped mandatory witness |
| `scripts/check-cost-accounted-rho-coverage.sh` | Runs release package coverage with two jobs by default, matches dense raw profiles to exact ELF build identities, contains LLVM branch-mapping failures, and writes per-source line/branch evidence under `target/verification/cost-accounted-rho-coverage/` |
| `scripts/check-cost-accounted-rho-documentation.sh` | Checks the cost-accounting, Casper, Rholang, finality, fork-choice, and slashing documentation for pgmcp-compatible GFM math delimiters and balanced fenced blocks |
| `scripts/check-uptime-documentation.sh` | Checks permanent uptime documentation for pgmcp-compatible GFM math, balanced fenced blocks, and current rendered diagrams |
| `scripts/check-parallel-validator-consensus.sh` | Exhausts independent-validator replay, support, atomic floor publication, and crash/restart schedules with TLC and requires every named defective control to reproduce its invariant violation |
| `scripts/check-finalized-floor-ALL.sh` | Runs the complete Rocq, TLC, Apalache, property, integration, and negative-control gate for finalized-floor semantics |
| `scripts/delete_data.sh` | Deletes `.log` and `.mdb` files under `docker/` |

## Usage

Examples:

```bash
./scripts/run_rust_tests.sh
./scripts/check-cost-accounted-rho-ALL.sh
./scripts/check-cost-accounted-rho-coverage.sh
./scripts/check-cost-accounted-rho-documentation.sh
./scripts/check-parallel-validator-consensus.sh
./scripts/check-finalized-floor-ALL.sh
./scripts/delete_data.sh
```

## Cost-accounting coverage gate

The cost-accounting coverage gate runs the complete release test suites for
`crypto`, `models`, `rholang`, `casper`, and `rspace_plus_plus` with unstable
Rust branch instrumentation. Raw profiles are grouped by LLVM module
signature, merged without sparse encoding, and paired only with an executable
whose ELF build identity equals the profile identity. One export is attempted
per matched executable, and exact source records are extracted from successful
exports. This prevents a profile from being interpreted against a different
test binary.

LLVM 22 can crash while constructing branch-instantiation groups for some
generic async Rust mappings. The gate treats those crashes as unavailable
reports, not successful coverage. It requires at least one exact record for
every branch-reportable source and separately reruns the affected Casper engine
and RSpace suites with stable line instrumentation. The RSpace interface file
contains only type and trait declarations, so it is recorded as
declaration-only and compile-checked instead of receiving fabricated executable
line counts.

Each run owns a unique scratch directory under
`target/verification/cost-accounted-rho-coverage/` and removes it with a
guarded `EXIT` trap. The default invocation always rebuilds profiles and runs
every test.
After a reporting-only failure, the following diagnostic invocation may reuse
the profiles from the immediately preceding complete run:

```bash
COVERAGE_REUSE_PROFILES=1 ./scripts/check-cost-accounted-rho-coverage.sh
```

Reuse fails if any required profile or exact source record is absent. It is a
local recovery path for the reporting phase and is not used by the aggregate
verification gate or CI.

## Casper test scratch lifecycle

Casper's shared test LMDB is rooted at `target/casper-test-scratch`, not the
system temporary directory. This prevents repeated test binaries from placing
LMDB pages on hosts where `/tmp` is tmpfs. Each process receives an exact,
PID-prefixed directory and registers normal-exit deletion because Rust does not
drop `lazy_static` values. The Casper test suite includes a subprocess
regression that initializes the environment, exits, and confirms removal.
Abnormally terminated runs can leave only disk-backed diagnostic scratch under
that target directory.

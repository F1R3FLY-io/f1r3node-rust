# CI Jobs and Check Names

This page lists the jobs of `.github/workflows/ci.yml`, `.github/workflows/ci-fork-pr.yml`, and `.github/workflows/_integration-pipeline.yml`. Each job has one purpose, and its name states that purpose. A red check therefore tells the reader which kind of work failed.

## Categories

| Category | Meaning |
| --- | --- |
| Gate | Collects the results of other jobs into one check. It runs no check of its own. |
| Static check | Reads files and history. It runs no program under test. |
| Script test | Runs a behavioral test of a CI, release, or soak script. It needs no Rust build. |
| Soak harness test | Builds the `casper-soak` harness and runs the real soak driver, also in Docker containers. |
| Unit test | Runs `cargo test` for a crate. |
| Build | Builds a release artifact or a Docker image. |
| Integration test | Runs a node network on ephemeral runners. |
| Setup | Resolves the target commit and the run decisions for the other jobs. |

## Jobs of `ci.yml`

| Job id | Check name | Category | Needs |
| --- | --- | --- | --- |
| `build_base` | Resolve Target | Setup | none |
| `static_checks` | Static Checks | Static check | `build_base` |
| `script_tests` | Script Tests | Script test | `build_base` |
| `soak_harness_tests` | Soak Harness Tests | Soak harness test | `build_base` |
| `deny` | cargo-deny | Static check | `build_base` |
| `markdown_link_check` | Markdown Link Check | Static check | `build_base` |
| `test` | Test (*crate*) | Unit test | `build_base`, `static_checks` |
| `test_casper` | Test (casper *n*/2) | Unit test | `build_base`, `static_checks` |
| `coverage` | Coverage (*crate*) | Unit test | `build_base`, `static_checks` |
| `coverage_summary` | Coverage Summary | Gate | `build_base`, `coverage` |
| `test_casper_gate` | Test Gate (casper) | Gate | `test_casper`, `coverage_summary` |
| `pipeline` | Integration Pipeline | Build and integration test | `build_base`, `static_checks`, `script_tests`, `soak_harness_tests` |
| `integration_tests_amd64` | Integration Gate (amd64) | Gate | `pipeline` |
| `integration_tests_arm64` | Integration Gate (arm64) | Gate | `pipeline` |
| `notify_queue_failure` | Notify Queue Failure | Notification | `pipeline`, both integration gates |
| `release_docker_image` | Release Docker Image | Build | the checks above |
| `release_packages` | Release Packages | Build | the checks above |

The Integration Pipeline waits for all three of Static Checks, Script Tests, and Soak Harness Tests. A failed inexpensive check therefore does not start ephemeral OCI runners.

### Static Checks

1. Verify workflow security invariants (first, before the toolchain install)
2. Verify commit identities in pull request commits (pull requests only)
3. Verify supply-chain controls
4. Check formatting
5. Run clippy

### Script Tests

The job runs 20 script tests:

- Commit trailer check
- Pre-push hook
- Stack CI gate
- Soak schedule routing
- System-integration node capability contract
- System-integration repin helper
- Release evidence
- Release gates
- Release promotion
- Release gate evidence
- Deployment train validator
- Deployment train manifests
- Release workflows
- TLA+ gate classification and routing
- Coverage summary aggregation
- Soak summary aggregation
- Soak verdicts
- Latency benchmark readiness and finalization
- Integration preflight driver
- Soak dashboard restoration

### Soak Harness Tests

1. Build the soak harness
2. Verify soak fail-closed driver (`scripts/bench/test-run-merge-recovery-soak.sh`)
3. Verify isolated disk admission and emergency scenarios (`scripts/bench/test-soak-disk-admission.sh`)
4. Verify the disk admission failure report and bounded-command liveness (`scripts/bench/test-soak-disk-admission-timing.sh`, with `SOAK_DISK_TEST_SKIP_TIMING=1`)

## Jobs of the integration pipeline

`_integration-pipeline.yml` runs as the Integration Pipeline job of `ci.yml` and of `ci-fork-pr.yml`. Its check names start with `Integration Pipeline /`.

| Job | Category |
| --- | --- |
| Await Launch Approval | Gate |
| Build Docker Image (*arch*) | Build |
| Launch Ephemeral Runners | Setup |
| Integration Tests (*arch*-*provider*) | Integration test |
| Integration Tests (*arch*-*provider*, ucc) | Integration test |
| Reclaim Ephemeral Runners | Setup |
| Smoke Test (Docker Standalone), (Docker Shard), (Local Standalone) | Integration test |

## Old and new check names

| Old name | New name | Change |
| --- | --- | --- |
| Build Base | Resolve Target | The job builds nothing. It resolves the target commit and the run decisions. |
| Lint | Static Checks, Script Tests, Soak Harness Tests | The job ran static checks, 20 script tests, and the soak harness tests. |
| Test (casper) | Test Gate (casper) | The job is a gate over the casper shards and the coverage summary. |
| Heavy Pipeline / *job* | Integration Pipeline / *job* | The caller job is the integration pipeline. |
| Integration Tests (amd64), (arm64) | Integration Gate (amd64), (arm64) | The jobs are gates over the integration test slots. |

Check names on pull requests and runs from before the change keep the old names.

## Required checks

The `devProtect` ruleset requires these checks:

- Static Checks, Script Tests, and Soak Harness Tests
- cargo-deny
- Test (*crate*) for each crate, and Test (casper 1/2) and Test (casper 2/2)
- Integration Gate (amd64) and Integration Gate (arm64)

The `masterProtect` ruleset requires Static Checks, Script Tests, Soak Harness Tests, cargo-deny, Test (*crate*) for each crate, and Test Gate (casper).

The heavy reuse gate in the `build_base` target step and `release-train.sh validate-ci-evidence` read Integration Gate (amd64) and (arm64). A pull request run from before TASK-023-4 has only the old names. A merge group therefore cannot reuse the heavy result of such a run, and it runs the Integration Pipeline again.

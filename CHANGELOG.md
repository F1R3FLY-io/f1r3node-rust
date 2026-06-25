# Changelog

All notable changes to the Rust implementation of F1r3node will be documented in this file.
This changelog is automatically generated from conventional commits.


## [0.4.16] - 2026-06-25

### Bug Fixes

- gate malloc-trim machinery to linux for cross-platform clippy parity
- align image artifacts with docs and code (#39) ([#39](https://github.com/F1R3FLY-io/f1r3node-rust/pull/39))
- gate malloc-trim machinery to linux-gnu for cross-platform clippy parity
- import ALLOCATOR_TRIM_TOTAL_METRIC under linux-gnu cfg
- drop unnecessary i64 casts in set_block_data FFI
- raise ulimit -n to 65536 for LMDB-heavy parallel tests
- align .env.example OCIR_REPO with public OCIR repo name
- restore block-interval guard on malloc_trim
- restore block-interval guard on malloc_trim
- drop unnecessary i64 casts in set_block_data FFI
- shared env cache with weak refs, fix EnvAlreadyOpened bypass
- render .env.remote keys from gitignored docker/.env
- surface shard-down and vps-bench-latency in `just --list`
- fund validator4 REV address so PoS bonding can succeed
- unify image name to f1r3fly-rust, add CI timeouts, dedup toolchain pin
- use #[allow(unused_variables)] instead of _ prefix for cfg-gated vars, rename Docker image to f1r3node-rust
- resolve clippy needless_if in block_processor_instance on Linux CI
- resolve clippy needless_ifs and missing import on Linux CI
- add cfg-gated import for ALLOCATOR_TRIM_TOTAL_METRIC on Linux
- remove unnecessary i64 casts flagged by clippy -D warnings
- run pre-push tests per-crate to avoid LMDB lock contention
- set executable bit in git index for pre-commit and pre-push hooks
- restore doc comment fencing broken by wrap_comments formatting
- restore wallet test data corrupted by format_strings rustfmt option
- add lmdb system dependency and scope pre-push clippy to lib targets

### CI

- push release commit via RELEASE_PAT to satisfy protected master
- remove TEST-ONLY fork-runner-fix trigger
- fix fork-PR pipeline startup, gating, and concurrency
- use ephemeral-launch-internal env for ungated path to fix startup_failure
- enable fork-PR access to ephemeral runners via gated pull_request_target
- make ephemeral-launch environment gate unconditional (#62) ([#62](https://github.com/F1R3FLY-io/f1r3node-rust/pull/62))
- gate launch_ephemeral_runners on ephemeral-launch environment (#60) ([#60](https://github.com/F1R3FLY-io/f1r3node-rust/pull/60))
- serialize workflow + deselect test_epoch_transition_under_heartbeat (#32) ([#32](https://github.com/F1R3FLY-io/f1r3node-rust/pull/32))
- restore explicit toolchain pin on Install Rust steps
- publish multi-arch Docker image to OCIR
- restrict Docker image release to master and tags only
- increase ARM64 integration test timeout scale to 2.0x
- add Docker build, integration tests, and release jobs for Oracle Cloud runners
- add GitHub Actions workflow with lint and per-crate test matrix

### Documentation

- rewrite CONTRIBUTING for staging flow; add SECURITY policy
- align CONTRIBUTING.md with F1R3FLY.io standard template
- document SAFETY contracts and ENV_CACHE lifecycle
- update last_updated frontmatter to 2026-04-15
- rename oracle-cloud-setup -> vps-cloud-testing, rewrite for 3 paths
- plan EPOCH-009 distributed OCI testbed for latency benchmarking
- document pulling the f1r3fly-rust image from OCIR
- set up migration epochs and stigmergic infrastructure
- expand local node and Docker setup instructions, add just install for Fedora

### Features

- align pre-commit/push policy with staging and dev
- add cargo-deny gate to pre-commit and CI
- TASK-009-5 latency benchmark port (closes EPOCH-009)
- shard-down recipe + ignore .claude/ runtime state
- TASK-009-4 Justfile recipes + deploy/status/teardown scripts
- TASK-009-3 distributed compose split for VPS-1/VPS-2
- TASK-009-2 image-transfer script and OCI setup guide
- TASK-009-1 OCI provisioning scripts for distributed latency testbed
- add CLI flags for ceremony-master-mode and mergeable-channel-gc
- add pre-commit and pre-push git hooks with lint/test gates
- extract pure Rust workspace from f1r3node rust/dev branch

### Miscellaneous

- align contributing docs and commit checks
- ignore proptest-regressions workspace-wide
- untrack per-crate Cargo.lock build artifacts
- ignore proptest-regressions workspace-wide
- default image to OCIR and fix helm double-tag bug

### Refactoring

- standardize logging top to bottom (#67) ([#67](https://github.com/F1R3FLY-io/f1r3node-rust/pull/67))
- TASK-002-1 extract prometheus/grafana to monitoring.yml

### Testing

- drop racy histogram-sample assertion (#33) ([#33](https://github.com/F1R3FLY-io/f1r3node-rust/pull/33))



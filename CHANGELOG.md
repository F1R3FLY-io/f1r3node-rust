# Changelog

All notable changes to the Rust implementation of F1r3node will be documented in this file.
This changelog is automatically generated from conventional commits.


## [0.4.31] - 2026-07-30

### Bug Fixes

- restore secret inheritance and probe credential delivery
- stop inheriting repository secrets into the heavy pipeline
- harden token and tag handling
- isolate GitHub App credentials
- pass environment secrets to reusable pipeline
- scope privileged credentials to trusted workflows
- raise the link-checker request timeout to 45s

### Miscellaneous

- match any App private key in the invariant check
- enforce workflow credential invariants and add code owners


## [0.4.30] - 2026-07-30

### Bug Fixes

- bump the system-integration pin past the validator4 certs

### Documentation

- record the pin-rot and no-op-slot failure modes as TASK-010-6


## [0.4.29] - 2026-07-30

### Bug Fixes

- supersede stale fork PR runs too
- supersede stale PR runs instead of queueing them
- drop the timeout on the approval-wait job
- gate the whole heavy pipeline on launch approval
- hold launch approval outside the OCI serialization lock
- keep internal PRs out of the fork concurrency group
- clear inherited git env before cargo in pre-commit


## [0.4.26] - 2026-07-29

### Bug Fixes

- pin just installer release
- clear inherited git env before cargo in pre-commit

### Documentation

- add per-branch CI and soak status badges


## [0.4.25] - 2026-07-29

### Bug Fixes

- clarify ReadOnlyMode error as validator not bonded
- complete PR #114 port — finalization scope, byte-bounded admission, DAG-width backpressure (#133) ([#133](https://github.com/F1R3FLY-io/f1r3node-rust/pull/133))
- address verified findings from PR #1 multi-agent review
- allow bootstrap network helper arity
- adapt merge resolution to parking_lot mutex
- format staging merge resolution
- resolve staging merge conflicts
- update anyhow for cargo-deny advisory
- address recovery review feedback
- align recovery wiring with staging APIs
- satisfy lint and release test checks
- skip already-cached mergeable-channel entries during replay
- key pre-push race checks off pushed refs, not HEAD
- remove unreachable duplicate match arms in error classifiers
- replace per-channel DashMap locks with fixed striped locks
- O(n) scan in enforce_history_cache_bounds -> AtomicUsize
- replace global RwLock in HotStore with DashMap per collection
- replace Vec::insert(0) with push in event_log writes
- update crossbeam-epoch for security advisory
- unblock CI for rho-pure-eval workspace crate

### CI

- verify install.py checksum, enforce pin/ref sync, harden release-tag parse
- pin oci-cli version 3.89.1 and complete SYSTEM_INTEGRATION_REF cross-refs
- isolate triple-bisim proptests to a serial bounded step
- skip duplicate heavy runs on promotion PRs
- accept 503 in markdown link check
- reserve node host ports in smoke jobs to stop ephemeral collisions
- retry OCI CLI installer download on transient 429s
- cut integration matrix from 20 to 4 OCI VMs per run
- strip broken Microsoft apt sources before installing dependencies
- update vars in invoking make job-run in testbed-quality-gate.yml
- add concurrency group, DURATION for heartbeat test
- add mntd parameter cap for tps experiment in testbed workflow
- add suite overview cards to testbed quality gate
- add testbed quality gate workflow
- harden OCI validation approval
- schedule cargo-deny audits
- harden OCI credential handling
- centralize OCI validation ref
- wire trusted OCI validation into CI
- modularize OCI validation workflow
- add trusted OCI validation workflow
- allow integration matrix shards to run concurrently
- pin system-integration to stable commit
- pin pyf1r3fly integration test utilities
- checkout pyf1r3fly for integration tests
- add scheduled supply-chain audit
- bump system-integration ref
- stabilize PR heavy checks
- aggregator skip-guard, full releases, runner-watchdog pin + OCI-auth hardening (#88) ([#88](https://github.com/F1R3FLY-io/f1r3node-rust/pull/88))
- pin OCI CLI installer by tag+sha256, dedupe system-integration ref

### Documentation

- document fail-closed bootstrap and to_map lock discipline
- canonicalize OCIR registry endpoint to us-sanjose-1.ocir.io
- add git worktree policy — no worktree creation without explicit user request
- document deferred eval_new sugar in plan
- align contributor CI policy
- add neutral cloud benchmark guide; restore vps-* testbed recipes
- remove /harmonize template markers from CONTRIBUTING.md

### Features

- multi-thread runtime
- recover validators from stale minority forks
- authenticate deployerId via rho:deploy:data (GAP-2)
- reject lib-namespace registration pending temp-name enforcement
- add registry_lookup unified URN dispatcher
- deprecation notify wiring (Step 7)
- public rho:registry:1.0.0 entry point (Step 6)
- lookupVersion resolver with semver matching (Step 5)
- rho:registry:ops:1.0.0 helper URN (Step 4)
- versioned-registry contracts and Rust probe spec (Step 3)
- wire VersionedRegistry.rho into genesis (Step 2)
- add semver module and versioned registry plan
- make http body limit configurable via config
- adopt rholang-rs method-call + agent block sugar

### Miscellaneous

- bump spin 0.9.8 -> 0.9.9 (yanked crate)
- update empty_state_hash_fixed for genesis contract addition
- remove working-artifact plan and issues docs
- update system integration ref
- align local checks with CI features
- clean up clippy and deny warnings
- re-pin rholang-parser to master HEAD after #95 merge
- bump anyhow 1.0.102 -> 1.0.103 for RUSTSEC-2026-0190
- drop stale RUSTSEC-2026-0097 advisory exception

### Performance

- gate RSS sampling behind mem_profile debug enablement
- remove per-read Mutex on LmdbKeyValueStore
- cut per-produce cloning in the matcher hot path

### Refactoring

- plumb urn_map into ProcessContext and SystemProcesses
- fix err messages for not found and enhance reporting response serialization
- name DAG lookup test result
- improve error messages in block query and exploratory deploy tests
- streamline error handling and improve response consistency in API

### Style

- format integration test matrix

### Testing

- await hot changes in capture regression
- add regression coverage for non-reproduced payload capture bug
- add minority fork recovery integration coverage
- cover stale validator recovery trigger
- use shared-rspace store manager in block DAG storage fixture
- fix foreign genesis rejection assertions to check DAG not buffer
- implement foreign genesis rejection test


## [0.4.15] - 2026-05-20

### Bug Fixes

- harden soak checkpoint publishing

### Features

- publish mid-run soak checkpoints


## [0.4.21] - 2026-07-16

### CI

- ignore soak dashboard URL in link check until first Pages deploy

### Documentation

- document maintainer-side removal of alert email subscriptions

### Features

- subscribe alert recipient emails via create-ons-topic.sh
- weekend benchmark metrics, gates, dashboard, and alerts (EPOCH-010)


## [0.4.19] - 2026-07-14

### CI

- pin OCI CLI installer by tag+sha256 in soak workflow
- add scheduled merge recovery soak on ephemeral OCI runner
- add scheduled merge recovery soak on ephemeral OCI runner

### Documentation

- make AI assistant guidance vendor-neutral, add subagent policy


## [0.4.18] - 2026-07-10

### CI

- pass aggregator gates on skipped pipeline, harden OCI auth step


## [0.4.17] - 2026-07-09

### CI

- add scheduled reaper for leaked ephemeral OCI runners


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



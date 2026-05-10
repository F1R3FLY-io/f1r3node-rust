# Slashing Search Horizon

This document defines the expanded defensive search program for finding
slashing bugs, vulnerabilities, projection risks, and theorem-strengthening
properties. It complements the specification, verification, threat model,
and traceability ledger; it is not a replacement for Rocq or TLA+ proof
authority.

## 1. Goal

The search program should maximize the chance of finding slashing issues
before production by combining exact models, randomized state machines,
coverage-guided fuzzing, symbolic Rust verification, model checking, and
system-level adversarial tests.

The governing rule is unchanged:

```
witness → source traceability → classification → proof/test/doc/source action
```

A generated witness is not called a Rust vulnerability unless it is
reproduced on the production Rust path or contradicts a production-path
invariant.

## 2. Search Layers

| Layer | Tooling | Purpose | Output |
|-------|---------|---------|--------|
| Exact finite modeling | Sage | Search small graph, stake, epoch, timing, arithmetic, and evidence spaces exactly. | Classified witness candidates. |
| Stateful generation | Hypothesis inside Sage | Explore longer lifecycle, partition, campaign, and horizon traces with shrinking. | Minimal JSON witnesses and replay fixtures. |
| Coverage-guided fuzzing | `cargo-fuzz` / libFuzzer | Drive Rust parsers, projections, and public slashing helpers through coverage feedback. | Minimized crash or assertion corpus entries. |
| Symbolic Rust checking | Kani | Prove bounded Rust helper properties and find counterexamples in fixed domains. | Proof success or concrete counterexample. |
| Runtime safety checking | Miri / sanitizers | Detect undefined behavior, invalid aliasing, data races, and sanitizer-visible memory faults where supported. | Runtime safety failure with reproducer. |
| Explicit model checking | TLC | Exhaust finite TLA+ configurations and regression models. | Exhaustion statistics or counterexample trace. |
| Symbolic model checking | Apalache | Explore bounded symbolic TLA+ traces where TLC state enumeration is too costly. | SMT-backed counterexample or bounded proof. |
| System adversarial testing | local multi-node harness / Jepsen-style schedules | Exercise partitions, restarts, delayed gossip, proposer withholding, clock skew, churn, and stale evidence on production-shaped nodes. | End-to-end exploitability result. |

## 3. Current Implemented Hooks

The repository now includes these additional hooks:

- `fuzz/` contains coverage-guided fuzz targets for checked
  sequence/epoch arithmetic, slash deploy round-trips, block-message
  proto normalization, structure-aware slash authorization paths,
  detector classification paths, and candidate-to-SlashDeploy lifecycle
  traces.
- `casper/src/rust/slashing_authorization.rs` contains Kani proof harnesses
  for `checked_base_seq`, `checked_next_seq`, bounded
  `epoch_for_block_number` domains, epoch-target predicates, received
  SlashDeploy authorization, individual authorization preconditions, and
  duplicate target-key equality.
- `scripts/ci/slashing-search-horizon.sh` runs authorization regressions,
  fuzz smoke targets when `cargo-fuzz` is installed, Kani harnesses when
  Kani is installed, fuzz-artifact triage, optional coverage reports when
  `RUN_COVERAGE=1`, optional mutation/supply-chain/model-check gates when
  `RUN_MUTANTS=1`, `RUN_DENY=1`, or `RUN_APALACHE=1`, optional Miri checks
  when `RUN_MIRI=1`, and
  tiered Sage/Hypothesis replay when a frontier/nightly/exhaustive tier
  is selected.

## 4. Commands

The search-horizon jobs are intentionally documented as manual commands
rather than active GitHub workflow jobs. On an Ubuntu runner or developer
machine, use this common setup:

```sh
rustup toolchain install nightly-2026-02-09
rustup default nightly-2026-02-09
sudo apt-get update
sudo apt-get install -y protobuf-compiler libssl-dev pkg-config
```

Manual equivalent of the removed **coverage-guided fuzz smoke** workflow job:

```sh
cargo install --locked cargo-fuzz
FUZZ_RUNS=10000 bash scripts/ci/slashing-search-horizon.sh
```

Manual equivalent of the removed **Kani symbolic checks** workflow job:

```sh
cargo install --locked kani-verifier
cargo kani setup
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness checked_base_seq_matches_i32_predecessor
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness checked_next_seq_matches_i32_successor
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness epoch_for_block_number_rejects_invalid_domain
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness epoch_for_block_number_matches_bounded_floor_division
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness slash_target_epoch_is_current_matches_epoch_projection
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness slash_evidence_epoch_matches_target_matches_epoch_projection
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness received_slash_deploy_authorized_rejects_invalid_domain
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness received_slash_deploy_authorized_is_conjunction_on_bounded_domain
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness slash_target_key_collides_matches_pair_equality
```

Run the integrated smoke search:

```sh
FUZZ_RUNS=10000 bash scripts/ci/slashing-search-horizon.sh
```

Run a deeper generated-fixture frontier:

```sh
SLASHING_SEARCH_TIER=frontier bash scripts/ci/slashing-search-horizon.sh
```

Run a persistent nightly-style frontier:

```sh
SLASHING_SEARCH_TIER=nightly bash scripts/ci/slashing-search-horizon.sh
```

Run the manually controlled exhaustive tier:

```sh
SLASHING_SEARCH_TIER=exhaustive RUN_ROCQ=1 RUN_TLA=1 bash scripts/ci/slashing-search-horizon.sh
```

The tiers are:

| Tier | Default fuzz runs | Additional behavior |
|------|-------------------|---------------------|
| `smoke` | `10,000` | Rust regression slice, fuzz smoke, Kani when installed, optional Miri. |
| `frontier` | `100,000` | Smoke plus Sage/Hypothesis `quick frontier` JSON fixture generation and Rust replay. |
| `nightly` | `1,000,000` | Smoke plus Sage/Hypothesis corpus search and Rust replay. |
| `exhaustive` | `1,000,000` | Smoke plus deep Sage/Hypothesis replay; Rocq/TLA+ are enabled only with `RUN_ROCQ=1` and `RUN_TLA=1`. |

Run the fuzz targets directly:

```sh
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run slashing_arithmetic -- -runs=10000 -max_len=64
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run slash_deploy_roundtrip -- -runs=10000 -max_len=512
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run block_message_roundtrip -- -runs=10000 -max_len=4096
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run slash_authorization_paths -- -runs=10000 -max_len=2048
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run equivocation_detector_paths -- -runs=10000 -max_len=2048
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" cargo fuzz run slash_lifecycle_trace -- -runs=10000 -max_len=4096
```

Run the Kani harnesses directly:

```sh
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness checked_base_seq_matches_i32_predecessor
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness checked_next_seq_matches_i32_successor
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness epoch_for_block_number_rejects_invalid_domain
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness epoch_for_block_number_matches_bounded_floor_division
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness slash_target_epoch_is_current_matches_epoch_projection
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness slash_evidence_epoch_matches_target_matches_epoch_projection
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness received_slash_deploy_authorized_rejects_invalid_domain
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness received_slash_deploy_authorized_is_conjunction_on_bounded_domain
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness slash_target_has_positive_bond_matches_positive
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness received_authorization_requires_positive_bond_on_bounded_domain
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness received_authorization_requires_invalid_evidence_on_bounded_domain
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness received_authorization_requires_current_epoch_on_bounded_domain
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness received_authorization_requires_evidence_epoch_on_bounded_domain
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' cargo kani -p casper --harness slash_target_key_collides_matches_pair_equality
```

Run the optional feedback gates:

```sh
RUN_COVERAGE=1 bash scripts/ci/slashing-search-horizon.sh
RUN_DENY=1 bash scripts/ci/slashing-search-horizon.sh
RUN_MUTANTS=1 SYSTEMD_MEMORY_MAX=32G SYSTEMD_CPU_QUOTA=400% bash scripts/ci/slashing-search-horizon.sh
RUN_APALACHE=1 bash scripts/ci/slashing-search-horizon.sh
```

The runner writes generated reports to `target/slashing-search-horizon/`.
These files are operational evidence, not specification authority. Durable
conclusions are promoted to the traceability ledger, formal models,
specification, threat model, and design documents only after Rust
traceability confirms the behavior.

Replay generated Sage/Hypothesis fixtures directly:

```sh
SLASHING_REPLAY_JSON=/tmp/slashing-hypothesis-frontier-fixtures.json \
SLASHING_RUST_FIXTURES_JSON=/tmp/slashing-horizon-rust-fixtures.json \
cargo test -p casper generated_
```

Run the Miri-compatible smoke check:

```sh
rustup component add miri rust-src
RUN_MIRI=1 bash scripts/ci/slashing-search-horizon.sh
```

The integrated runner stores Miri's sysroot cache under
`target/miri-cache` by default so sandboxed runs do not depend on a
writable home-directory cache.

## 5. Promotion Rules

| Finding class | Required action |
|---------------|-----------------|
| `confirmed_current_bug` | Fix Rust, add regression/integration test, update Rocq/TLA+ if behavior is normative, and update threat/spec/design docs. |
| `confirmed_fixed_bug` | Keep pre-fix regression and permitted-delta theorem; no new source change. |
| `not_reproduced_in_rust` | Record traceability and keep only if useful as a projection or model fixture. |
| `projection_risk_guarded` | Add/keep guard tests and specification text; source changes only if the production path reproduces the risk. |
| `assumption_counterexample` | Strengthen theorem preconditions and documentation; add counterexample fixture. |
| `proof_or_model_strengthening` | Promote to Rocq/TLA+ theorem/invariant and deterministic Rust fixture if it improves regression coverage. |

Every promoted witness must include:

- deterministic reproduction command,
- minimized input or trace,
- classification,
- Rust source traceability result,
- formal artifact target,
- regression test target,
- documentation target.

## 6. Search Priorities

The next highest-value expansions are:

1. Structure-aware fuzz targets that drive deeper production DAG,
   detector, report, and lifecycle paths beyond the current
   `validate_received_slash_deploys` authorization target.
2. Coverage-guided replay targets seeded from Sage/Hypothesis JSON
   traces, including minimization back into deterministic Rust fixtures.
3. Kani harnesses for record-key canonicalization, duplicate
   justification rejection, and small detector contribution domains.
4. Apalache checks for authorization and epoch/churn TLA+ models with
   larger symbolic validator and epoch domains than TLC can cheaply
   enumerate.
5. A local multi-node adversarial harness for partitions, delayed gossip,
   proposer withholding, restarts, stale evidence, and churn.
6. Semantic mutant campaigns for known bad slashing behaviors:
   stale-epoch authorization, duplicate-child over-count, missing-pointer
   abort, wrapping arithmetic, loose rebond identity, and report pruning.

## 7. References

- NIST SP 800-218, *Secure Software Development Framework (SSDF) Version
  1.1*, DOI: https://doi.org/10.6028/NIST.SP.800-218.
- NIST SP 800-154, *Guide to Data-Centric System Threat Modeling*:
  https://csrc.nist.gov/pubs/sp/800/154/ipd.
- Rust Fuzz Book, `cargo-fuzz`: https://rust-fuzz.github.io/book/cargo-fuzz.html.
- Rust Fuzz Book, structure-aware fuzzing:
  https://rust-fuzz.github.io/book/cargo-fuzz/structure-aware-fuzzing.html.
- Kani Rust Verifier: https://model-checking.github.io/kani/.
- Miri: https://github.com/rust-lang/miri.
- Apalache symbolic model checker for TLA+:
  https://apalache-mc.org/docs/apalache/index.html.

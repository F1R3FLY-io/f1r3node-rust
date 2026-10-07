# Scripts

Helper scripts intended to be run from the repository root.

## Available Scripts

| Script | Purpose |
| --- | --- |
| `scripts/check-fmt.sh` | The formatting check, decided by EXIT CODE (0 clean / 1 diffs / 2 tool error). `--self-test` proves it separates all three — a grep for `^Diff in` cannot, because a crashing rustfmt prints no diffs and reads as clean |
| `scripts/gate-router-post-commit` | The gate ROUTER. `--install` puts it where hooks actually fire; `--census` lists gates that declare no `@watches:`. A red fence is delivered to the agent that CAUSED it, because a post-commit hook's stdout goes to the committer's terminal — and git identity cannot route here, since every concurrent agent commits under one identity. Runs no cargo: 20.5 ms. ⚠ Do **not** "install the hooks" by setting `core.hooksPath=.githooks` — the tracked `.githooks/pre-commit` there runs `cargo fmt --check` and `cargo clippy --workspace -- -D warnings`, which is #82/#86 (sequenced last) and would block every commit immediately |
| `scripts/bench/stack-safety-phase7.sh` | Self-capped Phase 7 resource harness. It derives the converted register from `stack_depth_gate`, fits fixture-subtracted Cachegrind `Ir`/`Dr`/`Dw` ladders, and profiles substitution/sorting/normalization/evaluation peak heap with Massif and matched fixture controls. Results go under `target/phase7-stack-safety/` |
| `scripts/bench/stack-safety-phase7-histograms.sh` | Self-capped, feature-gated production-corpus depth histograms for the EPathMap escape arm, bincode encoder, bincode decoder, and protobuf decoder. Raw observations and summaries go under `target/phase7-stack-safety/`; the script refuses an unobserved subject |
| `scripts/run_rust_tests.sh` | Runs the release test suite crate by crate |
| `scripts/delete_data.sh` | Deletes `.log` and `.mdb` files under `docker/` |
| `scripts/setup-hooks.sh` | Installs the pre-commit and pre-push hooks |

## Formal Verification Scripts

These scripts need the tools in the [Formal Verification Tooling](../README.md#formal-verification-tooling) section of the repository README.

| Script | Purpose |
| --- | --- |
| `scripts/ci/check-formal-invariants.sh` | Runs the TLA+ and Rocq gates that scheduled CI runs. Use `--tla`, `--rocq`, or `--exhaustive`. |
| `scripts/ci/check-tla-invariants.sh` | Runs every TLA+ configuration in its gating list. The formal-invariants script calls it. |
| `scripts/ci/dump-tla-traces.sh` | Prints the counterexample traces from a TLC run |
| `scripts/ci/slashing-search-horizon.sh` | Runs the slashing fuzz tiers |
| `scripts/check-deploy-lifecycle-ALL.sh` | Runs the complete evidence set for the deploy-lifecycle area |
| `scripts/check-finalized-floor-ALL.sh` | Runs the complete evidence set for the finalized-floor area |
| `scripts/check-fork-choice-ALL.sh` | Runs the complete evidence set for the fork-choice area |
| `scripts/check-merge-algebra-ALL.sh` | Runs the complete evidence set for the merge-algebra area |
| `scripts/check-rspace-guards-ALL.sh` | Runs the complete evidence set for the RSpace guards area |
| `scripts/check-runtime-isolation-ALL.sh` | Runs the complete evidence set for the runtime-isolation area |
| `scripts/check-slashing-ALL.sh` | Runs the complete evidence set for the slashing area |

Each `check-*-ALL.sh` script runs the Rocq proofs, the TLA+ configurations, and the Rust tests for one theory area. Run one from the repository root. See [docs/formal-verification.md](../docs/formal-verification.md) for the method behind each area.

## Usage

Examples:

```bash
./scripts/run_rust_tests.sh
./scripts/delete_data.sh
./scripts/bench/stack-safety-phase7.sh all
./scripts/bench/stack-safety-phase7-histograms.sh
bash scripts/ci/check-formal-invariants.sh --all
./scripts/check-slashing-ALL.sh
```

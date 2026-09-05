# Scripts

Helper scripts intended to be run from the repository root.

## Available Scripts

| Script | Purpose |
| --- | --- |
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
bash scripts/ci/check-formal-invariants.sh --all
./scripts/check-slashing-ALL.sh
```

# Supply-Chain Policy

This directory holds `policy.toml`, the policy of the supply-chain check. The check runs in CI as the `cargo-deny` job. The checker is the Rust crate in `scripts/supply-chain/`. `@F1R3FLY-io/ci-maintainers` owns both directories.

The pre-commit hook runs only `cargo deny check`, which reads `deny.toml`. It does not read `policy.toml`, so it does not enforce the review dates.

## What the policy contains

| Section | Purpose |
|---|---|
| `manifests` | The Cargo manifests that the check scans: the workspace, `fuzz/`, and `scripts/soak-charts/`. |
| `[scanner]` | The pinned `cargo-deny` version and the sha256 of each release binary. |
| `[exceptions.<advisory>]` | One record for each RustSec advisory that the check accepts for a time. |

Each exception record has these fields:

- `package` and `versions`: the exact crate and versions that the advisory affects in a lock file.
- `owner`: the team that reviews the exception.
- `review-by`: the date of the next review.

Each exception must also appear in `advisories.ignore` in `deny.toml` with a reason. The checker refuses a policy when the two lists differ.

## Exception lifecycle

An exception is temporary. It records a known advisory that the project cannot fix yet, and the reason why.

- The `review-by` date must be after the current UTC date. On the `review-by` date itself, the check fails on every branch and in the merge queue.
- The check fails at 00:00 UTC on that date, which is the previous evening in the Americas.
- Starting 7 days before the date, the check prints a warning for each exception that is due. Under GitHub Actions, the warning is also a `::warning` annotation on the run. The warning does not fail the check.
- The usual review window is 30 days.

## Renew the exceptions

Do the renewal before the `review-by` date, preferably when the first warning appears.

1. Create a branch from `dev`.
2. For each exception, confirm that the package and versions are still in a lock file.
3. Confirm that the reason in `deny.toml` still describes the dependency correctly.
4. Remove the exception from both files when the dependency no longer exists.
5. Set a new `review-by` date, usually 30 days later.
6. Run the full check with the pinned `cargo-deny` (commands below).
7. Open a pull request to `dev` with the `ci-heavy` label.

A renewal is a review, not only a date change. Record what you checked in the pull request.

## Commands

```bash
# Install the pinned cargo-deny into a directory of your choice
bash scripts/ci/check-supply-chain.sh install --install-dir "$HOME/.cache/cargo-deny"

# Run the full check
PATH="$HOME/.cache/cargo-deny:$PATH" bash scripts/ci/check-supply-chain.sh check

# Run the checker tests
cargo test --locked -p supply-chain
```

To list the versions of a package in a lock file, search the `[[package]]` entries for its name, for example `grep -A1 'name = "dotenv"' Cargo.lock`.

# Contributing

## Before You Start

1. Install the native toolchain described in [DEVELOPER.md](DEVELOPER.md).
2. Create a feature branch from your working base.
3. Keep changes scoped to one concern per pull request.

## Local Checks

Run the smallest useful set for your change, then the broader checks if you touched shared behavior.

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test
```

Useful narrower commands:

```bash
cargo test -p node
cargo test -p casper
cargo test -p rholang --release
./scripts/run_rust_tests.sh
```

## Documentation Expectations

- Update Markdown when commands, ports, flags, paths, or workflows change.
- Keep examples runnable from the repository root unless the doc says otherwise.
- Prefer Rust-only instructions and terminology in this repository.

## Pull Requests

Include:

- What changed
- Why it changed
- How you verified it
- Any follow-up work that remains

If you skipped a validation step, state that explicitly in the PR description.

## Branches and Forks

New or occasional contributors should open pull requests from personal forks. Keep fork branches focused and up to date with the target branch.

Known recurring contributors may be invited to work from branches in the upstream `F1R3FLY-io/f1r3node-rust` repository. Maintainers grant upstream access based on project need, contributor identity, prior review history, and expected scope of work.

Upstream access does not bypass review. Protected branches such as `master`, `dev`, and `staging` still require pull requests and required checks before merge.

## CI Approval for Fork Pull Requests

CI for fork-based pull requests requires maintainer approval every time. This protects project CI capacity, GitHub-hosted minutes, and Oracle Cloud Infrastructure runner capacity while contributor trust is established.

Approval to run CI is not approval to merge. Maintainers may review the code, ask for local validation output, or request changes before approving expensive CI.

Full OCI-backed validation runs only after an owner or maintainer approves the pull request for full validation. Maintainers may ask a trusted contributor to move work to an upstream branch, or may mirror/cherry-pick reviewed work into an upstream branch before running the full pipeline.

Untrusted fork code must not run on persistent self-hosted runners. Project-managed compute for untrusted code should use disposable or ephemeral runners.

The project expects to evolve toward a contributor trust and reputation process. Until that process is formalized, maintainers decide when a contributor is trusted enough for upstream branch access and full CI use.

## Code Review Notes

- Keep crate boundaries clean.
- Add or update tests when behavior changes.
- Do not introduce undocumented setup steps.

## License

By contributing, you agree that your contributions are licensed under Apache License 2.0.

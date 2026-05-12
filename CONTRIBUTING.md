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

## Code Review Notes

- Keep crate boundaries clean.
- Add or update tests when behavior changes.
- Do not introduce undocumented setup steps.

## License

By contributing, you agree that your contributions are licensed under Apache License 2.0.

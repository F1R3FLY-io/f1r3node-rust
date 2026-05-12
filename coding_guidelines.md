# Coding Guidelines

Shared conventions for the Rust workspace and accompanying documentation.

## Common

1. Keep examples runnable from the repository root unless the document states otherwise.
2. Prefer small, composable changes over broad refactors without tests.
3. Update Markdown when behavior, commands, paths, or configuration change.

## Rust

1. Run `cargo fmt --all` before opening a PR.
2. Run `cargo clippy --workspace --all-targets` for non-trivial code changes.
3. Prefer explicit error propagation and typed errors at library boundaries.
4. Keep crate APIs focused and avoid leaking implementation details across workspace boundaries.
5. Add tests alongside behavior changes whenever practical.

## Rholang

1. Keep example contracts small and runnable.
2. Document any required keys, ports, or deploy sequence near the example itself.
3. Prefer release-mode validation for performance-sensitive interpreter examples.

## Documentation

1. Prefer Rust-only terminology in this repository.
2. Keep command blocks copy-pasteable.
3. Link to the nearest relevant README instead of duplicating long setup sections.

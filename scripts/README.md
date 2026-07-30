# Scripts

Helper scripts intended to be run from the repository root.

## Available Scripts

| Script | Purpose |
| --- | --- |
| `scripts/check-fmt.sh` | The formatting check, decided by EXIT CODE (0 clean / 1 diffs / 2 tool error). `--self-test` proves it separates all three — a grep for `^Diff in` cannot, because a crashing rustfmt prints no diffs and reads as clean |
| `scripts/gate-router-post-commit` | The gate ROUTER. `--install` puts it where hooks actually fire; `--census` lists gates that declare no `@watches:`. A red fence is delivered to the agent that CAUSED it, because a post-commit hook's stdout goes to the committer's terminal — and git identity cannot route here, since every concurrent agent commits under one identity. Runs no cargo: 20.5 ms. ⚠ Do **not** "install the hooks" by setting `core.hooksPath=.githooks` — the tracked `.githooks/pre-commit` there runs `cargo fmt --check` and `cargo clippy --workspace -- -D warnings`, which is #82/#86 (sequenced last) and would block every commit immediately |
| `scripts/run_rust_tests.sh` | Runs the release test suite crate by crate |
| `scripts/build_rust_libraries.sh` | Builds shared library artifacts under `rust_libraries/release/` |
| `scripts/build_rust_libraries_docker.sh` | Cross-builds shared libraries for Linux Docker targets |
| `scripts/build_rust_libraries_docker_native.sh` | Builds Linux shared libraries for the host architecture |
| `scripts/clean_rust_libraries.sh` | Removes generated shared library artifacts and cleans selected crates |
| `scripts/delete_data.sh` | Deletes `.log` and `.mdb` files under `docker/` |

## Usage

Examples:

```bash
./scripts/run_rust_tests.sh
./scripts/build_rust_libraries.sh
./scripts/delete_data.sh
```

## Notes

- The Docker-oriented build scripts expect either native Linux builds or the `cross` tool.
- The shared-library build scripts generate artifacts for integration and packaging workflows; they are not required for a normal `cargo build` of the workspace.

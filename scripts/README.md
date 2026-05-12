# Scripts

Helper scripts intended to be run from the repository root.

## Available Scripts

| Script | Purpose |
| --- | --- |
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

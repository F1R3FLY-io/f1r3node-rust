# rspace++

Tuple space storage and replay engine used by the Rust node.

## Responsibilities

- Manage hot and cold tuple space state
- Store and replay checkpoints and history
- Export and import trie-backed state
- Support mergeable channel and replay workflows used by `casper` and `rholang`

## Build

Main crate:

```bash
cargo build -p rspace_plus_plus
cargo build --release -p rspace_plus_plus
```

Rho types helper crate:

```bash
cargo build -p rspace_plus_plus_rhotypes
cargo build --release -p rspace_plus_plus_rhotypes
```

## Test

```bash
cargo test -p rspace_plus_plus
cargo test -p rspace_plus_plus --release
```

## Key Source Areas

| Path | Purpose |
| --- | --- |
| `src/rspace/history/` | Checkpoints, roots, history repository |
| `src/rspace/merger/` | Merge and state change logic |
| `src/rspace/state/` | Import, export, and state manager code |
| `src/rspace/shared/` | Store manager implementations |
| `tests/` | Replay, storage, export/import, and reporting tests |
| `libs/rspace_rhotypes/` | Helper crate shared with model and contract code |

## Notes

- The crate uses LMDB via `heed`.
- Test data is created under temporary directories and may leave `.mdb` files behind if interrupted.

# Decisions — P0-integration (all phase 0 patches on current dev)

Each entry gives the decision, the alternatives, the reason and whether it is reversible.

## D-P0-int-1 — Base and order

- **Decision:** `patch/P0-integration` starts at dev `1a19f1127`. It holds every phase 0 patch in plan order, as cherry-picks of the commits on the patch branches:

  | Order | Patch | Source branch tip |
  |---|---|---|
  | 1 | P0-00 | `patch/P0-00` `12dc47571` |
  | 2 | P0-01 | `patch/P0-01` `7bf43ebe1` |
  | 3 | P0-04 | `patch/P0-04` `431f625e3` |
  | 4 | P0-02 | `patch/P0-02` `e7003c322` |
  | 5 | P0-03 | `patch/P0-03` `934b56555` |
  | 6 | P0-05 | `patch/P0-05` (replayed on `patch/P0-06`) |
  | 7 | P0-06 | `patch/P0-06` `db69fd35b` |
  | 8 | P0-07 | `patch/P0-07` `5251c5f69` |
  | 9 | P0-08 | `patch/P0-08` `02d4c535c` |
  | 10 | P0-09 | `patch/P0-09` `3bb16243a` |
  | 11 | P0-10 | `patch/P0-10` `989f0bd1f` |
  | 12 | P0-12 | `patch/P0-12` `a21313e41` |

  P0-11 is dropped. P0-13 is separate. `patch/P1-01-hooks` is not in phase 0 and is not included.
- **Result:** All 33 commits applied without conflicts. The 36 dev commits after `252a86d1d` touch none of the patched hunks.
- **Reversible:** yes.

## D-P0-int-2 — Full release test suite

- **Decision:** Run `cargo test --release --workspace --no-fail-fast`. This covers the crates of `scripts/run_rust_tests.sh` and the workspace tool crates under `scripts/`.
- **Reason:** The pre-push gate runs release tests. In debug builds two genesis specs time out and one WAL test hits a `debug_assert`, also on dev. In release they pass.
- **Reversible:** yes.

## Results (2026-10-30, stable 1.97 with `RUSTC_BOOTSTRAP=1`, as root in a container)

| Package | Pass | Fail | Ignored |
|---|---|---|---|
| block-storage | 278 | 0 | 0 |
| casper (lib 733, `tests/mod.rs` 1152, other test targets) | 1942 | 0 | 16 |
| comm | 413 | 0 | 0 |
| crypto | 116 | 0 | 0 |
| graphz | 48 | 0 | 0 |
| models | 316 | 0 | 0 |
| node | 455 | 0 | 1 |
| rholang (with doc tests) | 2283 | 0 | 70 |
| rspace++ | 421 | 0 | 7 |
| shared | 147 | 1 | 0 |
| tool crates under `scripts/` | 315 | 14 | 5 |

All 15 failures come from the test environment, not from the patches:

- `shared` `a_failed_rotation_keeps_the_active_file_writable` makes a directory read-only and expects a write to fail. The tests ran as root, which ignores the mode bits. No patch touches `tracing_init`.
- `casper-soak`: 13 `host_control::linux` tests stop with "Run process fixtures in an isolated container". `manifest_resume_through_driver` ends with `writer_stop_failed` for the same reason. No patch touches `scripts/casper-soak`.

## Open before the phase 0 pull request

- The P0-07 soak has not run. Run it on this branch before the phase 0 pull request.
- P0-12 escalation (`decisions/P0-12.md`, D-P0-12-4): where the rejection-search shard parameters live before any shard sets an activation height.

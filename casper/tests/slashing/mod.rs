// Slashing-subsystem test suite.
//
// This module hosts the test catalogue prescribed by
// docs/theory/slashing/design/14-test-plan.md:
//   • 54 example-based use-case tests (uc_<NN>_*.rs)
//   • 27 property-based theorem tests (prop_t_*.rs)
//   •  1 cross-implementation bisimilarity test (uc_39_*.rs) running
//      every harness operation against the hand-translated Rocq
//      oracle in `oracle.rs`
//
// Pre-fix regression coverage is provided out-of-band: the bug-fix
// commits land sequentially, so reverting to the parent commit and
// re-running the post-fix UC tests reproduces the bug. No Cargo
// feature gating is used.
//
// Submodules:
//   • `types`       — local DagState / PoSState / Status enums
//   • `harness`     — SlashingTestHarness state-machine API (spec §14.2.1)
//   • `generators`  — proptest strategies (spec §14.2.2)         [pending]
//   • `oracle`      — Rust mirror of the Rocq definitions (§14.2.3) [pending]
//
// Per-bug regression tests and per-UC example tests are added as
// `pre_fix_bug_<N>.rs` / `uc_<NN>_*.rs` siblings as each lands.

mod harness;
mod types;

mod uc_03_ignorable_unrequested;

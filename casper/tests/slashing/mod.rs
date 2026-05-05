// Slashing-subsystem test suite.
//
// This module hosts the test catalogue prescribed by
// docs/theory/slashing/design/14-test-plan.md:
//   • 54 example-based use-case tests (uc_<NN>_*.rs)
//   • 27 property-based theorem tests (prop_t_*.rs)
//   •  9 pre-fix regression tests   (pre_fix_bug_<N>.rs), each gated
//      by the matching `pre-fix-bug-N` Cargo feature defined in
//      casper/Cargo.toml
//   •  1 cross-implementation bisimilarity test (uc_39_*.rs) running
//      every harness operation against the hand-translated Rocq
//      oracle in `oracle.rs`
//
// Submodules:
//   • `harness`     — SlashingTestHarness API (spec §14.2.1)
//   • `generators`  — proptest strategies (spec §14.2.2)
//   • `oracle`      — Rust mirror of the Rocq definitions (spec §14.2.3)
//   • `types`       — local DagState / PoSState / Status enums
//
// Sub-module files are added incrementally as each phase lands.

#[cfg(test)]
mod self_check {
    /// Compile-time sanity check that the slashing test scaffold is
    /// wired into the casper integration-test tree. No runtime work yet —
    /// the real harness lands with Phase 1.
    #[test]
    fn slashing_module_registered() {
        // Trivially-true assertion proves the module compiles and runs.
        assert!(true, "slashing module compiles");
    }
}

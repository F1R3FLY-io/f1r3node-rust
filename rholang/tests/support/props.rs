//! The property runner of the P5 suites, as `reified_p4_props.rs` runs its properties.
//!
//! Every property has a fixed seed, so a run is reproducible, and no failure is persisted to the
//! source tree. A pure property and a property over the in-memory agent run 64 cases. A
//! property that evaluates deploys on a Rholang runtime runs 16 cases and shrinks at most 64
//! times, because one case costs a runtime.

use proptest::prelude::ProptestConfig;
use proptest::strategy::Strategy;
use proptest::test_runner::{RngSeed, TestCaseError, TestRunner};

/// The fixed seed of every P5 property.
pub const PROPERTY_SEED: u64 = 0x0E0D_2026_1006_0005;

/// The cases of a pure property and of a property over the in-memory agent.
pub const PURE_CASES: u32 = 64;

/// The cases of a property that evaluates deploys on a Rholang runtime.
pub const RUNTIME_CASES: u32 = 16;

/// The config of a pure property and of a property over the in-memory agent.
pub fn pure_config() -> ProptestConfig {
    ProptestConfig {
        cases: PURE_CASES,
        rng_seed: RngSeed::Fixed(PROPERTY_SEED),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

/// The config of a property that evaluates deploys on a Rholang runtime.
pub fn runtime_config() -> ProptestConfig {
    ProptestConfig {
        cases: RUNTIME_CASES,
        rng_seed: RngSeed::Fixed(PROPERTY_SEED),
        failure_persistence: None,
        max_shrink_iters: 64,
        ..ProptestConfig::default()
    }
}

/// Runs the property `test` over `strategy` with `config`. A failure reports the reason and the
/// minimal failing input, as the `proptest!` macro does.
pub fn check<S>(
    config: ProptestConfig,
    strategy: S,
    test: impl Fn(S::Value) -> Result<(), TestCaseError>,
) where
    S: Strategy,
    S::Value: std::fmt::Debug,
{
    TestRunner::new(config)
        .run(&strategy, test)
        .unwrap_or_else(|failure| panic!("{failure}"));
}

/// The tokio runtime of one case. The Rholang runtime spawns tasks, so it needs worker threads.
pub fn tokio_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("build the tokio runtime of a case")
}

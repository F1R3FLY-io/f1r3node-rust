//! The multi-signature specs, as MODULES of the `mod.rs` integration target.
//!
//! # Why these live in a directory
//!
//! Cargo auto-discovers **every** `tests/*.rs` as its own integration-test
//! target. These three files were also declared as `mod`s of `casper/tests/
//! mod.rs`, so each one was compiled into two separate test binaries and every
//! test inside it ran twice on every `cargo test` / `cargo nextest run` — once
//! as `casper::mod::multi_sig_pipeline_spec::…` and once as
//! `casper::multi_sig_pipeline_spec::…`. Nothing depended on the duplication;
//! it was pure build time and pure test time, paid on every run.
//!
//! Cargo does **not** auto-discover `tests/<dir>/*.rs`, which is precisely why
//! every other module of that target — `add_block`, `api`, `batch1`, `batch2`,
//! `blocks`, `engine`, `genesis`, `helper`, `merging`, `multi_node`,
//! `slashing`, `sync`, `util` — is a directory. These three were the only
//! `mod`s declared as bare files at `tests/*.rs`, and that was the whole
//! defect. Putting them where the house structure already puts shared modules
//! makes the double compilation impossible rather than merely absent.
//!
//! ⚠ A file added HERE is a module of the aggregate target. A file added at
//! `casper/tests/*.rs` is a standalone target — correct for something like
//! `system_deploy_error_message_determinism.rs`, which declares no `mod` in
//! `casper/tests/mod.rs`. Do not do both for the same file.
//!
//! Test-name filters are unaffected: `cargo test multi_sig_pipeline_spec` still
//! selects the same tests, because the module path still contains that segment.

mod multi_sig_pipeline_spec;
mod multi_sig_runtime_fanout_spec;
mod multi_sig_runtime_integration_spec;

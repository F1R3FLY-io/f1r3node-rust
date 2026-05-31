//! D3 (DR-9, OD-1/OD-2) — REMOVED.
//!
//! This suite exercised the per-cosigner PRE-CHARGE / REFUND fan-out of the old
//! singular-phlo escrow model (`play_deploy_with_cost_accounting_cosigned`'s
//! precharge→user→refund loop and the per-signer
//! `generate_pre_charge_deploy_random_seed_for_signer` /
//! `generate_refund_deploy_random_seed_for_signer` seed derivations).
//!
//! D3 deletes that escrow model: a deploy's fundedness is proven once at block
//! assembly by the per-signature acceptance gate against the supply pool Σ⟦s⟧,
//! and the single consensus decrement is the settlement debit applied at block
//! close — there is NO per-deploy pre-charge / refund round-trip, hence no
//! per-cosigner seeds to test. The surviving multi-signature SIGNATURE / wire /
//! envelope behavior is covered by `multi_sig_pipeline_spec.rs` and
//! `multi_sig_runtime_integration_spec.rs`; the per-COMM consensus cost
//! equivalence (gate demand == runtime consumed == settlement debit) is covered
//! by `rholang/tests/accounting/delta_sigma_spec.rs` and the acceptance-gate
//! tests in `casper/src/rust/util/rholang/acceptance.rs`.

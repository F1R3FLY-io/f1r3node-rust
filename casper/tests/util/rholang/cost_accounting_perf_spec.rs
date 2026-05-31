//! D3 (DR-9, OD-1/OD-2) — REMOVED.
//!
//! This was a performance benchmark for the PoS cost-accounting system deploys
//! `PreChargeDeploy` / `RefundDeploy` (it timed the per-deploy escrow
//! pre-charge / refund round-trip and counted its RSpace produce/consume ops).
//!
//! D3 deletes that escrow model: there is no per-deploy pre-charge / refund —
//! a deploy's cost is the per-COMM token count, funded once against the
//! per-signature supply pool Σ⟦s⟧ and settled at block close. The benchmarked
//! system deploys no longer exist, so this benchmark has no successor.
//! (Block-replay timing remains covered by the standard replay path.)

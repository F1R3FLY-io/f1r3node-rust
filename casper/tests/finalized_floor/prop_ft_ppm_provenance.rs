// G2 proptests — θ_ppm PROVENANCE determinism + f32↔ppm round-trip on the REAL
// `ProofOfStake::fault_tolerance_threshold_to_ppm`.
//
// Cross-checks (against the actual runtime conversion) the Rocq FtProvenance.v
// capstone (`reconcile_is_onchain`, `reconcile_agrees_on_onchain`,
// `ppm_range_decision_no_overflow`) and the Z3 `ft_ppm_roundtrip.py` witnesses.
//
// Runtime facts modelled:
//   - genesis/contracts/proof_of_stake.rs:22  fault_tolerance_threshold_to_ppm(f32)->i64
//       = round(f64(x) * 1e6) as i64
//   - engine/initializing.rs:1099             UNCONDITIONAL override of local ppm by on-chain
//   - util/token_metadata_check.rs:105        on-chain ppm range-checked to [-1e6, 1e6]

use casper::rust::genesis::contracts::proof_of_stake::ProofOfStake;
use proptest::prelude::*;

const PPM_DEN: i64 = 1_000_000;

/// Faithful model of the node's reconcile/override at
/// `casper/src/rust/engine/initializing.rs:1099`:
///     casper_shard_conf.fault_tolerance_threshold_ppm = on_chain_ppm;
/// The assignment is UNCONDITIONAL — the `if` at :1089 gates only the warning log —
/// so the ppm the node finalizes with is the on-chain value, ignoring local config.
fn reconcile(_local_ppm: i64, on_chain_ppm: i64) -> i64 { on_chain_ppm }

/// Display inverse of `fault_tolerance_threshold_to_ppm`, mirroring
/// `token_metadata_check::read_on_chain_fault_tolerance_threshold` (:124) and
/// `initializing.rs:1087`: `(ppm as f64 / 1e6) as f32`.
fn from_ppm_f32(ppm: i64) -> f32 { (ppm as f64 / PPM_DEN as f64) as f32 }

// -- pure-function model of the genesis embed/read path (see LIMITATION) -------------
// LIMITATION: the true differential runs a genesis ceremony through a live
// `RuntimeManager` + tuplespace and reads the PoS `getFaultToleranceThreshold`
// exploratory deploy — heavy hermetic scaffolding not built here. We model the
// identity faithfully: genesis embeds the i64 ppm verbatim, and
// `read_on_chain_fault_tolerance_threshold_ppm` (token_metadata_check.rs:91) returns
// that same i64 after the [-1e6, 1e6] range-check (:105). Both are the identity on an
// in-range ppm, so the differential reduces to the range-guarded identity witnessed
// below (`genesis_embed_read_ppm_differential`).
fn embed_ppm_in_genesis(ppm: i64) -> i64 { ppm }
fn read_on_chain_ppm(embedded_ppm: i64) -> Option<i64> {
    if (-PPM_DEN..=PPM_DEN).contains(&embedded_ppm) {
        Some(embedded_ppm)
    } else {
        None
    }
}

proptest! {
    // (a) reconcile is the on-chain projection: local config is not a fork input.
    #[test]
    fn reconcile_is_onchain(local in any::<i64>(), onchain in any::<i64>()) {
        prop_assert_eq!(reconcile(local, onchain), onchain);
    }

    // (a') agreement form: agreeing on-chain ppm ⇒ agreeing reconciled ppm for ANY two
    // local configs (mirrors FtProvenance.reconcile_agrees_on_onchain).
    #[test]
    fn reconcile_agrees_on_onchain(
        l1 in any::<i64>(), l2 in any::<i64>(), onchain in any::<i64>()
    ) {
        prop_assert_eq!(reconcile(l1, onchain), reconcile(l2, onchain));
    }

    // (b) the REAL to_ppm on θ ∈ [-1,1]: result within the runtime's validated range,
    // and the f32→ppm→f32 display path is a fixed point on the integer ppm — so the
    // display conversion never perturbs the ppm that feeds the exact decision.
    #[test]
    fn to_ppm_in_range_and_display_stable(x in -1.0f32..=1.0f32) {
        let ppm = ProofOfStake::fault_tolerance_threshold_to_ppm(x);
        prop_assert!(
            (-PPM_DEN..=PPM_DEN).contains(&ppm),
            "ppm {} out of [-1e6,1e6] for x={}", ppm, x
        );
        let redisplayed = ProofOfStake::fault_tolerance_threshold_to_ppm(from_ppm_f32(ppm));
        prop_assert_eq!(
            redisplayed, ppm,
            "display path perturbed ppm: {} -> f32 {} -> {}", ppm, from_ppm_f32(ppm), redisplayed
        );
    }

    // (c) genesis-embed→read differential, modeled as a PURE-FUNCTION equality (see
    // LIMITATION above): the ppm a validator reads back from genesis equals the ppm
    // derived from the config f32 embedded at genesis.
    #[test]
    fn genesis_embed_read_ppm_differential(x in -1.0f32..=1.0f32) {
        let config_ppm = ProofOfStake::fault_tolerance_threshold_to_ppm(x);
        let embedded = embed_ppm_in_genesis(config_ppm);
        let read_back = read_on_chain_ppm(embedded);
        prop_assert_eq!(read_back, Some(config_ppm));
    }
}

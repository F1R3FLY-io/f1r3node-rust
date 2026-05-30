// Per-signature token-supply read/write helpers (Cost-Accounted Rho Calculus).
//
// Realizes DR-13 and [supply-realization-c-d-handoff.md] Decision 5 +
// [stageb-minting-halt-interface.md] Decision 5: the per-signature supply pool
// `Σ⟦s⟧` is a SINGLE balance-carrying datum `(TOKEN_TAG, n)` on the unforgeable
// channel `SignatureChannel::from_sig(s)` (`supply(s) = n`, 0 if absent). The
// channel is content-addressed and UNNAMEABLE from Rholang (no bytes→GPrivate
// surface primitive — handoff Decision 1), so the ONLY writer is this Rust
// module, riding a `GSysAuthToken`-bearing system deploy (handoff Decision 3).
//
// These helpers are shared by BOTH ends of the C↔D handoff:
//   * the supply PRODUCER (`CloseBlockDeploy::post_eval`, Stage B) — mints into
//     `Σ⟦v⟧` via [`produce_balance`];
//   * the WD-D2 acceptance-gate CONSUMER — reads `Σ⟦s⟧` via [`read_balance`] /
//     [`decode_balance_datum`] (NOT a second decoder — handoff Decision 5).
//
// INTEGRATION INVARIANT (handoff Coordination): [`supply_channel`] and WD-D0's
// `Sig::lane_hash` (accounting/mod.rs:1448-1459) MUST be anchored to the SAME
// `from_sig` basis so a deploy's lane key and its supply channel never drift.
// We realize that by deriving both from the single function
// `SignatureChannel::from_sig`; asserted by `supply_channel_equals_lane_pool_channel`.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{ETuple, Expr, ListParWithRandom, Par};
use models::rust::block::state_hash::StateHash;
use rholang::rust::interpreter::accounting::{Sig, SignatureChannel};

use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;

/// Fixed, genesis-scoped discriminator distinguishing a supply balance datum
/// from any other datum that might (in principle) reside on a signature
/// channel. The supply datum is `Σ⟦s⟧!( (TOKEN_TAG, n) )` with `n: Long` the
/// available `s`-layer count (spec Def 17 is a layer COUNT). Only Rust writes
/// the channel, so `TOKEN_TAG` confusion is structurally impossible (handoff
/// Decision 6 / TM-CA "TOKEN_TAG confusion" row), but the tag keeps the datum
/// self-describing and the decoder total.
pub const TOKEN_TAG: &str = "phlo";

/// The ONE channel-keying function: `Σ⟦s⟧ ≜ SignatureChannel::from_sig(s).par`.
///
/// This is the single canonical signature→name map used identically by the
/// Appendix-A translation, the supply producer (C), and the WD-D2 consumer
/// (handoff Decision 1). The g/#P axis collapses at the channel (DR-1: equal
/// atom bytes ⇒ equal channel) and compounds are permutation-invariant via
/// `ParSortMatcher::sort_match` (accounting/mod.rs:1544-1612).
pub fn supply_channel(sig: &Sig) -> Par {
    SignatureChannel::from_sig(sig).par
}

/// Pure decoder: extract the balance `n` from a channel's resident pars.
///
/// Finds the single `(GString(TOKEN_TAG), GInt(n))` tuple datum and returns
/// `n`; returns `0` when no such datum is present (the spec's `supply(s) = 0`
/// for an absent pool). Total over arbitrary `&[Par]`: any par that is not a
/// well-formed `(TOKEN_TAG, GInt)` tuple is skipped. The single-datum
/// invariant maintained by [`produce_balance`] means at most one tuple matches;
/// if several were present (they never are) the first in iteration order wins,
/// which is deterministic for a given channel's stored ordering.
pub fn decode_balance_datum(data: &[Par]) -> i64 {
    for par in data {
        if let Some(n) = decode_one_balance_par(par) {
            return n;
        }
    }
    0
}

/// Decode a single par as a `(TOKEN_TAG, n)` balance tuple, if it is one.
fn decode_one_balance_par(par: &Par) -> Option<i64> {
    // A balance datum is a single ETuple expr of arity 2: (GString TOKEN_TAG, GInt n).
    let expr = par.exprs.first()?;
    let ETuple { ps, .. } = match expr.expr_instance.as_ref()? {
        ExprInstance::ETupleBody(etuple) => etuple,
        _ => return None,
    };
    if ps.len() != 2 {
        return None;
    }
    let tag = match ps[0].exprs.first()?.expr_instance.as_ref()? {
        ExprInstance::GString(s) => s.as_str(),
        _ => return None,
    };
    if tag != TOKEN_TAG {
        return None;
    }
    match ps[1].exprs.first()?.expr_instance.as_ref()? {
        ExprInstance::GInt(n) => Some(*n),
        _ => None,
    }
}

/// Build the single balance datum par `(TOKEN_TAG, n)`.
pub fn balance_datum(n: i64) -> Par {
    let tag = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(TOKEN_TAG.to_string())),
    }]);
    let count = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(n)),
    }]);
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::ETupleBody(ETuple {
            ps: vec![tag, count],
            locally_free: Vec::new(),
            connective_used: false,
        })),
    }])
}

/// Read `supply(s) = n` from the LIVE hot store on `chan` (0 if absent).
///
/// Non-destructive (`get_data_par` reads the store without mutating it). This
/// is §7.6 step 3 "compute `Σ_c` from the available token stack" (tex
/// 1633-1634) and the read-half of [`produce_balance`]'s read-modify-replace.
pub async fn read_balance(runtime_ops: &RuntimeOps, chan: &Par) -> i64 {
    let data = runtime_ops.get_data_par(chan).await;
    decode_balance_datum(&data)
}

/// Read the supply balance, DISTINGUISHING an ABSENT pool from a present
/// zero-balance pool: returns `Some(n)` iff a `(TOKEN_TAG, n)` datum is resident
/// on `chan` (even `n == 0`), and `None` iff no balance datum exists at all.
///
/// This is the WD-D2 acceptance-gate ACTIVATION discriminator (reported grounding
/// refinement of the design's uniform "0 if absent"): a signer whose pool is
/// ABSENT (`None`) is not yet under cost-accounting funding — the Cost-Accounted
/// Rho ECONOMIC producer (Workstream C) has not provisioned its pool — so the
/// gate admits it WITHOUT funding enforcement and WITHOUT a settlement debit
/// (preserving pre-C / non-cost-accounted behavior bit-for-bit). A signer whose
/// pool is PRESENT (`Some(n)`, including a drained `Some(0)`) IS under
/// cost-accounting, so the gate enforces `Σ_s ≥ Δ_s + margin` and the §7.7
/// reject-both discipline (a drained pool correctly rejects further spends — the
/// spec's duplicate-deploy example, tex 1677-1687). `decode_balance_datum`
/// cannot make this distinction (it folds absent and present-zero both to 0), so
/// this presence probe is a separate, intentional read.
pub async fn read_balance_present(runtime_ops: &RuntimeOps, chan: &Par) -> Option<i64> {
    let data = runtime_ops.get_data_par(chan).await;
    decode_balance_present(&data)
}

/// Like [`decode_balance_datum`] but returns `None` when NO `(TOKEN_TAG, n)`
/// datum is present (vs `Some(n)` for a resident balance, including `Some(0)`).
/// Total over arbitrary `&[Par]`; the first matching tuple wins (the
/// single-datum invariant means at most one matches).
pub fn decode_balance_present(data: &[Par]) -> Option<i64> {
    for par in data {
        if let Some(n) = decode_one_balance_par(par) {
            return Some(n);
        }
    }
    None
}

/// Write the supply balance `n` to `chan` as the SINGLE datum `(TOKEN_TAG, n)`.
///
/// Read-modify-**replace**: any existing balance datum on `chan` is removed
/// first (`remove_all_data`) so the channel holds exactly one datum (handoff
/// Decision 2 / Stage B Decision 5 single-datum invariant). Because the
/// produce is a bare data write on a channel with no waiting continuation
/// (Rholang cannot name `Σ⟦v⟧`), it stores the datum directly (no COMM) — the
/// resulting trie leaf is determined solely by `(chan, datum, persist=false)`.
///
/// CONSENSUS NOTE (play/replay symmetry): `random_state` participates in the
/// stored datum's identity (`hash_produce` over the bincode-serialized datum,
/// stable_hash_provider.rs:63-76), hence in the post-state trie ROOT. Callers
/// therefore MUST pass a `random_state` that is byte-identical on play and
/// replay (e.g. derived from the close-block deploy's replay-stable
/// `initial_rand`). See [`CloseBlockDeploy::post_eval`].
pub async fn produce_balance(
    runtime_ops: &mut RuntimeOps,
    chan: &Par,
    n: i64,
    random_state: Vec<u8>,
) -> Result<(), CasperError> {
    // Drain any prior balance datum so the channel carries exactly one.
    runtime_ops
        .runtime
        .reducer
        .space
        .remove_all_data(chan)
        .await
        .map_err(|e| CasperError::RuntimeError(format!("supply remove_all_data failed: {}", e)))?;

    let data = ListParWithRandom {
        pars: vec![balance_datum(n)],
        random_state,
    };

    // Bare data write (no continuation on Σ⟦v⟧ ⇒ no COMM, just store_data).
    runtime_ops
        .runtime
        .reducer
        .space
        .produce(chan.clone(), data, false)
        .await
        .map_err(|e| CasperError::RuntimeError(format!("supply produce failed: {}", e)))?;

    Ok(())
}

/// Deterministic per-mint `random_state` for a supply produce.
///
/// Anchored to the close-block deploy's replay-stable `initial_rand`
/// (`generate_close_deploy_random_seed_from_*`, identical on play and replay)
/// advanced by `index` (the validator's position in the SORTED mint set). The
/// sorted order makes the derivation independent of fold/iteration order, so it
/// is byte-identical play/replay regardless of how the mint set was assembled.
pub fn mint_random_state(close_rand: &crypto::rust::hash::blake2b512_random::Blake2b512Random, index: i64) -> Vec<u8> {
    // `split_byte` takes an i8 path tag; clamp the per-validator index into a
    // stable byte and fold the high bits into the seed via a second split so we
    // never alias two validators' produce random states even past 127 mints.
    let lo = (index & 0x7f) as i8;
    let hi = ((index >> 7) & 0x7f) as i8;
    close_rand.split_byte(lo).split_byte(hi).to_bytes()
}

/// Deterministic per-DEBIT `random_state` for a WD-D2 settlement-debit produce,
/// on a RNG path DISJOINT from both the mint loop ([`mint_random_state`]) and the
/// close-block mint-list channel (`split_byte(0x2a)`). A validator may BOTH be
/// minted (its `Σ⟦v⟧` credited at an epoch boundary) AND have a pool debited in
/// the same close block; routing the debit through a distinct fixed
/// `DEBIT_RNG_PATH` prefix split before the per-index splits guarantees the mint
/// and debit produce DISTINCT datum identities even when they target the same
/// channel — so the read-modify-replace leaves a single, deterministic datum and
/// the post-state trie root is byte-identical on play and replay.
///
/// Anchored to the same close-block deploy `initial_rand` the mint uses
/// (`generate_close_deploy_random_seed_from_*`, identical on play and replay) and
/// advanced by the debit's position in the SORTED debit set — so the derivation
/// is independent of fold/iteration order and byte-identical play/replay.
pub fn debit_random_state(close_rand: &crypto::rust::hash::blake2b512_random::Blake2b512Random, index: i64) -> Vec<u8> {
    // Fixed domain split distinct from the mint's index-derived first split
    // (lo ∈ [0, 127]) and from the mint-list channel path (0x2a = 42): a
    // negative tag is unreachable by the non-negative `index & 0x7f`, so the
    // debit stream can never alias the mint stream.
    const DEBIT_RNG_PATH: i8 = -0x2b; // disjoint from mint (lo≥0) and 0x2a
    let lo = (index & 0x7f) as i8;
    let hi = ((index >> 7) & 0x7f) as i8;
    close_rand
        .split_byte(DEBIT_RNG_PATH)
        .split_byte(lo)
        .split_byte(hi)
        .to_bytes()
}

/// Deterministic `random_state` for the Cost-Accounted Rho Stage-C slash
/// `Σ⟦v⟧`-zero produce ([`SlashDeploy::post_eval`]), on a RNG path DISJOINT from
/// the close-block mint ([`mint_random_state`], `lo ∈ [0,127]`), the WD-D2
/// settlement debit ([`debit_random_state`], `-0x2b`), and the close-block
/// mint-list channel (`0x2a`). A slash deploy has at most ONE offender pool to
/// zero per deploy, so there is no per-index stream here — a single fixed
/// domain split off the slash deploy's `initial_rand` suffices. Anchored to the
/// slash deploy's replay-stable seed (`generate_slash_deploy_random_seed`,
/// byte-identical play/replay for the same proposer + seq_num + invalid block
/// hash), so the zeroed datum's identity — hence the post-state trie root — is
/// byte-identical on play and replay (the consensus-critical symmetry).
pub fn slash_random_state(slash_rand: &crypto::rust::hash::blake2b512_random::Blake2b512Random) -> Vec<u8> {
    // Fixed domain split distinct from the mint (lo≥0), debit (-0x2b), and
    // mint-list (0x2a) paths: a slash-zero produce can never alias a mint or
    // debit produce even when they target the same channel in different deploys.
    const SLASH_RNG_PATH: i8 = -0x2c;
    slash_rand.split_byte(SLASH_RNG_PATH).to_bytes()
}

/// Read the pre-state hash the supply read/write operate against. The supply
/// channel read/write target the LIVE hot store, which for `post_eval` is the
/// post-closeBlock state; the `pre_state_hash` is carried only for diagnostics
/// / the `ReplaySupplyMismatch` cross-check context.
pub fn pre_state_hash_hex(pre_state_hash: &StateHash) -> String {
    hex::encode(pre_state_hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rholang::rust::interpreter::accounting::Sig;

    /// The shared-basis integration invariant (handoff Coordination, Stage B
    /// Decision 5): `supply_channel(s)` is exactly `SignatureChannel::from_sig`
    /// of `s` — the SAME basis WD-D0's `Sig::lane_hash` is anchored to
    /// (accounting/mod.rs:1450 derives the lane key from `from_sig(self).par`).
    /// We assert (a) the channel equality and (b) that `lane_hash` is the
    /// domain-separated Blake2b256 of exactly this channel's wire encoding, so
    /// two signatures share a lane iff they share a supply channel — the
    /// no-drift property.
    #[test]
    fn supply_channel_equals_lane_pool_channel() {
        use prost::Message;

        let sigs = vec![
            Sig::Ground(vec![1, 2, 3, 4]),
            Sig::Ground(b"validator-pk-bytes".to_vec()),
            Sig::Quote(vec![9, 9, 9]),
            Sig::And(
                Box::new(Sig::Ground(vec![1])),
                Box::new(Sig::Ground(vec![2])),
            ),
            Sig::Unit,
        ];

        const SIGNATURE_LANE_DOMAIN: &[u8] =
            b"f1r3node:cost-accounted-rho:signature-lane:v1";

        for s in &sigs {
            // (a) supply_channel == from_sig basis.
            let supply = supply_channel(s);
            let from_sig = SignatureChannel::from_sig(s).par;
            assert_eq!(
                supply, from_sig,
                "supply_channel must equal SignatureChannel::from_sig for {:?}",
                s
            );

            // (b) lane_hash is anchored to the SAME channel (no drift).
            let encoded = supply.encode_to_vec();
            let mut domain_separated =
                Vec::with_capacity(SIGNATURE_LANE_DOMAIN.len() + encoded.len());
            domain_separated.extend_from_slice(SIGNATURE_LANE_DOMAIN);
            domain_separated.extend_from_slice(&encoded);
            let expected = crypto::rust::hash::blake2b256::Blake2b256::hash(domain_separated);
            assert_eq!(
                &expected[..32],
                &s.lane_hash()[..],
                "lane_hash must be the domain-separated Blake2b256 of supply_channel for {:?}",
                s
            );
        }
    }

    #[test]
    fn decode_balance_datum_absent_is_zero() {
        assert_eq!(decode_balance_datum(&[]), 0);
        // A non-balance par (bare GInt) is not a balance datum.
        let bare = RhoNumberLikePar(7).into();
        assert_eq!(decode_balance_datum(std::slice::from_ref(&bare)), 0);
    }

    #[test]
    fn decode_balance_datum_roundtrip() {
        for n in [0_i64, 1, 42, 1_000_000, i64::MAX] {
            let datum = balance_datum(n);
            assert_eq!(decode_balance_datum(std::slice::from_ref(&datum)), n);
        }
    }

    #[test]
    fn decode_balance_datum_wrong_tag_is_zero() {
        // A correctly-shaped tuple with the WRONG tag is ignored.
        let tag = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString("rev".to_string())),
        }]);
        let count = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(99)),
        }]);
        let wrong = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps: vec![tag, count],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }]);
        assert_eq!(decode_balance_datum(std::slice::from_ref(&wrong)), 0);
    }

    #[test]
    fn mint_random_state_is_deterministic_and_distinct_per_index() {
        use crypto::rust::hash::blake2b512_random::Blake2b512Random;
        let rand = Blake2b512Random::create_from_bytes(&[7_u8; 128]);
        let a0 = mint_random_state(&rand, 0);
        let a0_again = mint_random_state(&rand, 0);
        let a1 = mint_random_state(&rand, 1);
        let a200 = mint_random_state(&rand, 200);
        assert_eq!(a0, a0_again, "same index ⇒ byte-identical random_state");
        assert_ne!(a0, a1, "distinct indices ⇒ distinct random_state");
        assert_ne!(a1, a200, "distinct indices past 127 ⇒ distinct random_state");
    }

    // Tiny helper to make a bare GInt par for the absent-is-zero test.
    struct RhoNumberLikePar(i64);
    impl From<RhoNumberLikePar> for Par {
        fn from(v: RhoNumberLikePar) -> Par {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(v.0)),
            }])
        }
    }
}

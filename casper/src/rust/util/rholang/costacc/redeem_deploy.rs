// Cost-Accounted Rho Stage-C validator redemption system deploy (DR-7, DR-12).
//
// Governance-triggered (NOT auto-emitted by the block creator) adjudication of a
// quarantined (slashed) validator. Mirrors `slash_deploy.rs` in shape (a
// `SystemDeployTrait` whose `source()` invokes the PoS contract, here
// `@PoS!("redeemSlashed", …)`), but adds the DR-12 PoS-multisig-quorum PLATFORM
// OBLIGATION: the redemption datum `(validatorPk, outcome)` must be authorized by
// at least `pos_multi_sig_quorum` of the configured `pos_multi_sig_public_keys`,
// verified HERE in Rust before the verdict is passed into Rholang. The Rholang
// `redeemSlashed` is double-gated (`sysAuthToken` AND this `multiSigVerified`
// boolean); a false verdict rejects with no state change (no restore).
//
// Redemption writes NEITHER `Σ⟦v⟧` NOR `@W_v` directly (so there is NO supply
// `post_eval` here, unlike `SlashDeploy`/`CloseBlockDeploy`): its entire effect is
// the PoS Rholang state transition (un-halt / restore / penalty-to-coop / clear
// quarantine + stale epochs), captured by the normal system-deploy checkpoint and
// replayed via `replay_system_deploy_internal`. A restored validator is re-funded
// by the NORMAL next-epoch mint (all phlogiston creation stays on the single
// authorized path; `MintingHalt.v` `halted_validator_supply_not_increased`).

use std::collections::HashMap;
use std::collections::HashSet;

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::Par;
use models::rust::utils::{new_gbool_par, new_gint_par, new_gstring_par};
use rholang::rust::interpreter::rho_type::{Extractor, RhoBoolean, RhoNil, RhoString};
use rspace_plus_plus::rspace::history::Either;

use crate::rust::errors::CasperError;
use crate::rust::util::rholang::system_deploy::SystemDeployTrait;
use crate::rust::util::rholang::system_deploy_user_error::SystemDeployUserError;

/// Domain-separation tag for the redemption-authorization digest. Binds a
/// signature to "this is a Cost-Accounted Rho validator-redemption authorization"
/// so a multisig key's signature over some OTHER protocol message can never be
/// replayed as a redemption authorization.
const REDEMPTION_AUTH_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:redeem-authorization:v1";

/// The three adjudication outcomes (DR-3 two-effect slashing / spec Appendix B
/// "Slashing"). Encoded BOTH into the redemption-authorization digest (so the
/// signed datum commits to the exact outcome — a Guilty signature cannot be
/// replayed as Vindicated) AND into the Rholang `outcome` env value (the tuple
/// the `redeemSlashed` contract matches on).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RedemptionOutcome {
    /// Innocent: restore the full quarantined bond, un-halt, clear quarantine +
    /// stale epochs; the next-epoch mint re-funds.
    Vindicated,
    /// Partial: move `penalty` (clamped in Rholang to `[0, quarantinedBond]`) to
    /// `coopMultiVault` (the only coop growth path), restore the remainder.
    Guilty { penalty: i64 },
    /// Total: destroy the quarantined stake; the validator stays unbonded AND
    /// halted (only the quarantine record is cleared).
    Burned,
}

impl RedemptionOutcome {
    /// Stable discriminant tag — the FIRST element of the Rholang outcome tuple
    /// and the outcome marker in the authorization digest. Must match the string
    /// literals the Rholang `redeemSlashed` matches on (`"Vindicated"`,
    /// `"Guilty"`, `"Burned"`).
    fn tag(&self) -> &'static str {
        match self {
            RedemptionOutcome::Vindicated => "Vindicated",
            RedemptionOutcome::Guilty { .. } => "Guilty",
            RedemptionOutcome::Burned => "Burned",
        }
    }

    /// The penalty payload (0 for non-Guilty outcomes). The SECOND element of the
    /// Rholang outcome tuple (`Nil` is represented as `0` for Vindicated/Burned —
    /// the contract ignores the second element except in the Guilty branch).
    fn penalty(&self) -> i64 {
        match self {
            RedemptionOutcome::Guilty { penalty } => *penalty,
            _ => 0,
        }
    }

    /// The Rholang `outcome` env value: the tuple the `redeemSlashed` contract
    /// matches on. `("Vindicated", Nil)` / `("Guilty", penalty)` / `("Burned", Nil)`.
    fn to_rholang_par(&self) -> Par {
        use models::rhoapi::expr::ExprInstance;
        use models::rhoapi::{ETuple, Expr};

        let tag_par = new_gstring_par(self.tag().to_string(), Vec::new(), false);
        let snd_par = match self {
            RedemptionOutcome::Guilty { penalty } => new_gint_par(*penalty, Vec::new(), false),
            // Nil second element for Vindicated / Burned (the contract's `_`).
            _ => Par::default(),
        };
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps: vec![tag_par, snd_par],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }])
    }
}

/// A single cosigner authorization over the redemption datum: a `(public_key,
/// signature)` pair. The signature is over the [`RedeemDeploy::auth_digest`].
#[derive(Clone, Debug)]
pub struct RedemptionAuthorization {
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Clone)]
pub struct RedeemDeploy {
    /// The quarantined (slashed) validator being adjudicated.
    pub validator_pk: Vec<u8>,
    /// The adjudication outcome.
    pub outcome: RedemptionOutcome,
    /// The configured PoS-multisig public keyset (`$$posMultiSigPublicKeys$$`),
    /// hex-encoded — the genesis-configured `pos_multi_sig_public_keys`.
    pub pos_multi_sig_public_keys: Vec<String>,
    /// The configured quorum (`$$posMultiSigQuorum$$`) — the minimum number of
    /// DISTINCT authorizing keys for a redemption to be admitted (DR-7/DR-12).
    pub pos_multi_sig_quorum: u32,
    /// The cosigner authorizations over [`RedeemDeploy::auth_digest`].
    pub authorizations: Vec<RedemptionAuthorization>,
    pub initial_rand: Blake2b512Random,
}

impl RedeemDeploy {
    /// The canonical redemption-authorization digest the cosigners sign:
    /// `Blake2b256(DOMAIN ++ validatorPk ++ outcomeTag ++ penalty_le_bytes)`.
    /// Domain-separated and outcome-bound, so a signature authorizes exactly THIS
    /// `(validator, outcome)` redemption and nothing else.
    pub fn auth_digest(&self) -> Vec<u8> {
        let tag = self.outcome.tag().as_bytes();
        let penalty = self.outcome.penalty().to_le_bytes();
        let mut preimage = Vec::with_capacity(
            REDEMPTION_AUTH_DOMAIN.len() + self.validator_pk.len() + tag.len() + penalty.len(),
        );
        preimage.extend_from_slice(REDEMPTION_AUTH_DOMAIN);
        preimage.extend_from_slice(&self.validator_pk);
        preimage.extend_from_slice(tag);
        preimage.extend_from_slice(&penalty);
        Blake2b256::hash(preimage).to_vec()
    }

    /// DR-12 PoS-multisig-quorum PLATFORM OBLIGATION. Returns `true` iff at least
    /// `pos_multi_sig_quorum` DISTINCT keys of `pos_multi_sig_public_keys`
    /// produced a valid secp256k1 signature over [`RedeemDeploy::auth_digest`].
    ///
    /// Consensus-critical and DETERMINISTIC (pure function of the deploy's fields
    /// + the genesis multisig config), so it returns the SAME verdict on play and
    /// replay. Counting is over DISTINCT authorized keys: a single key signing
    /// twice, or a signature from a key NOT in the configured set, does not count
    /// toward quorum. A zero quorum (mis-config) is treated as never-satisfied
    /// (`quorum == 0 ⇒ false`) so redemption can never be unconditionally open.
    pub fn verify_multisig_quorum(&self) -> bool {
        if self.pos_multi_sig_quorum == 0 {
            return false;
        }

        // The set of configured authority keys (raw bytes), for O(1) membership.
        let authorized_keys: HashSet<Vec<u8>> = self
            .pos_multi_sig_public_keys
            .iter()
            .filter_map(|hex_key| hex::decode(hex_key).ok())
            .collect();

        let digest = self.auth_digest();
        let secp = Secp256k1;

        // Count DISTINCT authorized keys with a valid signature over the digest.
        let mut satisfied: HashSet<Vec<u8>> = HashSet::with_capacity(self.authorizations.len());
        for auth in &self.authorizations {
            if satisfied.contains(&auth.public_key) {
                continue;
            }
            if !authorized_keys.contains(&auth.public_key) {
                continue;
            }
            if secp.verify(&digest, &auth.signature, &auth.public_key) {
                satisfied.insert(auth.public_key.clone());
            }
        }

        satisfied.len() as u32 >= self.pos_multi_sig_quorum
    }

    fn mk_validator_pk(&self) -> (String, Par) {
        (
            "sys:casper:redeemValidatorPk".to_string(),
            new_gstring_par(hex::encode(&self.validator_pk), Vec::new(), false),
        )
    }

    fn mk_outcome(&self) -> (String, Par) {
        (
            "sys:casper:redeemOutcome".to_string(),
            self.outcome.to_rholang_par(),
        )
    }

    /// The DR-12 verdict env value: the Rust-verified multisig-quorum boolean the
    /// Rholang `redeemSlashed` consumes as `multiSigVerified`. Computed by
    /// [`RedeemDeploy::verify_multisig_quorum`] — the platform obligation — so the
    /// quorum check is enforced in Rust (consensus-critical, replay-deterministic)
    /// and merely WITNESSED in Rholang.
    fn mk_multisig_verified(&self) -> (String, Par) {
        (
            "sys:casper:redeemMultiSigVerified".to_string(),
            new_gbool_par(self.verify_multisig_quorum(), Vec::new(), false),
        )
    }
}

impl SystemDeployTrait for RedeemDeploy {
    type Output = (RhoBoolean, Either<RhoString, RhoNil>);
    type Result = ();

    fn source() -> &'static str {
        r#"
          new rl(`rho:registry:lookup`),
          poSCh,
          validatorPk(`sys:casper:redeemValidatorPk`),
          outcome(`sys:casper:redeemOutcome`),
          sysAuthToken(`sys:casper:authToken`),
          multiSigVerified(`sys:casper:redeemMultiSigVerified`),
          return(`sys:casper:return`)
          in {
            rl!(`rho:system:pos`, *poSCh) |
            for(@(_, PoS) <- poSCh) {
              @PoS!("redeemSlashed", *validatorPk.hexToBytes(), *outcome, *sysAuthToken, *multiSigVerified, *return)
            }
        }"#
    }

    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, Self::Result> {
        match value {
            (true, _) => Either::Right(()),
            (false, Either::Left(error_msg)) => Either::Left(SystemDeployUserError::new(error_msg)),
            _ => Either::Left(SystemDeployUserError::new(
                "Redemption failed unexpectedly".to_string(),
            )),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn rand(&self) -> Blake2b512Random {
        self.initial_rand.clone()
    }

    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = HashMap::new();

        let (vpk_key, vpk_value) = self.mk_validator_pk();
        env.insert(vpk_key, vpk_value);

        let (outcome_key, outcome_value) = self.mk_outcome();
        env.insert(outcome_key, outcome_value);

        let (sys_key, sys_value) = self.mk_sys_auth_token();
        env.insert(sys_key, sys_value);

        let (ms_key, ms_value) = self.mk_multisig_verified();
        env.insert(ms_key, ms_value);

        let (ret_key, ret_value) = self.mk_return_channel();
        env.insert(ret_key, ret_value);

        env
    }

    fn return_channel(&mut self) -> Result<Par, CasperError> {
        match self.env().get("sys:casper:return") {
            Some(par) => Ok(par.clone()),
            None => Err(CasperError::RuntimeError(
                "Return channel not found. This is a compile time error.".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;

    /// Build a redeem deploy with `n_keys` configured authority keys at
    /// `quorum`, signing the redemption digest with the FIRST `n_signers` of
    /// those keys (over the deploy's own `auth_digest`). Returns the deploy.
    fn build_deploy(
        outcome: RedemptionOutcome,
        n_keys: usize,
        quorum: u32,
        n_signers: usize,
    ) -> RedeemDeploy {
        let secp = Secp256k1;
        let validator_pk = b"offender-validator-pk".to_vec();

        // Generate `n_keys` authority keypairs.
        let keypairs: Vec<(Vec<u8>, Vec<u8>)> = (0..n_keys)
            .map(|_| {
                let (sk, pk) = secp.new_key_pair();
                (sk.bytes.to_vec(), pk.bytes.to_vec())
            })
            .collect();
        let pos_multi_sig_public_keys: Vec<String> =
            keypairs.iter().map(|(_, pk)| hex::encode(pk)).collect();

        // Construct the deploy WITHOUT authorizations first to compute the digest.
        let mut deploy = RedeemDeploy {
            validator_pk,
            outcome,
            pos_multi_sig_public_keys,
            pos_multi_sig_quorum: quorum,
            authorizations: Vec::new(),
            initial_rand: Blake2b512Random::create_from_bytes(&[3_u8; 128]),
        };
        let digest = deploy.auth_digest();

        // Sign with the first `n_signers` authority keys.
        deploy.authorizations = keypairs
            .iter()
            .take(n_signers)
            .map(|(sk, pk)| RedemptionAuthorization {
                public_key: pk.clone(),
                signature: secp.sign(&digest, sk),
            })
            .collect();
        deploy
    }

    #[test]
    fn multisig_quorum_met_is_verified() {
        // 3 keys, quorum 2, 2 valid signers ⇒ verified.
        let deploy = build_deploy(RedemptionOutcome::Vindicated, 3, 2, 2);
        assert!(
            deploy.verify_multisig_quorum(),
            "a quorum of valid authority signatures must verify"
        );
    }

    #[test]
    fn multisig_under_quorum_is_rejected() {
        // 3 keys, quorum 2, only 1 valid signer ⇒ rejected (no restore).
        let deploy = build_deploy(RedemptionOutcome::Vindicated, 3, 2, 1);
        assert!(
            !deploy.verify_multisig_quorum(),
            "fewer than quorum signatures must be rejected"
        );
    }

    #[test]
    fn multisig_unauthorized_key_does_not_count() {
        // 2 authority keys, quorum 2, but BOTH authorizations come from keys
        // NOT in the configured set ⇒ rejected (an outsider cannot redeem).
        let secp = Secp256k1;
        let mut deploy = build_deploy(RedemptionOutcome::Vindicated, 2, 2, 0);
        let digest = deploy.auth_digest();
        deploy.authorizations = (0..2)
            .map(|_| {
                let (sk, pk) = secp.new_key_pair();
                RedemptionAuthorization {
                    public_key: pk.bytes.to_vec(),
                    signature: secp.sign(&digest, &sk.bytes),
                }
            })
            .collect();
        assert!(
            !deploy.verify_multisig_quorum(),
            "signatures from keys outside the configured authority set must not count"
        );
    }

    #[test]
    fn multisig_duplicate_signer_counts_once() {
        // 3 keys, quorum 2, but the SAME (authorized) signer appears twice and
        // no second distinct signer ⇒ only 1 distinct key ⇒ rejected.
        let mut deploy = build_deploy(RedemptionOutcome::Vindicated, 3, 2, 1);
        let dup = deploy.authorizations[0].clone();
        deploy.authorizations.push(dup);
        assert_eq!(deploy.authorizations.len(), 2);
        assert!(
            !deploy.verify_multisig_quorum(),
            "a single key signing twice must count once toward quorum"
        );
    }

    #[test]
    fn multisig_signature_is_outcome_bound() {
        // A signature over a Vindicated digest must NOT authorize a Guilty
        // redemption (the digest commits to the outcome tag + penalty).
        let vindicated = build_deploy(RedemptionOutcome::Vindicated, 3, 2, 2);
        // Re-use the vindicated authorizations on a Guilty deploy with the SAME
        // keys — the digest differs, so the signatures no longer verify.
        let guilty = RedeemDeploy {
            outcome: RedemptionOutcome::Guilty { penalty: 50 },
            authorizations: vindicated.authorizations.clone(),
            ..vindicated.clone()
        };
        assert!(
            vindicated.verify_multisig_quorum(),
            "the vindicated authorizations verify against the vindicated digest"
        );
        assert!(
            !guilty.verify_multisig_quorum(),
            "vindicated signatures must NOT authorize a guilty redemption (outcome binding)"
        );
    }

    #[test]
    fn multisig_zero_quorum_is_never_open() {
        // A zero quorum (mis-config) must never be unconditionally satisfied,
        // even with valid signatures present.
        let deploy = build_deploy(RedemptionOutcome::Vindicated, 3, 0, 3);
        assert!(
            !deploy.verify_multisig_quorum(),
            "a zero quorum must be treated as never-satisfied"
        );
    }

    #[test]
    fn redemption_outcome_rholang_par_shapes() {
        // The outcome Par is a 2-tuple whose first element is the tag string.
        // (A light structural check; the contract matches on these.)
        let v = RedemptionOutcome::Vindicated.to_rholang_par();
        let g = RedemptionOutcome::Guilty { penalty: 7 }.to_rholang_par();
        let b = RedemptionOutcome::Burned.to_rholang_par();
        for p in [&v, &g, &b] {
            assert_eq!(p.exprs.len(), 1, "outcome par is a single tuple expr");
        }
    }
}

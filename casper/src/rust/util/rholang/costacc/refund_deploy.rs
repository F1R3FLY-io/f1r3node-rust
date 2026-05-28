// See casper/src/main/scala/coop/rchain/casper/util/rholang/costacc/RefundDeploy.scala

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::Par;
use models::rust::utils::{new_gbytearray_par, new_gint_par};
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoNil, RhoString};
use rspace_plus_plus::rspace::history::Either;

use crate::rust::errors::CasperError;
use crate::rust::util::rholang::system_deploy::SystemDeployTrait;
use crate::rust::util::rholang::system_deploy_user_error::SystemDeployUserError;

/// Refund the unused portion of a deploy's pre-charge back to a specific
/// signer's REV vault.
///
/// `pk` identifies which cosigner's vault receives the refund. For legacy
/// single-sig deploys this is the primary deployer's pk; for multi-sig deploys
/// the runtime fan-out at `runtime.rs::play_deploy_with_cost_accounting_cosigned`
/// constructs one `RefundDeploy { pk: signer.pk, refund_amount: ..., rand: ... }`
/// per cosigner in canonical pk-ascending FIFO drain order.
///
/// The on-contract `refundDeploy` method (per §1.7 PoS refinement) is
/// 4-argument `(deployerId, refundAmount, sysAuthToken, return)`. The
/// `deployerId` argument is wired into the env binding `sys:casper:deployerId`
/// (mirrors `PreChargeDeploy::env` at `pre_charge_deploy.rs:57`), and the
/// `source()` Rholang passes `*initialDeployerId` into the contract call.
/// The PoS Map `currentDeploysStateCh` then looks up this specific deployer's
/// pre-charge entry, validates the refund, transfers, and deletes the entry.
pub struct RefundDeploy {
    pub refund_amount: i64,
    pub pk: PublicKey,
    pub rand: Blake2b512Random,
    /// Per-deploy-group id scoping the PoS charge-tracking channel. MUST be
    /// the SAME value used by this deploy's `PreChargeDeploy` so the refund
    /// finds the cosigner's entry on `@(*posDeployStateTag, deployGroupId)`.
    /// See `system_deploy_util::deploy_group_id`.
    pub deploy_group_id: Vec<u8>,
}

impl SystemDeployTrait for RefundDeploy {
    type Output = (RhoBoolean, Either<RhoString, RhoNil>);
    type Result = ();

    fn source() -> &'static str {
        r#"
          new rl(`rho:registry:lookup`),
          poSCh,
          initialDeployerId(`sys:casper:deployerId`),
          deployGroupId(`sys:casper:deployGroupId`),
          refundAmount(`sys:casper:refundAmount`),
          sysAuthToken(`sys:casper:authToken`),
          return(`sys:casper:return`)
          in {
            rl!(`rho:system:pos`, *poSCh) |
            for(@(_, PoS) <- poSCh) {
                @PoS!("refundDeploy", *initialDeployerId, *deployGroupId, *refundAmount, *sysAuthToken, *return)
            }
        }"#
    }

    fn process_result(value: (bool, Either<String, ()>)) -> Either<SystemDeployUserError, ()> {
        match value {
            (true, _) => Either::Right(()),
            (false, Either::Left(error_msg)) => Either::Left(SystemDeployUserError::new(error_msg)),
            _ => Either::Left(SystemDeployUserError::new("<no cause>".to_string())),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn rand(&self) -> Blake2b512Random { self.rand.clone() }

    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = HashMap::new();

        // Bind `sys:casper:deployerId` so the contract source's
        // `initialDeployerId(`sys:casper:deployerId`)` resolves to THIS
        // cosigner's pk. This is what the PoS Map keys lookups on.
        let (d_key, d_value) = self.mk_deployer_id(&self.pk);
        env.insert(d_key, d_value);

        // Bind `sys:casper:deployGroupId` (GByteArray ground term) so the
        // contract operates on the SAME group-scoped channel the pre-charge
        // used: `@(*posDeployStateTag, deployGroupId)`.
        env.insert(
            "sys:casper:deployGroupId".to_string(),
            new_gbytearray_par(self.deploy_group_id.clone(), Vec::new(), false),
        );

        env.insert(
            "sys:casper:refundAmount".to_string(),
            new_gint_par(self.refund_amount, Vec::new(), false),
        );

        let (sys_key, sys_value) = self.mk_sys_auth_token();
        env.insert(sys_key, sys_value);

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

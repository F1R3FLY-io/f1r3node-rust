// See casper/src/main/scala/coop/rchain/casper/util/rholang/costacc/PreChargeDeploy.scala

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::Par;
use models::rust::utils::{new_gbool_par, new_gbytearray_par, new_gint_par};
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoNil, RhoString};
use rspace_plus_plus::rspace::history::Either;

use crate::rust::errors::CasperError;
use crate::rust::util::rholang::system_deploy::SystemDeployTrait;
use crate::rust::util::rholang::system_deploy_user_error::SystemDeployUserError;

pub struct PreChargeDeploy {
    pub charge_amount: i64,
    pub pk: PublicKey,
    pub rand: Blake2b512Random,
    /// Per-deploy-group id scoping the PoS charge-tracking channel
    /// (`deploy_group_id`). Identical across all cosigners of one deploy;
    /// distinct across deploys. See `system_deploy_util::deploy_group_id`.
    pub deploy_group_id: Vec<u8>,
    /// True only for the FIRST cosigner's pre-charge in a deploy's
    /// pk-ascending fan-out (`i == 0`). The PoS `chargeDeploy` contract
    /// seeds the group-scoped state map with `{}` exactly once, on this
    /// first charge. Safe because a deploy's charges run sequentially
    /// (awaited Rust loop), so there is no race among them.
    pub is_first: bool,
}

impl SystemDeployTrait for PreChargeDeploy {
    type Output = (RhoBoolean, Either<RhoString, RhoNil>);
    type Result = ();

    fn source() -> &'static str {
        r#"
          new rl(`rho:registry:lookup`),
          poSCh,
          initialDeployerId(`sys:casper:deployerId`),
          deployGroupId(`sys:casper:deployGroupId`),
          isFirst(`sys:casper:isFirst`),
          chargeAmount(`sys:casper:chargeAmount`),
          sysAuthToken(`sys:casper:authToken`),
          return(`sys:casper:return`)
          in {
            rl!(`rho:system:pos`, *poSCh) |
            for(@(_, PoS) <- poSCh) {
                @PoS!("chargeDeploy", *initialDeployerId, *deployGroupId, *isFirst, *chargeAmount, *sysAuthToken, *return)
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

        let (d_key, d_value) = self.mk_deployer_id(&self.pk);
        env.insert(d_key, d_value);

        // Bind `sys:casper:deployGroupId` to the per-deploy-group bytes as a
        // GByteArray ground term (mirrors how `mk_deployer_id` binds a
        // ground term for `sys:casper:deployerId`). In-contract this becomes
        // the 2nd component of the group-scoped channel `@(*tag, deployGroupId)`.
        env.insert(
            "sys:casper:deployGroupId".to_string(),
            new_gbytearray_par(self.deploy_group_id.clone(), Vec::new(), false),
        );

        // Bind `sys:casper:isFirst`; the contract seeds the group map with
        // `{}` only when this is true (first cosigner of the deploy).
        env.insert(
            "sys:casper:isFirst".to_string(),
            new_gbool_par(self.is_first, Vec::new(), false),
        );

        env.insert(
            "sys:casper:chargeAmount".to_string(),
            new_gint_par(self.charge_amount, Vec::new(), false),
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

// See casper/src/main/scala/coop/rchain/casper/util/rholang/costacc/SlashDeploy.scala

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::Par;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{EquivocationEvidence, SystemDeployData};
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::rho_type::{Extractor, RhoBoolean, RhoNil, RhoString};
use rspace_plus_plus::rspace::history::Either;

use crate::rust::errors::CasperError;
use crate::rust::util::proto_util;
use crate::rust::util::rholang::system_deploy::SystemDeployTrait;
use crate::rust::util::rholang::system_deploy_user_error::SystemDeployUserError;
use crate::rust::util::rholang::system_deploy_util;

#[derive(Clone)]
pub struct SlashDeploy {
    pub evidence: EquivocationEvidence,
    pub pk: PublicKey,
    pub initial_rand: Blake2b512Random,
}

impl SlashDeploy {
    /// The slash `pk` issues in its block at `seq_num`.
    pub fn new(evidence: EquivocationEvidence, pk: PublicKey, seq_num: i32) -> Self {
        let initial_rand = system_deploy_util::generate_slash_deploy_random_seed(
            pk.bytes.clone(),
            seq_num,
            &proto_util::commitment_hash(&evidence.first),
        );
        Self {
            evidence,
            pk,
            initial_rand,
        }
    }

    /// The block the slash cites to PoS: the first of the evidence pair. Its
    /// sender is the offender.
    pub fn cited_block_hash(&self) -> BlockHash {
        proto_util::commitment_hash(&self.evidence.first)
    }
}

impl SystemDeployTrait for SlashDeploy {
    type Output = (RhoBoolean, Either<RhoString, RhoNil>);
    type Result = ();

    fn source() -> &'static str {
        r#"
          new rl(`rho:registry:lookup`),
          poSCh,
          deployerId(`sys:casper:deployerId`),
          invalidBlockHash(`sys:casper:invalidBlockHash`),
          sysAuthToken(`sys:casper:authToken`),
          return(`sys:casper:return`)
          in {
            rl!(`rho:system:pos`, *poSCh) |
            for(@(_, PoS) <- poSCh) {
              @PoS!("slash", *deployerId, *invalidBlockHash.hexToBytes(), *sysAuthToken, *return)
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
                "Slashing failed unexpectedly".to_string(),
            )),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn recorded_form(&self) -> SystemDeployData {
        SystemDeployData::create_slash(self.evidence.clone(), self.pk.clone())
    }

    fn rand(&self) -> Blake2b512Random { self.initial_rand.clone() }

    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = HashMap::new();

        let (d_key, d_value) = self.mk_deployer_id(&self.pk);
        env.insert(d_key, d_value);

        env.insert(
            "sys:casper:invalidBlockHash".to_string(),
            new_gstring_par(hex::encode(self.cited_block_hash()), Vec::new(), false),
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

#[cfg(test)]
mod tests {
    use models::rust::block_implicits::get_random_block_default;

    use super::*;

    /// Every node and the replay path rebuild the slash's randomness from the
    /// proposer, its sequence number and the cited block alone.
    #[test]
    fn the_seed_derives_from_the_proposer_the_seq_num_and_the_cited_block() {
        let (a, b) = (get_random_block_default(), get_random_block_default());
        let evidence = EquivocationEvidence {
            first: proto_util::block_commitment(&a),
            second: proto_util::block_commitment(&b),
        };
        let pk = PublicKey::from_bytes(&[0x07; 32]);

        let deploy = SlashDeploy::new(evidence.clone(), pk.clone(), 42);

        assert_eq!(deploy.cited_block_hash(), proto_util::hash_block(&a));
        assert_eq!(
            deploy.initial_rand,
            system_deploy_util::generate_slash_deploy_random_seed(
                pk.bytes.clone(),
                42,
                &proto_util::hash_block(&a)
            )
        );
        let swapped = SlashDeploy::new(
            EquivocationEvidence {
                first: evidence.second,
                second: evidence.first,
            },
            pk,
            42,
        );
        assert_ne!(deploy.initial_rand, swapped.initial_rand);
    }
}

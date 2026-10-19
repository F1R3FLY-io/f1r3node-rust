// See models/src/main/scala/coop/rchain/models/NormalizerEnv.scala

use std::collections::HashMap;

use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;

use super::casper::protocol::casper_message::DeployData;
use super::deploy_parameters::{parameters_to_par, ParameterError};
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::{GDeployId, GDeployerId, GUnforgeable, Par};

const SYSTEM_DEPLOY_ID_URI: &str = "rho:system:deployId";
const SYSTEM_DEPLOYER_ID_URI: &str = "rho:system:deployerId";

pub fn with_deployer_id(deployer_pk: &PublicKey) -> HashMap<String, Par> {
    let mut env = HashMap::new();
    let deployer_id_par = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
            public_key: deployer_pk.bytes.to_vec(),
        })),
    }]);

    env.insert(SYSTEM_DEPLOYER_ID_URI.to_string(), deployer_id_par);
    env
}

pub fn normalizer_env_from_deploy(
    deploy: &Signed<DeployData>,
) -> Result<HashMap<String, Par>, ParameterError> {
    let parameter_values = parameters_to_par(&deploy.data.parameters)?;
    let mut env = HashMap::new();

    let deploy_id_par = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId {
            sig: deploy.sig.to_vec(),
        })),
    }]);

    let deployer_id_par = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
            public_key: deploy.pk.bytes.to_vec(),
        })),
    }]);

    env.insert(SYSTEM_DEPLOY_ID_URI.to_string(), deploy_id_par);
    env.insert(SYSTEM_DEPLOYER_ID_URI.to_string(), deployer_id_par);

    for (parameter, value) in deploy.data.parameters.iter().zip(parameter_values) {
        env.insert(format!("rho:deploy:param:{}", parameter.name), value);
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use crypto::rust::public_key::PublicKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use prost::bytes::Bytes;

    use super::*;

    fn signed_deploy_fixture() -> Signed<DeployData> {
        Signed {
            data: DeployData {
                term: "Nil".to_string(),
                time_stamp: 1,
                phlo_price: 1,
                phlo_limit: 1,
                valid_after_block_number: 0,
                shard_id: "root".to_string(),
                expiration_timestamp: None,
                parameters: Vec::new(),
            },
            pk: PublicKey::from_bytes(&[1, 2, 3, 4]),
            sig: Bytes::from(vec![5, 6, 7, 8]),
            sig_algorithm: Box::new(Secp256k1),
        }
    }

    #[test]
    fn with_deployer_id_binds_only_the_system_uri() {
        let deployer_pk = PublicKey::from_bytes(&[10, 11, 12, 13]);
        let env = with_deployer_id(&deployer_pk);

        assert!(env.contains_key(SYSTEM_DEPLOYER_ID_URI));
        assert!(!env.contains_key("rho:rchain:deployerId"));
        assert_eq!(env.len(), 1);
    }

    #[test]
    fn normalizer_env_from_deploy_binds_only_the_system_uris() {
        let deploy = signed_deploy_fixture();
        let env = normalizer_env_from_deploy(&deploy).unwrap();

        assert!(env.contains_key(SYSTEM_DEPLOY_ID_URI));
        assert!(env.contains_key(SYSTEM_DEPLOYER_ID_URI));
        assert!(!env.contains_key("rho:rchain:deployId"));
        assert!(!env.contains_key("rho:rchain:deployerId"));
        assert_eq!(env.len(), 2);
    }
}

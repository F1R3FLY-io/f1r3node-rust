use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signatures_alg::SignaturesAlgFactory;
use crypto::rust::signatures::signed::Signed;
use serde::{Deserialize, Serialize};

use crate::rust::casper::protocol::casper_message::DeployData;
use crate::rust::deploy_parameters::validate_parameters;

/// A signed deploy as the HTTP API carries it: the deploy data plus the
/// hex-encoded deployer key and signature.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DeployRequest {
    pub data: DeployData,
    pub deployer: String,
    pub signature: String,
    #[serde(rename = "sigAlgorithm")]
    pub sig_algorithm: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeployRequestError {
    #[error("Invalid deploy amount: {0}")]
    Amount(String),
    #[error("Invalid deploy parameters: {0}")]
    Parameters(String),
    #[error("Public key is not valid base16 format: {0}")]
    DeployerHex(String),
    #[error("Signature is not valid base16 format: {0}")]
    SignatureHex(String),
    #[error("Signature algorithm not supported: {0}")]
    UnsupportedAlgorithm(String),
    #[error("Invalid signature: {0}")]
    Malformed(String),
    #[error("Invalid signature")]
    Unverified,
}

impl DeployRequest {
    pub fn from_signed(deploy: &Signed<DeployData>) -> Self {
        Self {
            data: deploy.data.clone(),
            deployer: hex::encode(&deploy.pk.bytes),
            signature: hex::encode(&deploy.sig),
            sig_algorithm: deploy.sig_algorithm.name(),
        }
    }

    /// Decodes the request and verifies its signature.
    pub fn to_signed(&self) -> Result<Signed<DeployData>, DeployRequestError> {
        self.data
            .validate_amounts()
            .map_err(DeployRequestError::Amount)?;
        validate_parameters(&self.data.parameters)
            .map_err(|error| DeployRequestError::Parameters(error.to_string()))?;
        let pk_bytes = hex::decode(&self.deployer)
            .map_err(|error| DeployRequestError::DeployerHex(error.to_string()))?;
        let sig_bytes = hex::decode(&self.signature)
            .map_err(|error| DeployRequestError::SignatureHex(error.to_string()))?;
        let sig_algorithm = SignaturesAlgFactory::apply(&self.sig_algorithm)
            .ok_or_else(|| DeployRequestError::UnsupportedAlgorithm(self.sig_algorithm.clone()))?;
        Signed::from_signed_data(
            self.data.clone(),
            PublicKey::from_bytes(&pk_bytes),
            sig_bytes.into(),
            sig_algorithm,
        )
        .map_err(DeployRequestError::Malformed)?
        .ok_or(DeployRequestError::Unverified)
    }
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;

    use super::*;
    use crate::rust::deploy_parameters::{DeployParameter, RholangValue};

    fn sample_deploy_data() -> DeployData {
        DeployData {
            term: "new x in { x!(1) }".to_string(),
            time_stamp: 1,
            phlo_price: 1.into(),
            phlo_limit: 1000,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            parameters: Vec::new(),
        }
    }

    #[test]
    fn a_signed_deploy_round_trips_through_the_request() {
        let deploy = Signed::create(
            sample_deploy_data(),
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap();
        let mut request = DeployRequest::from_signed(&deploy);
        assert_eq!(request.to_signed().unwrap(), deploy);
        request.sig_algorithm = "ed25519".to_string();
        assert_eq!(
            request.to_signed().unwrap_err().to_string(),
            "Signature algorithm not supported: ed25519"
        );
    }

    #[test]
    fn deploy_parameters_survive_json_and_reject_tampering() {
        let (sk, _) = Secp256k1.new_key_pair();
        let mut data = sample_deploy_data();
        data.parameters = vec![DeployParameter {
            name: "input".into(),
            value: RholangValue::List(vec![RholangValue::Int(-42), RholangValue::Nil]),
        }];
        let signed = Signed::create(data, Box::new(Secp256k1), sk).unwrap();
        let json = serde_json::to_string(&DeployRequest::from_signed(&signed)).unwrap();
        let mut restored: DeployRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.to_signed().unwrap(), signed);
        restored.data.parameters[0].value = RholangValue::Nil;
        assert_eq!(restored.to_signed(), Err(DeployRequestError::Unverified));
        restored.data.parameters[0].name = "invalid:name".into();
        assert!(restored
            .to_signed()
            .unwrap_err()
            .to_string()
            .contains("Invalid deploy parameters"));
        assert!(
            serde_json::from_str::<DeployRequest>(&serde_json::to_string(&restored).unwrap())
                .is_err()
        );
    }

    #[test]
    fn malformed_requests_name_their_fault() {
        let base = DeployRequest {
            data: sample_deploy_data(),
            deployer: "zz".to_string(),
            signature: "00".to_string(),
            sig_algorithm: "secp256k1".to_string(),
        };
        assert!(matches!(
            base.to_signed(),
            Err(DeployRequestError::DeployerHex(_))
        ));

        let mut negative_limit = base.clone();
        negative_limit.data.phlo_limit = -1;
        assert!(matches!(
            negative_limit.to_signed(),
            Err(DeployRequestError::Amount(_))
        ));

        let bad_sig_hex = DeployRequest {
            deployer: "00".to_string(),
            signature: "zz".to_string(),
            ..base.clone()
        };
        assert!(matches!(
            bad_sig_hex.to_signed(),
            Err(DeployRequestError::SignatureHex(_))
        ));

        let unsupported = DeployRequest {
            deployer: "00".to_string(),
            signature: "00".to_string(),
            sig_algorithm: "rot13".to_string(),
            ..base.clone()
        };
        assert_eq!(
            unsupported.to_signed(),
            Err(DeployRequestError::UnsupportedAlgorithm(
                "rot13".to_string()
            ))
        );

        let (_sk, pk) = Secp256k1.new_key_pair();
        let wrong_sig = DeployRequest {
            deployer: hex::encode(&pk.bytes),
            signature: "0011".to_string(),
            ..base
        };
        assert!(wrong_sig.to_signed().is_err());
    }
}

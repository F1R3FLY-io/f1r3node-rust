// See casper/src/main/scala/coop/rchain/casper/ValidatorIdentity.scala

use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::{SignaturesAlg, SignaturesAlgFactory};
use models::casper::Signature;
use models::rust::casper::protocol::casper_message::BlockMessage;

use super::errors::CasperError;
use super::util::proto_util;

#[derive(Clone)]
pub struct ValidatorIdentity {
    pub public_key: PublicKey,
    pub private_key: PrivateKey,
    pub signature_algorithm: String,
}

impl ValidatorIdentity {
    pub fn new(private_key: &PrivateKey) -> Self {
        let public_key = Secp256k1.to_public(private_key);

        Self {
            public_key,
            private_key: private_key.clone(),
            signature_algorithm: Secp256k1.name(),
        }
    }

    pub fn signature(&self, data: &[u8]) -> Signature {
        let signature = SignaturesAlgFactory::apply(&self.signature_algorithm)
            .expect("Failed to apply signature algorithm")
            .sign_with_private_key(data, &self.private_key);

        Signature {
            public_key: self.public_key.bytes.clone(),
            algorithm: self.signature_algorithm.clone(),
            sig: signature.into(),
        }
    }

    pub fn sign_block(&self, block: &BlockMessage) -> BlockMessage {
        let mut new_block = block.clone();
        new_block.sig_algorithm = self.signature_algorithm.clone();
        new_block.sender = self.public_key.bytes.clone();

        let block_hash = proto_util::hash_block(&new_block);
        let sig = self.signature(block_hash.as_ref());

        new_block.sig = sig.sig;
        new_block.block_hash = block_hash;

        new_block
    }

    /// Fast-path variant of `sign_block` that reuses per-deploy Merkle
    /// leaves computed earlier on the propose path.  Correct iff the
    /// provided `leaves` match `block.body.deploys` + `block.body.system_deploys`
    /// in commit order — the caller is responsible for threading them
    /// through unchanged.  Saves one `deploys_root_leaves` recomputation
    /// at sign time.
    pub fn sign_block_with_leaves(
        &self,
        block: &BlockMessage,
        leaves: &[[u8; 32]],
    ) -> BlockMessage {
        let mut new_block = block.clone();
        new_block.sig_algorithm = self.signature_algorithm.clone();
        new_block.sender = self.public_key.bytes.clone();

        let block_hash = proto_util::hash_block_with_leaves(&new_block, leaves);
        let sig = self.signature(block_hash.as_ref());

        new_block.sig = sig.sig;
        new_block.block_hash = block_hash;

        new_block
    }

    pub fn from_private_key_with_logging(
        priv_key: Option<&str>,
    ) -> Result<Option<Self>, CasperError> {
        match priv_key {
            Some(priv_key) => Self::from_hex(priv_key).map(Some),
            None => {
                tracing::warn!("No private key detected, cannot create validator identification.");
                Ok(None)
            }
        }
    }

    fn from_hex(priv_key_hex: &str) -> Result<Self, CasperError> {
        if priv_key_hex.len() != 64 {
            return Err(CasperError::SigningError(
                "Validator private key must contain 64 hexadecimal characters".into(),
            ));
        }
        let decoded = hex::decode(priv_key_hex).map_err(|_| {
            CasperError::SigningError("Validator private key is not valid hexadecimal".into())
        })?;
        k256::SecretKey::from_slice(&decoded).map_err(|_| {
            CasperError::SigningError(
                "Validator private key is not a valid secp256k1 scalar".into(),
            )
        })?;
        let private_key = PrivateKey::from_bytes(&decoded);

        Ok(Self::new(&private_key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_identity_preserves_public_key_and_signing() {
        let (private_key, public_key) = Secp256k1.new_key_pair();
        let identity = ValidatorIdentity::from_private_key_with_logging(Some(&hex::encode(
            &private_key.bytes,
        )))
        .unwrap()
        .unwrap();
        assert_eq!(identity.public_key, public_key);
        let message = [42; 32];
        let signature = identity.signature(&message);
        assert!(Secp256k1.verify(&message, &signature.sig, &signature.public_key));
    }

    #[test]
    fn absent_key_preserves_read_only_identity() {
        assert!(ValidatorIdentity::from_private_key_with_logging(None)
            .unwrap()
            .is_none());
    }

    #[test]
    fn invalid_keys_return_errors_without_key_material() {
        for key in [
            String::new(),
            "1".repeat(63),
            "11".repeat(31),
            "11".repeat(33),
            "zz".repeat(32),
            "00".repeat(32),
            "ff".repeat(32),
        ] {
            let error = ValidatorIdentity::from_private_key_with_logging(Some(&key))
                .err()
                .expect("Invalid validator key must fail");
            assert!(matches!(error, CasperError::SigningError(_)));
            if !key.is_empty() {
                assert!(!error.to_string().contains(&key));
            }
        }
    }
}

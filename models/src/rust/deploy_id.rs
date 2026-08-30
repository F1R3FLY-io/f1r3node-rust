use std::fmt;

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize
)]
pub struct DeployIdV6([u8; Self::LENGTH]);

impl DeployIdV6 {
    pub const LENGTH: usize = 32;

    pub const fn as_array(&self) -> &[u8; Self::LENGTH] { &self.0 }

    pub const fn into_array(self) -> [u8; Self::LENGTH] { self.0 }
}

impl TryFrom<&[u8]> for DeployIdV6 {
    type Error = DeployIdLengthError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let actual = value.len();
        let bytes = value
            .try_into()
            .map_err(|_| DeployIdLengthError { actual })?;
        Ok(Self(bytes))
    }
}

impl TryFrom<Vec<u8>> for DeployIdV6 {
    type Error = DeployIdLengthError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> { Self::try_from(value.as_slice()) }
}

impl AsRef<[u8]> for DeployIdV6 {
    fn as_ref(&self) -> &[u8] { &self.0 }
}

impl From<DeployIdV6> for Vec<u8> {
    fn from(value: DeployIdV6) -> Self { value.0.to_vec() }
}

impl fmt::Display for DeployIdV6 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize
)]
pub struct LegacyDeploySignature(Vec<u8>);

impl LegacyDeploySignature {
    pub fn new(bytes: Vec<u8>) -> Self { Self(bytes) }

    pub fn as_bytes(&self) -> &[u8] { &self.0 }

    pub fn into_bytes(self) -> Vec<u8> { self.0 }
}

impl AsRef<[u8]> for LegacyDeploySignature {
    fn as_ref(&self) -> &[u8] { &self.0 }
}

impl From<Vec<u8>> for LegacyDeploySignature {
    fn from(value: Vec<u8>) -> Self { Self::new(value) }
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize
)]
pub enum DeployLookupId {
    Legacy(LegacyDeploySignature),
    V6(DeployIdV6),
}

impl DeployLookupId {
    pub fn from_protocol_bytes(
        protocol_version: i64,
        bytes: &[u8],
    ) -> Result<Self, DeployIdLengthError> {
        if protocol_version >= 6 {
            DeployIdV6::try_from(bytes).map(Self::V6)
        } else {
            Ok(Self::Legacy(LegacyDeploySignature::new(bytes.to_vec())))
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Legacy(signature) => signature.as_bytes(),
            Self::V6(deploy_id) => deploy_id.as_ref(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("protocol-v6 deploy identity must be 32 bytes, got {actual}")]
pub struct DeployIdLengthError {
    pub actual: usize,
}

#[cfg(test)]
mod tests {
    use super::{DeployLookupId, LegacyDeploySignature};

    #[test]
    fn protocol_version_selects_identity_semantics_without_length_inference() {
        let bytes = [7; 32];
        assert!(matches!(
            DeployLookupId::from_protocol_bytes(5, &bytes).expect("legacy identity"),
            DeployLookupId::Legacy(_)
        ));
        assert!(matches!(
            DeployLookupId::from_protocol_bytes(6, &bytes).expect("v6 identity"),
            DeployLookupId::V6(_)
        ));
    }

    #[test]
    fn protocol_v6_rejects_non_digest_identity_while_legacy_preserves_it() {
        let bytes = vec![1, 2, 3];
        assert_eq!(
            DeployLookupId::from_protocol_bytes(5, &bytes).expect("legacy identity"),
            DeployLookupId::Legacy(LegacyDeploySignature::new(bytes.clone()))
        );
        assert_eq!(
            DeployLookupId::from_protocol_bytes(6, &bytes)
                .expect_err("v6 identity length")
                .actual,
            bytes.len()
        );
    }
}

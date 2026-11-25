use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;

use super::evidence::EvidenceDigest;
use crate::rust::errors::CasperError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashRecord {
    pub evidence: EvidenceDigest,
    pub offender: Bytes,
}

pub trait SlashSource: Send + Sync {
    fn slashes(&self, block: &BlockMessage) -> Result<Vec<SlashRecord>, CasperError>;
}

pub struct NoSlashing;

impl SlashSource for NoSlashing {
    fn slashes(&self, _block: &BlockMessage) -> Result<Vec<SlashRecord>, CasperError> {
        Ok(Vec::new())
    }
}

pub fn require_no_slashes(records: &[SlashRecord]) -> Result<(), CasperError> {
    if records.is_empty() {
        Ok(())
    } else {
        Err(CasperError::RuntimeError(format!(
            "slash execution arrives in B6; this block carries {} slash records",
            records.len()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_records_are_refused_until_b6() {
        assert!(require_no_slashes(&[]).is_ok());
        let record = SlashRecord {
            evidence: [1; 32],
            offender: Bytes::from_static(b"key"),
        };
        let error = require_no_slashes(&[record]).unwrap_err();
        assert!(format!("{error:?}").contains("arrives in B6"));
    }
}

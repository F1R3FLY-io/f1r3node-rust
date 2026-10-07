use models::rust::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireLimits};

use super::{invalid, Blake2b256, CasperError};

const BUCKET_DOMAIN: &[u8] = b"f1r3node:prepaid-source-bucket:v1";
const BUCKET_KEY_DOMAIN: &[u8] = b"f1r3node:prepaid-source-bucket-key:v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrepaidReceiptBucketLimits {
    pub occurrences: usize,
    pub wire: PhloWireLimits,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrepaidReceiptBucket<'a> {
    source_hash: [u8; 32],
    receipts: Vec<&'a [u8]>,
}

impl<'a> PrepaidReceiptBucket<'a> {
    pub fn new(
        source_hash: [u8; 32],
        receipts: &[&'a [u8]],
        limits: PrepaidReceiptBucketLimits,
    ) -> Result<Self, CasperError> {
        validate_entries(receipts, limits)?;
        let mut ordered = Vec::new();
        ordered
            .try_reserve_exact(receipts.len())
            .map_err(|_| invalid("bucket allocation failed"))?;
        ordered.extend_from_slice(receipts);
        ordered.sort_unstable();
        Ok(Self {
            source_hash,
            receipts: ordered,
        })
    }

    pub fn source_hash(&self) -> &[u8; 32] { &self.source_hash }

    pub fn storage_key(&self) -> [u8; 32] { Self::key_for_source(&self.source_hash) }

    pub fn key_for_source(source_hash: &[u8; 32]) -> [u8; 32] {
        Blake2b256::hash_stream(|feed| {
            feed(BUCKET_KEY_DOMAIN);
            feed(source_hash);
        })
        .try_into()
        .expect("Blake2b-256 digest length")
    }

    pub fn receipts(&self) -> &[&'a [u8]] { &self.receipts }

    pub fn remove(&mut self, index: usize) -> Result<&'a [u8], CasperError> {
        if index >= self.receipts.len() {
            return Err(invalid("bucket occurrence is out of range"));
        }
        Ok(self.receipts.remove(index))
    }

    pub fn check_occurrences(
        &self,
        source_hash: &[u8; 32],
        count: usize,
    ) -> Result<(), CasperError> {
        if self.source_hash != *source_hash || self.receipts.len() != count {
            return Err(invalid(
                "bucket differs from the captured source or occurrence count",
            ));
        }
        Ok(())
    }

    pub fn encode(&self, limits: PrepaidReceiptBucketLimits) -> Result<Vec<u8>, CasperError> {
        validate_entries(&self.receipts, limits)?;
        let mut out = PhloWireEncoder::new(limits.wire);
        out.bytes(BUCKET_DOMAIN)
            .map_err(|e| invalid(&e.to_string()))?;
        out.bytes(&self.source_hash)
            .map_err(|e| invalid(&e.to_string()))?;
        out.u64(u64::try_from(self.receipts.len()).map_err(|_| invalid("bucket count overflow"))?)
            .map_err(|e| invalid(&e.to_string()))?;
        for receipt in &self.receipts {
            out.bytes(receipt).map_err(|e| invalid(&e.to_string()))?;
        }
        Ok(out.into_bytes())
    }

    pub fn decode(
        bytes: &'a [u8],
        limits: PrepaidReceiptBucketLimits,
    ) -> Result<Self, CasperError> {
        let mut input =
            PhloWireDecoder::new(bytes, limits.wire).map_err(|e| invalid(&e.to_string()))?;
        if input.bytes().map_err(|e| invalid(&e.to_string()))? != BUCKET_DOMAIN {
            return Err(invalid("unsupported bucket domain"));
        }
        let source_hash = input
            .bytes()
            .map_err(|e| invalid(&e.to_string()))?
            .try_into()
            .map_err(|_| invalid("bucket source must be 32 bytes"))?;
        let count = usize::try_from(input.u64().map_err(|e| invalid(&e.to_string()))?)
            .map_err(|_| invalid("bucket count overflow"))?;
        if count > limits.occurrences || count > input.remaining().len() / 9 {
            return Err(invalid("bucket occurrence limit exceeded"));
        }
        let mut receipts: Vec<&'a [u8]> = Vec::new();
        receipts
            .try_reserve_exact(count)
            .map_err(|_| invalid("bucket allocation failed"))?;
        for _ in 0..count {
            let receipt = input.bytes().map_err(|e| invalid(&e.to_string()))?;
            if receipt.is_empty() || receipts.last().is_some_and(|previous| *previous > receipt) {
                return Err(invalid(
                    "bucket receipts must be nonempty and canonically ordered",
                ));
            }
            receipts.push(receipt);
        }
        input.finish().map_err(|e| invalid(&e.to_string()))?;
        Ok(Self {
            source_hash,
            receipts,
        })
    }
}

fn validate_entries(
    receipts: &[&[u8]],
    limits: PrepaidReceiptBucketLimits,
) -> Result<(), CasperError> {
    if receipts.len() > limits.occurrences {
        return Err(invalid("bucket occurrence limit exceeded"));
    }
    if BUCKET_DOMAIN.len() > limits.wire.field_bytes || 32 > limits.wire.field_bytes {
        return Err(invalid("bucket field limit exceeded"));
    }
    let mut size = BUCKET_DOMAIN.len() + 32 + 24;
    for receipt in receipts {
        if receipt.is_empty() || receipt.len() > limits.wire.field_bytes {
            return Err(invalid("empty or oversized bucket receipt"));
        }
        size = receipt
            .len()
            .checked_add(8)
            .and_then(|entry| size.checked_add(entry))
            .ok_or_else(|| invalid("bucket byte size overflow"))?;
    }
    if size > limits.wire.total_bytes {
        return Err(invalid("bucket byte limit exceeded"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

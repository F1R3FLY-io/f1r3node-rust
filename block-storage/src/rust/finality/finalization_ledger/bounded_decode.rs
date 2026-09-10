use models::rust::casper::protocol::casper_message::FinalizationCertificate;
use shared::rust::store::key_value_store::KvStoreError;

use super::{FinalizationLedgerKey, FinalizationLedgerValue};

const HASH_BYTES: usize = models::rust::block_hash::LENGTH;
const VALIDATOR_BYTES: usize = models::rust::validator::LENGTH;
const ENCODED_HASH_BYTES: usize = 8 + HASH_BYTES;
const ENCODED_LATEST_BYTES: usize = 8 + VALIDATOR_BYTES + ENCODED_HASH_BYTES;
const MAX_ROUND_BYTES: usize =
    272 + ENCODED_HASH_BYTES * FinalizationCertificate::MAX_FINALIZED_BLOCKS;
const MAX_WITNESS_BYTES: usize = 432
    + FinalizationCertificate::MAX_SHARD_ID_BYTES
    + ENCODED_LATEST_BYTES * FinalizationCertificate::MAX_EXACT_LATEST_MESSAGES
    + ENCODED_HASH_BYTES * FinalizationCertificate::MAX_SUPPORTING_BLOCKS
    + ENCODED_HASH_BYTES * FinalizationCertificate::MAX_FINALIZED_BLOCKS;

fn invalid(reason: &str) -> KvStoreError {
    KvStoreError::SerializationError(format!("invalid finalization ledger encoding: {reason}"))
}

struct Layout<'a> {
    remaining: &'a [u8],
}

impl<'a> Layout<'a> {
    fn tag(&mut self) -> Result<u32, KvStoreError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], KvStoreError> {
        let (taken, rest) = self
            .remaining
            .split_at_checked(length)
            .ok_or_else(|| invalid("truncated field"))?;
        self.remaining = rest;
        Ok(taken)
    }

    fn length(&mut self) -> Result<usize, KvStoreError> {
        let encoded: [u8; 8] = self.take(8)?.try_into().unwrap();
        usize::try_from(u64::from_le_bytes(encoded))
            .map_err(|_| invalid("length exceeds the addressable range"))
    }

    fn fixed_bytes(&mut self, required: usize) -> Result<(), KvStoreError> {
        if self.length()? != required {
            return Err(invalid("incorrect hash or validator length"));
        }
        self.take(required)?;
        Ok(())
    }

    fn hash(&mut self) -> Result<(), KvStoreError> { self.fixed_bytes(HASH_BYTES) }

    fn string(&mut self, limit: usize) -> Result<(), KvStoreError> {
        let length = self.length()?;
        if length > limit {
            return Err(invalid("string exceeds its field limit"));
        }
        std::str::from_utf8(self.take(length)?).map_err(|_| invalid("string is not UTF-8"))?;
        Ok(())
    }

    fn collection(&mut self, limit: usize, width: usize) -> Result<usize, KvStoreError> {
        let count = self.length()?;
        if count > limit {
            return Err(invalid("collection exceeds its field limit"));
        }
        let bytes = count
            .checked_mul(width)
            .ok_or_else(|| invalid("collection size overflows"))?;
        if bytes > self.remaining.len() {
            return Err(invalid("collection exceeds available bytes"));
        }
        Ok(count)
    }

    fn hashes(&mut self, limit: usize) -> Result<(), KvStoreError> {
        let count = self.collection(limit, ENCODED_HASH_BYTES)?;
        for _ in 0..count {
            self.hash()?;
        }
        Ok(())
    }
}

fn key_tag(key: &FinalizationLedgerKey) -> u32 {
    match key {
        FinalizationLedgerKey::Head => 0,
        FinalizationLedgerKey::Round(_) => 1,
        FinalizationLedgerKey::Effect(_) => 2,
        FinalizationLedgerKey::ProjectionCursor => 3,
        FinalizationLedgerKey::EffectsCursor => 4,
        FinalizationLedgerKey::EffectsComplete(_) => 5,
        FinalizationLedgerKey::EffectsCompactionCursor => 6,
        FinalizationLedgerKey::Genesis => 7,
        FinalizationLedgerKey::Witness(_) => 8,
        FinalizationLedgerKey::SettledRecoveryCharge(_) => 9,
        FinalizationLedgerKey::SettledRecoveryUsage(_) => 10,
    }
}

fn inspect_key(encoded: &[u8]) -> Result<u32, KvStoreError> {
    let mut layout = Layout { remaining: encoded };
    let tag = layout.tag()?;
    match tag {
        0 | 3 | 4 | 6 | 7 => {}
        1 | 5 => {
            layout.take(8)?;
        }
        2 => {
            layout.take(8)?;
            layout.hash()?;
            if layout.tag()? > 3 {
                return Err(invalid("unknown effect kind"));
            }
        }
        8..=10 => {
            layout.hash()?;
        }
        _ => return Err(invalid("unknown key variant")),
    }
    if !layout.remaining.is_empty() {
        return Err(invalid("key contains trailing bytes"));
    }
    Ok(tag)
}

fn inspect_value(encoded: &[u8]) -> Result<u32, KvStoreError> {
    let mut layout = Layout { remaining: encoded };
    let tag = layout.tag()?;
    let max_bytes = match tag {
        0 => 140,
        1 => MAX_ROUND_BYTES,
        2 | 5 => 4,
        3 | 4 | 6 | 10 => 12,
        7 => 132,
        8 => MAX_WITNESS_BYTES,
        9 => encoded.len(),
        _ => return Err(invalid("unknown value variant")),
    };
    if encoded.len() > max_bytes {
        return Err(invalid("row exceeds its encoded size limit"));
    }
    match tag {
        0 => {
            layout.take(8)?;
            layout.hash()?;
            layout.take(8)?;
            layout.hash()?;
            layout.hash()?;
        }
        1 => {
            layout.take(8)?;
            for _ in 0..3 {
                layout.hash()?;
            }
            layout.take(12)?;
            layout.hashes(FinalizationCertificate::MAX_FINALIZED_BLOCKS)?;
            for _ in 0..3 {
                layout.hash()?;
            }
        }
        7 => {
            layout.hash()?;
            layout.take(8)?;
            layout.hash()?;
            layout.hash()?;
        }
        8 => {
            layout.take(12)?;
            layout.string(FinalizationCertificate::MAX_SHARD_ID_BYTES)?;
            for _ in 0..6 {
                layout.hash()?;
            }
            layout.take(8)?;
            layout.hash()?;
            layout.take(16)?;
            let latest = layout.collection(
                FinalizationCertificate::MAX_EXACT_LATEST_MESSAGES,
                ENCODED_LATEST_BYTES,
            )?;
            for _ in 0..latest {
                layout.fixed_bytes(VALIDATOR_BYTES)?;
                layout.hash()?;
            }
            layout.hashes(FinalizationCertificate::MAX_SUPPORTING_BLOCKS)?;
            layout.hash()?;
            layout.hashes(FinalizationCertificate::MAX_FINALIZED_BLOCKS)?;
            layout.hash()?;
        }
        9 => {
            layout.take(16)?;
            layout.string(layout.remaining.len())?;
            layout.take(8)?;
            for _ in 0..4 {
                layout.hash()?;
            }
            layout.fixed_bytes(VALIDATOR_BYTES)?;
            layout.take(8)?;
        }
        3 | 4 | 6 | 10 => {
            layout.take(8)?;
        }
        _ => {}
    }
    if !layout.remaining.is_empty() {
        return Err(invalid("row contains trailing bytes"));
    }
    Ok(tag)
}

pub(super) fn recovery_row(
    key: &[u8],
    value: &[u8],
) -> Result<Option<(FinalizationLedgerKey, FinalizationLedgerValue)>, KvStoreError> {
    let key_tag = inspect_key(key)?;
    let value_tag = inspect_value(value)?;
    if key_tag >= 9 || value_tag >= 9 {
        if key_tag != value_tag {
            return Err(invalid("settled recovery key and value variants differ"));
        }
        Ok(Some((
            bincode::deserialize(key)?,
            bincode::deserialize(value)?,
        )))
    } else {
        Ok(None)
    }
}

pub(super) fn decode(
    key: &FinalizationLedgerKey,
    encoded: &[u8],
) -> Result<FinalizationLedgerValue, KvStoreError> {
    if inspect_value(encoded)? != key_tag(key) {
        return Err(invalid("key and value variants differ"));
    }
    Ok(bincode::deserialize(encoded)?)
}

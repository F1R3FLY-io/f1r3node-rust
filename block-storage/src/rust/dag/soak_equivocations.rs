use crypto::rust::hash::sha_256::Sha256Hasher;
use models::rust::validator::Validator;
use shared::rust::store::soak_snapshot::{
    encode_length_prefixed, SnapshotError, TransactionIdentity,
};

pub const MAX_EQUIVOCATION_ROWS: usize = 4096;
pub const EQUIVOCATION_SCOPE: &str = "batch-e-captured-equivocations-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquivocationInput {
    pub equivocator: Validator,
    pub base_sequence_number: i32,
    pub detected_hash_count: usize,
    pub raw_value_digest: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquivocationSnapshot {
    rows: Vec<EquivocationInput>,
    digest: [u8; 32],
    insertion_generation: u64,
    raw_bytes: usize,
    transactions: Vec<TransactionIdentity>,
}

pub fn validate_row_limit(limit: usize) -> Result<(), SnapshotError> {
    if !(1..=MAX_EQUIVOCATION_ROWS).contains(&limit) {
        return Err(SnapshotError::InvalidLimits(
            "equivocation rows must be in 1..=4096".into(),
        ));
    }
    Ok(())
}

fn malformed() -> SnapshotError { SnapshotError::Malformed("equivocation row".into()) }

fn length(bytes: &[u8], offset: &mut usize) -> Result<usize, SnapshotError> {
    let end = offset.checked_add(8).ok_or_else(malformed)?;
    let encoded = bytes.get(*offset..end).ok_or_else(malformed)?;
    *offset = end;
    usize::try_from(u64::from_le_bytes(
        encoded.try_into().map_err(|_| malformed())?,
    ))
    .map_err(|_| malformed())
}

pub(crate) fn decode_row(
    key: &[u8],
    value: &[u8],
    max_hashes: usize,
) -> Result<EquivocationInput, SnapshotError> {
    let mut position = 0;
    let validator_length = length(key, &mut position)?;
    let validator_end = position
        .checked_add(validator_length)
        .ok_or_else(malformed)?;
    let sequence_end = validator_end.checked_add(4).ok_or_else(malformed)?;
    if sequence_end != key.len() {
        return Err(malformed());
    }
    let validator = key.get(position..validator_end).ok_or_else(malformed)?;
    let sequence = i32::from_le_bytes(
        key[validator_end..sequence_end]
            .try_into()
            .map_err(|_| malformed())?,
    );
    position = 0;
    let count = length(value, &mut position)?;
    if count > max_hashes {
        return Err(SnapshotError::LimitExceeded {
            kind: "equivocation hashes",
            limit: max_hashes,
            observed: count,
        });
    }
    if count > value.len().saturating_sub(position) / 8 {
        return Err(malformed());
    }
    let mut previous: Option<&[u8]> = None;
    for _ in 0..count {
        let size = length(value, &mut position)?;
        let end = position.checked_add(size).ok_or_else(malformed)?;
        let hash = value.get(position..end).ok_or_else(malformed)?;
        if previous.is_some_and(|prior| prior >= hash) {
            return Err(malformed());
        }
        previous = Some(hash);
        position = end;
    }
    if position != value.len() {
        return Err(malformed());
    }
    let raw = encode_length_prefixed(value);
    Ok(EquivocationInput {
        equivocator: Validator::copy_from_slice(validator),
        base_sequence_number: sequence,
        detected_hash_count: count,
        raw_value_digest: Sha256Hasher::hash(raw)
            .try_into()
            .map_err(|_| malformed())?,
    })
}

impl EquivocationSnapshot {
    pub(crate) fn seal(
        mut rows: Vec<EquivocationInput>,
        raw_bytes: usize,
        insertion_generation: u64,
        transactions: Vec<TransactionIdentity>,
    ) -> Result<Self, SnapshotError> {
        rows.sort_by(|a, b| {
            (&a.equivocator, a.base_sequence_number).cmp(&(&b.equivocator, b.base_sequence_number))
        });
        if rows.windows(2).any(|pair| {
            pair[0].equivocator == pair[1].equivocator
                && pair[0].base_sequence_number == pair[1].base_sequence_number
        }) {
            return Err(malformed());
        }
        let mut canonical = EQUIVOCATION_SCOPE.as_bytes().to_vec();
        canonical.extend_from_slice(&insertion_generation.to_be_bytes());
        canonical.extend_from_slice(&(raw_bytes as u64).to_be_bytes());
        canonical.extend_from_slice(&(rows.len() as u64).to_be_bytes());
        for row in &rows {
            canonical.extend_from_slice(&(row.equivocator.len() as u64).to_be_bytes());
            canonical.extend_from_slice(&row.equivocator);
            canonical.extend_from_slice(&row.base_sequence_number.to_be_bytes());
            canonical.extend_from_slice(&(row.detected_hash_count as u64).to_be_bytes());
            canonical.extend_from_slice(&row.raw_value_digest);
        }
        let digest = Sha256Hasher::hash(canonical)
            .try_into()
            .map_err(|_| malformed())?;
        Ok(Self {
            rows,
            digest,
            insertion_generation,
            raw_bytes,
            transactions,
        })
    }

    pub fn raw_bytes(&self) -> usize { self.raw_bytes }

    pub fn rows(&self) -> &[EquivocationInput] { &self.rows }
    pub fn digest(&self) -> [u8; 32] { self.digest }
    pub fn insertion_generation(&self) -> u64 { self.insertion_generation }
    pub fn transactions(&self) -> &[TransactionIdentity] { &self.transactions }
}

use super::super::canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, FORMAT_VERSION_V1, OBJECT_DIGEST_LEN,
    OBJECT_DOMAIN_LEN, OBJECT_ENVELOPE_HEADER_LEN, ObjectDigest, ObjectRefV1, ReadCursor,
    parse_envelope, write_envelope_header, write_u64,
};
use super::super::mutation::StateVersionV1;
use super::{RetainedStateV1, STATE_ENTRY_LEN};

pub(crate) const RETENTION_ANCHOR_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceDAG.RetA\0\0";
pub(crate) const RETENTION_RADIX_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceDAG.RetR\0\0";
pub(crate) const GENERATION_CHECKPOINT_V2_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceDAG.Chkp2\0";
pub(crate) const RETENTION_KEY_LEN: usize = 8 + OBJECT_DIGEST_LEN + 8;
pub(crate) const EMPTY_RETENTION_RADIX_NODE_LEN: usize = 50;
pub(crate) const EMPTY_RETENTION_RADIX_NODE: [u8; EMPTY_RETENTION_RADIX_NODE_LEN] =
    empty_retention_radix_node();
pub(crate) const RETENTION_ANCHOR_CANONICAL_LEN: u64 = OBJECT_ENVELOPE_HEADER_LEN + ANCHOR_BODY_LEN;

pub(crate) const RETENTION_METADATA_MIN_OBJECTS: u64 = 2;
pub(crate) const RETENTION_METADATA_MIN_CANONICAL_BYTES: u64 =
    RETENTION_ANCHOR_CANONICAL_LEN + EMPTY_RETENTION_RADIX_NODE_LEN as u64;

const EMPTY_RADIX_BODY_LEN: u64 = 8 + 1 + 1 + 8;
const ANCHOR_BODY_LEN: u64 = STATE_ENTRY_LEN + 8 + OBJECT_DIGEST_LEN as u64 + 8;

const _: () = assert!(
    EMPTY_RETENTION_RADIX_NODE_LEN as u64 == OBJECT_ENVELOPE_HEADER_LEN + EMPTY_RADIX_BODY_LEN
);
const _: () = assert!(RETENTION_ANCHOR_CANONICAL_LEN == 136);
const _: () = assert!(RETENTION_METADATA_MIN_CANONICAL_BYTES == 186);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetentionFormat {
    V1,
    V2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RetentionReader {
    V1Only,
    Dual,
}

impl RetentionReader {
    pub(crate) const fn admits(self, format: RetentionFormat) -> bool {
        matches!((self, format), (_, RetentionFormat::V1) | (Self::Dual, RetentionFormat::V2))
    }
}

const fn empty_retention_radix_node() -> [u8; EMPTY_RETENTION_RADIX_NODE_LEN] {
    let mut bytes = [0_u8; EMPTY_RETENTION_RADIX_NODE_LEN];
    let version = FORMAT_VERSION_V1.to_le_bytes();
    let body_len = EMPTY_RADIX_BODY_LEN.to_le_bytes();
    let mut index = 0;
    while index < OBJECT_DOMAIN_LEN {
        bytes[index] = RETENTION_RADIX_DOMAIN[index];
        index += 1;
    }
    let mut offset = 0;
    while offset < 8 {
        bytes[OBJECT_DOMAIN_LEN + offset] = version[offset];
        bytes[OBJECT_DOMAIN_LEN + 8 + offset] = body_len[offset];
        offset += 1;
    }
    bytes
}

pub(crate) fn empty_retention_radix_ref() -> ObjectRefV1 {
    ObjectRefV1::new(
        ObjectDigest::of_canonical_bytes(&EMPTY_RETENTION_RADIX_NODE),
        EMPTY_RETENTION_RADIX_NODE_LEN as u64,
    )
    .expect("the empty retention radix node includes its envelope")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct RetentionKeyV2([u8; RETENTION_KEY_LEN]);

impl RetentionKeyV2 {
    pub(crate) fn new(state: StateVersionV1) -> Self {
        let mut key = [0_u8; RETENTION_KEY_LEN];
        key[..8].copy_from_slice(&state.generation().to_be_bytes());
        key[8..8 + OBJECT_DIGEST_LEN].copy_from_slice(state.manifest().digest().as_bytes());
        key[8 + OBJECT_DIGEST_LEN..].copy_from_slice(&state.manifest().encoded_len().to_be_bytes());
        Self(key)
    }

    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, CanonicalFormatError> {
        let key: [u8; RETENTION_KEY_LEN] =
            bytes
                .try_into()
                .map_err(|_| CanonicalFormatError::LengthOutOfRange {
                    field: "retention key",
                    value: bytes.len() as u64,
                })?;
        let (_, digest, encoded_len) = key_fields(&key);
        ObjectRefV1::new(ObjectDigest::from_array(digest), encoded_len)?;
        Ok(Self(key))
    }

    pub(crate) fn state(self) -> StateVersionV1 {
        let (generation, digest, encoded_len) = key_fields(&self.0);
        let manifest = ObjectRefV1::new(ObjectDigest::from_array(digest), encoded_len)
            .expect("constructed and parsed retention keys carry an envelope length");
        StateVersionV1::new(generation, manifest)
    }

    pub(crate) const fn as_bytes(&self) -> &[u8; RETENTION_KEY_LEN] { &self.0 }
}

fn key_fields(key: &[u8; RETENTION_KEY_LEN]) -> (u64, [u8; OBJECT_DIGEST_LEN], u64) {
    let mut generation = [0_u8; 8];
    let mut digest = [0_u8; OBJECT_DIGEST_LEN];
    let mut encoded_len = [0_u8; 8];
    generation.copy_from_slice(&key[..8]);
    digest.copy_from_slice(&key[8..8 + OBJECT_DIGEST_LEN]);
    encoded_len.copy_from_slice(&key[8 + OBJECT_DIGEST_LEN..]);
    (u64::from_be_bytes(generation), digest, u64::from_be_bytes(encoded_len))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RetentionAnchorV2 {
    current: RetainedStateV1,
    historical_count: u64,
    historical_root: ObjectRefV1,
}

impl RetentionAnchorV2 {
    pub(crate) fn new(
        current: RetainedStateV1,
        historical_count: u64,
        historical_root: ObjectRefV1,
    ) -> Result<Self, CanonicalFormatError> {
        if (historical_count == 0) != (historical_root == empty_retention_radix_ref()) {
            return Err(CanonicalFormatError::RetentionHistoryRootMismatch {
                count: historical_count,
            });
        }
        Ok(Self {
            current,
            historical_count,
            historical_root,
        })
    }

    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, CanonicalFormatError> {
        let body = parse_envelope(bytes, RETENTION_ANCHOR_DOMAIN, "RSpace retention anchor")?;
        let mut cursor = ReadCursor::new(body);
        let current = RetainedStateV1::parse(&mut cursor)?;
        let historical_count = cursor.read_u64()?;
        let historical_root = ObjectRefV1::parse(&mut cursor)?;
        cursor.finish()?;
        Self::new(current, historical_count, historical_root)
    }

    pub(crate) const fn current(self) -> RetainedStateV1 { self.current }

    pub(crate) const fn historical_count(self) -> u64 { self.historical_count }

    pub(crate) const fn historical_root(self) -> ObjectRefV1 { self.historical_root }

    pub(crate) fn object_ref(self) -> ObjectRefV1 {
        ObjectRefV1::new(self.canonical_digest(), RETENTION_ANCHOR_CANONICAL_LEN)
            .expect("the retention anchor length includes its envelope")
    }
}

impl CanonicalEncode for RetentionAnchorV2 {
    fn canonical_len(&self) -> u64 { RETENTION_ANCHOR_CANONICAL_LEN }

    fn write_canonical<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        write_envelope_header(sink, RETENTION_ANCHOR_DOMAIN, ANCHOR_BODY_LEN)?;
        self.current.write(sink)?;
        write_u64(sink, self.historical_count)?;
        self.historical_root.write(sink)
    }
}

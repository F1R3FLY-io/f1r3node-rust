//! Canonical structural channel images for PathMap-backed Rholang `Par` values.
//!
//! `ChannelImageV2` is the logical, storage-independent image of the exact
//! identity used by `Par::{Eq, Ord, Hash}`: the authoritative RPB3 component
//! Bag followed by root `connective_used`. Top-level `locally_free` is a
//! normalization summary and is deliberately absent. Encoding streams from
//! borrowed ParBag zipper endpoints; it never serializes ACT topology, decodes
//! a component, constructs `Vec<Par>`, projects entries, or sorts. External
//! admission uses the exact RPB3 validator so a forged score/body pair cannot
//! enter a resident Bag.
//!
//! ```text
//! ChannelImageV2 :=
//!     "RCI" 02
//!     endpoint*
//!     00 00
//!     distinct-count:u64-BE
//!     flags:u8
//!
//! endpoint := escape(rpb3-relative-key) 00 00 multiplicity:u64-BE
//! escape(00) := 00 FF
//! flags.bit0 := connective_used
//! ```
//!
//! RPB3-relative keys omit only the common `"RPB" 03` domain. Their first
//! byte is the nonzero component-family tag. Zero escaping is
//! lexicographically order preserving, fixed-width big-endian multiplicity
//! preserves numeric order, and the sequence terminator sorts before the next
//! family tag. Raw comparison of validated images therefore equals
//! `Par::Ord`.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use rspace_plus_plus::rspace::bag::{Bag, Multiplicity, SortedUniqueBagError};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use super::par_bag::{
    validate_canonical_key, CanonicalKeyMetadata, CanonicalParBagAccumulator, ParBagCodecError,
    PAR_BAG_KEY_DOMAIN,
};
use crate::rhoapi::Par;

/// Domain and version prefix of every canonical `ChannelImageV2`.
pub const CHANNEL_IMAGE_V2_DOMAIN: [u8; 4] = *b"RCI\x02";

const ESCAPE: u8 = 0;
const ESCAPED_ZERO: u8 = 0xFF;
const FRAME_END: u8 = 0;
const CONNECTIVE_USED_FLAG: u8 = 1;
const KNOWN_FLAGS: u8 = CONNECTIVE_USED_FLAG;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelImageError {
    TruncatedDomain,
    InvalidDomain { found: [u8; 4] },
    TruncatedFrame,
    InvalidEscape(u8),
    NonAscendingEndpoint,
    TruncatedMultiplicity,
    ZeroMultiplicity,
    LengthOverflow,
    DistinctCountOverflow,
    TruncatedDistinctCount,
    DistinctCountMismatch { declared: u64, observed: u64 },
    TruncatedFlags,
    InvalidFlags(u8),
    TrailingBytes { consumed: usize, available: usize },
    Component(ParBagCodecError),
    BagBuild(SortedUniqueBagError),
}

impl fmt::Display for ChannelImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedDomain => formatter.write_str("truncated ChannelImageV2 domain"),
            Self::InvalidDomain { found } => {
                write!(
                    formatter,
                    "invalid ChannelImageV2 domain/version: {found:02x?}"
                )
            }
            Self::TruncatedFrame => formatter.write_str("truncated ChannelImageV2 endpoint frame"),
            Self::InvalidEscape(byte) => {
                write!(formatter, "invalid ChannelImageV2 escape {byte:#04x}")
            }
            Self::NonAscendingEndpoint => formatter.write_str(
                "ChannelImageV2 RPB3 endpoints are duplicated or not strictly ascending",
            ),
            Self::TruncatedMultiplicity => {
                formatter.write_str("truncated ChannelImageV2 multiplicity")
            }
            Self::ZeroMultiplicity => {
                formatter.write_str("ChannelImageV2 multiplicity must be nonzero")
            }
            Self::LengthOverflow => {
                formatter.write_str("ChannelImageV2 endpoint length exceeds u64::MAX")
            }
            Self::DistinctCountOverflow => {
                formatter.write_str("ChannelImageV2 distinct endpoint count exceeds u64::MAX")
            }
            Self::TruncatedDistinctCount => {
                formatter.write_str("truncated ChannelImageV2 distinct endpoint count")
            }
            Self::DistinctCountMismatch { declared, observed } => write!(
                formatter,
                "ChannelImageV2 declares {declared} distinct endpoint(s), observed {observed}",
            ),
            Self::TruncatedFlags => formatter.write_str("truncated ChannelImageV2 root flags"),
            Self::InvalidFlags(flags) => {
                write!(formatter, "invalid ChannelImageV2 root flags {flags:#04x}")
            }
            Self::TrailingBytes {
                consumed,
                available,
            } => write!(
                formatter,
                "ChannelImageV2 consumed {consumed} of {available} bytes",
            ),
            Self::Component(error) => {
                write!(formatter, "invalid ChannelImageV2 RPB3 endpoint: {error}")
            }
            Self::BagBuild(error) => {
                write!(formatter, "invalid ChannelImageV2 Bag stream: {error}")
            }
        }
    }
}

impl std::error::Error for ChannelImageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Component(error) => Some(error),
            Self::BagBuild(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ParBagCodecError> for ChannelImageError {
    fn from(error: ParBagCodecError) -> Self { Self::Component(error) }
}

impl From<SortedUniqueBagError> for ChannelImageError {
    fn from(error: SortedUniqueBagError) -> Self { Self::BagBuild(error) }
}

/// Owned, already-validated ChannelImageV2 bytes.
#[derive(Clone)]
pub struct ChannelImageV2(Arc<[u8]>);

impl ChannelImageV2 {
    /// Stream the semantic image of `par` directly from authoritative RPB3
    /// endpoints. Root `locally_free` is intentionally not consulted.
    pub fn from_par(par: &Par) -> Result<Self, ChannelImageError> {
        let distinct = u64::try_from(par.distinct_len())
            .map_err(|_| ChannelImageError::DistinctCountOverflow)?;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&CHANNEL_IMAGE_V2_DOMAIN);

        let mut observed = 0u64;
        par.components().try_visit_canonical_key_parts(
            |tag, score, protobuf, multiplicity| -> Result<(), ChannelImageError> {
                observed = observed
                    .checked_add(1)
                    .ok_or(ChannelImageError::DistinctCountOverflow)?;
                append_escaped_byte(&mut bytes, tag.byte());
                append_escaped_slice(&mut bytes, score);
                append_escaped_slice(&mut bytes, protobuf);
                let score_len =
                    u64::try_from(score.len()).map_err(|_| ChannelImageError::LengthOverflow)?;
                append_escaped_slice(&mut bytes, &score_len.to_be_bytes());
                bytes.extend_from_slice(&[ESCAPE, FRAME_END]);
                bytes.extend_from_slice(&multiplicity.get().to_be_bytes());
                Ok(())
            },
        )?;
        if observed != distinct {
            return Err(ChannelImageError::DistinctCountMismatch {
                declared: distinct,
                observed,
            });
        }
        bytes.extend_from_slice(&[ESCAPE, FRAME_END]);
        bytes.extend_from_slice(&distinct.to_be_bytes());
        bytes.push(u8::from(par.connective_used()));
        Ok(Self(Arc::from(bytes)))
    }

    /// Admit externally supplied bytes only after the complete scalar cursor,
    /// RPB3 validator, strict-order, count, multiplicity, and flag checks pass.
    pub fn try_from_bytes(bytes: impl AsRef<[u8]>) -> Result<Self, ChannelImageError> {
        let bytes = bytes.as_ref();
        let (_, consumed) = parse_prefix(bytes, false)?;
        if consumed != bytes.len() {
            return Err(ChannelImageError::TrailingBytes {
                consumed,
                available: bytes.len(),
            });
        }
        Ok(Self(Arc::from(bytes)))
    }

    /// Validate an external image and reconstruct its identity-normalized
    /// `Par` in the same cursor pass.
    ///
    /// Persistence and state-sync readers should prefer this operation over a
    /// separate `try_from_bytes` followed by `decode_par`, which would
    /// deliberately validate twice because `ChannelImageV2` stores no shadow
    /// decoded representation.
    pub fn decode_bytes(bytes: impl AsRef<[u8]>) -> Result<(Self, Par), ChannelImageError> {
        let bytes = bytes.as_ref();
        let (par, consumed) = parse_prefix(bytes, true)?;
        if consumed != bytes.len() {
            return Err(ChannelImageError::TrailingBytes {
                consumed,
                available: bytes.len(),
            });
        }
        Ok((
            Self(Arc::from(bytes)),
            par.expect("the decode mode constructs one Par"),
        ))
    }

    pub fn as_bytes(&self) -> &[u8] { &self.0 }

    pub fn as_validated(&self) -> ValidatedChannelImage<'_> {
        ValidatedChannelImage { bytes: &self.0 }
    }

    pub fn decode_par(&self) -> Result<Par, ChannelImageError> {
        decode_validated_channel_image(self.as_bytes())
    }

    /// BLAKE2b-256 reference for content-addressed catalogs. The RCI2 domain
    /// prefix is inside the hashed bytes. The digest is never a structural-key
    /// substitute; every collision path must compare `as_bytes()`.
    pub fn digest(&self) -> Blake2b256Hash { Blake2b256Hash::new(self.as_bytes()) }
}

impl AsRef<[u8]> for ChannelImageV2 {
    fn as_ref(&self) -> &[u8] { self.as_bytes() }
}

impl fmt::Debug for ChannelImageV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChannelImageV2")
            .field("encoded_len", &self.0.len())
            .field("digest", &self.digest())
            .finish()
    }
}

impl PartialEq for ChannelImageV2 {
    fn eq(&self, other: &Self) -> bool { self.0 == other.0 }
}

impl Eq for ChannelImageV2 {}

impl PartialOrd for ChannelImageV2 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for ChannelImageV2 {
    fn cmp(&self, other: &Self) -> Ordering { self.0.cmp(&other.0) }
}

impl Hash for ChannelImageV2 {
    fn hash<H: Hasher>(&self, state: &mut H) { self.0.hash(state) }
}

/// Borrowed proof that one byte slice passed complete ChannelImageV2
/// validation.
#[derive(Clone, Copy, Debug)]
pub struct ValidatedChannelImage<'a> {
    bytes: &'a [u8],
}

impl<'a> ValidatedChannelImage<'a> {
    pub fn validate_exact(bytes: &'a [u8]) -> Result<Self, ChannelImageError> {
        let (_, consumed) = parse_prefix(bytes, false)?;
        if consumed != bytes.len() {
            return Err(ChannelImageError::TrailingBytes {
                consumed,
                available: bytes.len(),
            });
        }
        Ok(Self { bytes })
    }

    /// Validate and take one ChannelImageV2 from a concatenated byte stream.
    pub fn take_prefix(bytes: &'a [u8]) -> Result<(Self, usize), ChannelImageError> {
        let (_, consumed) = parse_prefix(bytes, false)?;
        Ok((
            Self {
                bytes: &bytes[..consumed],
            },
            consumed,
        ))
    }

    pub fn as_bytes(self) -> &'a [u8] { self.bytes }

    /// Decode to the identity-normalized representative. Top-level
    /// `locally_free` is empty because it is not part of `Par` equality.
    pub fn decode_par(self) -> Result<Par, ChannelImageError> {
        decode_validated_channel_image(self.bytes)
    }
}

fn decode_validated_channel_image(bytes: &[u8]) -> Result<Par, ChannelImageError> {
    let (par, consumed) = parse_prefix(bytes, true)?;
    debug_assert_eq!(consumed, bytes.len());
    Ok(par.expect("the decode mode constructs one Par"))
}

fn parse_prefix(bytes: &[u8], build: bool) -> Result<(Option<Par>, usize), ChannelImageError> {
    if bytes.len() < CHANNEL_IMAGE_V2_DOMAIN.len() {
        return Err(ChannelImageError::TruncatedDomain);
    }
    let mut found = [0u8; CHANNEL_IMAGE_V2_DOMAIN.len()];
    found.copy_from_slice(&bytes[..CHANNEL_IMAGE_V2_DOMAIN.len()]);
    if found != CHANNEL_IMAGE_V2_DOMAIN {
        return Err(ChannelImageError::InvalidDomain { found });
    }

    let mut cursor = CHANNEL_IMAGE_V2_DOMAIN.len();
    let (bag, observed) = if build {
        let mut accumulator = CanonicalParBagAccumulator::new();
        let mut observed = 0u64;
        let components = Bag::try_from_sorted_unique(|writer| {
            observed = parse_endpoints(bytes, &mut cursor, |key, multiplicity, metadata| {
                writer.push(key, multiplicity)?;
                accumulator.record_prevalidated(multiplicity, metadata)?;
                Ok(())
            })?;
            Ok::<(), ChannelImageError>(())
        })?;
        (Some(accumulator.finish(components)), observed)
    } else {
        let observed =
            parse_endpoints(bytes, &mut cursor, |_key, _multiplicity, _metadata| Ok(()))?;
        (None, observed)
    };

    let declared =
        read_u64_be(bytes, &mut cursor).map_err(|_| ChannelImageError::TruncatedDistinctCount)?;
    if declared != observed {
        return Err(ChannelImageError::DistinctCountMismatch { declared, observed });
    }
    let flags = *bytes.get(cursor).ok_or(ChannelImageError::TruncatedFlags)?;
    cursor = cursor
        .checked_add(1)
        .ok_or(ChannelImageError::TruncatedFlags)?;
    if flags & !KNOWN_FLAGS != 0 {
        return Err(ChannelImageError::InvalidFlags(flags));
    }
    let connective_used = flags & CONNECTIVE_USED_FLAG != 0;
    let par = bag.map(|bag| Par::from_parts(bag, Vec::new(), connective_used));
    Ok((par, cursor))
}

fn parse_endpoints(
    bytes: &[u8],
    cursor: &mut usize,
    mut visitor: impl FnMut(&[u8], Multiplicity, &CanonicalKeyMetadata) -> Result<(), ChannelImageError>,
) -> Result<u64, ChannelImageError> {
    let mut observed = 0u64;
    let mut previous = Vec::<u8>::new();
    let mut key = Vec::<u8>::new();

    loop {
        let first = *bytes
            .get(*cursor)
            .ok_or(ChannelImageError::TruncatedFrame)?;
        if first == ESCAPE {
            let next = (*cursor)
                .checked_add(1)
                .ok_or(ChannelImageError::TruncatedFrame)?;
            let end = *bytes.get(next).ok_or(ChannelImageError::TruncatedFrame)?;
            if end != FRAME_END {
                return Err(ChannelImageError::InvalidEscape(end));
            }
            *cursor = next
                .checked_add(1)
                .ok_or(ChannelImageError::TruncatedFrame)?;
            break;
        }

        key.clear();
        key.extend_from_slice(&PAR_BAG_KEY_DOMAIN);
        take_escaped_frame(bytes, cursor, &mut key)?;
        // Every admitted endpoint started with a nonzero family tag. An empty
        // endpoint is therefore not a separate parse state: `00 00` above is
        // unambiguously the sequence terminator.
        if !previous.is_empty() && previous.as_slice() >= key.as_slice() {
            return Err(ChannelImageError::NonAscendingEndpoint);
        }
        let raw_multiplicity =
            read_u64_be(bytes, cursor).map_err(|_| ChannelImageError::TruncatedMultiplicity)?;
        let multiplicity =
            Multiplicity::new(raw_multiplicity).map_err(|_| ChannelImageError::ZeroMultiplicity)?;
        let metadata = validate_canonical_key(&key)?;
        visitor(&key, multiplicity, &metadata)?;
        previous.clear();
        previous.extend_from_slice(&key);
        observed = observed
            .checked_add(1)
            .ok_or(ChannelImageError::DistinctCountOverflow)?;
    }
    Ok(observed)
}

fn append_escaped_byte(output: &mut Vec<u8>, byte: u8) {
    output.push(byte);
    if byte == ESCAPE {
        output.push(ESCAPED_ZERO);
    }
}

fn append_escaped_slice(output: &mut Vec<u8>, bytes: &[u8]) {
    for &byte in bytes {
        append_escaped_byte(output, byte);
    }
}

fn take_escaped_frame(
    bytes: &[u8],
    cursor: &mut usize,
    output: &mut Vec<u8>,
) -> Result<(), ChannelImageError> {
    loop {
        let byte = *bytes
            .get(*cursor)
            .ok_or(ChannelImageError::TruncatedFrame)?;
        *cursor = (*cursor)
            .checked_add(1)
            .ok_or(ChannelImageError::TruncatedFrame)?;
        if byte != ESCAPE {
            output.push(byte);
            continue;
        }
        let escaped = *bytes
            .get(*cursor)
            .ok_or(ChannelImageError::TruncatedFrame)?;
        *cursor = (*cursor)
            .checked_add(1)
            .ok_or(ChannelImageError::TruncatedFrame)?;
        match escaped {
            ESCAPED_ZERO => output.push(ESCAPE),
            FRAME_END => return Ok(()),
            other => return Err(ChannelImageError::InvalidEscape(other)),
        }
    }
}

fn read_u64_be(bytes: &[u8], cursor: &mut usize) -> Result<u64, ()> {
    let end = (*cursor)
        .checked_add(std::mem::size_of::<u64>())
        .ok_or(())?;
    let word: [u8; 8] = bytes
        .get(*cursor..end)
        .ok_or(())?
        .try_into()
        .map_err(|_| ())?;
    *cursor = end;
    Ok(u64::from_be_bytes(word))
}

//! Immutable log-pack header codec and predecessor linkage.

use std::fmt::{Display, Formatter};

use super::PACK_HEADER_LEN_V1;
use crate::rspace::persistence::checksum::{
    CHECKSUM_FIELD_LEN, crc32c, decode_crc32c, encode_crc32c,
};
use crate::rspace::persistence::physical::{PHYSICAL_DIGEST_LEN, STORE_ID_LEN};
use crate::rspace::persistence::{
    LogPackFormatError, LogPackSequence, PackId, PhysicalDigest, StoreId,
};

const PACK_DOMAIN: &[u8; 16] = b"RSpacePack.V1\0\0\0";
const PACK_VERSION: u64 = 1;
const PACK_AUTH_LEN: usize = PACK_HEADER_LEN_V1 as usize - CHECKSUM_FIELD_LEN;

/// Terminal identity needed to open the next immutable pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackTerminalIdentityV1 {
    pack_id: PackId,
    last_sequence: LogPackSequence,
    commit_digest: PhysicalDigest,
}

impl PackTerminalIdentityV1 {
    pub const fn new(
        pack_id: PackId,
        last_sequence: LogPackSequence,
        commit_digest: PhysicalDigest,
    ) -> Self {
        Self {
            pack_id,
            last_sequence,
            commit_digest,
        }
    }

    pub const fn pack_id(self) -> PackId { self.pack_id }

    pub const fn last_sequence(self) -> LogPackSequence { self.last_sequence }

    pub const fn commit_digest(self) -> PhysicalDigest { self.commit_digest }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PackPredecessorV1 {
    pack_id: PackId,
    terminal_commit: PhysicalDigest,
}

/// Exact 128-byte identity header at offset zero of every immutable pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackHeaderV1 {
    store_id: StoreId,
    pack_id: PackId,
    first_sequence: LogPackSequence,
    predecessor: Option<PackPredecessorV1>,
}

impl PackHeaderV1 {
    /// The unique first pack of a durable store.
    pub const fn genesis(store_id: StoreId) -> Self {
        Self {
            store_id,
            pack_id: PackId::GENESIS,
            first_sequence: LogPackSequence::FIRST,
            predecessor: None,
        }
    }

    /// Construct the only valid successor of a verified terminal identity.
    pub fn successor(
        store_id: StoreId,
        previous: PackTerminalIdentityV1,
    ) -> Result<Self, PackFormatError> {
        Ok(Self {
            store_id,
            pack_id: previous.pack_id.successor()?,
            first_sequence: previous.last_sequence.successor()?,
            predecessor: Some(PackPredecessorV1 {
                pack_id: previous.pack_id,
                terminal_commit: previous.commit_digest,
            }),
        })
    }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this store id accessor yet"
    )]
    pub const fn store_id(self) -> StoreId { self.store_id }

    pub const fn pack_id(self) -> PackId { self.pack_id }

    pub const fn first_sequence(self) -> LogPackSequence { self.first_sequence }

    pub const fn previous_pack_id(self) -> Option<PackId> {
        match self.predecessor {
            Some(predecessor) => Some(predecessor.pack_id),
            None => None,
        }
    }

    pub const fn previous_terminal_commit(self) -> PhysicalDigest {
        match self.predecessor {
            Some(predecessor) => predecessor.terminal_commit,
            None => PhysicalDigest::ZERO,
        }
    }

    pub fn encode(self) -> [u8; PACK_HEADER_LEN_V1 as usize] {
        let mut bytes = [0u8; PACK_HEADER_LEN_V1 as usize];
        bytes[..16].copy_from_slice(PACK_DOMAIN);
        put_u64(&mut bytes, 16, PACK_VERSION);
        put_u64(&mut bytes, 24, PACK_HEADER_LEN_V1);
        bytes[32..48].copy_from_slice(self.store_id.as_bytes());
        put_u64(&mut bytes, 48, self.pack_id.get());
        put_u64(&mut bytes, 56, self.first_sequence.get());
        if let Some(predecessor) = self.predecessor {
            put_u64(&mut bytes, 64, predecessor.pack_id.get());
            bytes[72..104].copy_from_slice(predecessor.terminal_commit.as_bytes());
        }
        put_u64(&mut bytes, 104, 0);
        put_u64(&mut bytes, 112, 0);
        let checksum = crc32c(&bytes[..PACK_AUTH_LEN]);
        bytes[PACK_AUTH_LEN..].copy_from_slice(&encode_crc32c(checksum));
        bytes
    }

    pub fn parse_exact(bytes: &[u8]) -> Result<Self, PackFormatError> {
        let actual = u64::try_from(bytes.len()).map_err(|_| PackFormatError::LengthOutOfRange)?;
        if actual != PACK_HEADER_LEN_V1 {
            return Err(PackFormatError::Length {
                expected: PACK_HEADER_LEN_V1,
                actual,
            });
        }
        let declared_checksum = decode_crc32c(
            bytes[PACK_AUTH_LEN..]
                .try_into()
                .expect("fixed pack checksum extent"),
        )
        .map_err(PackFormatError::ChecksumOutOfRange)?;
        let actual_checksum = crc32c(&bytes[..PACK_AUTH_LEN]);
        if declared_checksum != actual_checksum {
            return Err(PackFormatError::Checksum {
                expected: declared_checksum,
                actual: actual_checksum,
            });
        }
        if &bytes[..16] != PACK_DOMAIN {
            return Err(PackFormatError::InvalidDomain);
        }
        let version = get_u64(bytes, 16);
        if version != PACK_VERSION {
            return Err(PackFormatError::UnsupportedVersion(version));
        }
        let header_len = get_u64(bytes, 24);
        if header_len != PACK_HEADER_LEN_V1 {
            return Err(PackFormatError::DeclaredLength(header_len));
        }
        let flags = get_u64(bytes, 104);
        if flags != 0 {
            return Err(PackFormatError::UnsupportedFlags(flags));
        }
        let reserved = get_u64(bytes, 112);
        if reserved != 0 {
            return Err(PackFormatError::NonzeroReserved(reserved));
        }
        let store_id = StoreId::from_array(
            bytes[32..48]
                .try_into()
                .expect("fixed pack store ID extent"),
        );
        let pack_id = PackId::new(get_u64(bytes, 48))?;
        let first_sequence = LogPackSequence::new(get_u64(bytes, 56))?;
        let previous_pack_raw = get_u64(bytes, 64);
        let previous_terminal_commit = PhysicalDigest::from_array(
            bytes[72..104]
                .try_into()
                .expect("fixed pack predecessor digest extent"),
        );
        let predecessor = if previous_pack_raw == 0 {
            if pack_id != PackId::GENESIS ||
                first_sequence != LogPackSequence::FIRST ||
                previous_terminal_commit != PhysicalDigest::ZERO
            {
                return Err(PackFormatError::InvalidGenesis {
                    pack_id: pack_id.get(),
                    first_sequence: first_sequence.get(),
                    previous_terminal_commit,
                });
            }
            None
        } else {
            let previous_pack_id = PackId::new(previous_pack_raw)?;
            let expected = previous_pack_id.successor()?;
            if pack_id != expected {
                return Err(PackFormatError::NonconsecutivePack {
                    previous: previous_pack_id.get(),
                    current: pack_id.get(),
                });
            }
            if first_sequence == LogPackSequence::FIRST {
                return Err(PackFormatError::SuccessorStartsAtGenesisSequence);
            }
            Some(PackPredecessorV1 {
                pack_id: previous_pack_id,
                terminal_commit: previous_terminal_commit,
            })
        };
        Ok(Self {
            store_id,
            pack_id,
            first_sequence,
            predecessor,
        })
    }

    pub fn verify_identity(
        self,
        expected_store: StoreId,
        expected_pack: PackId,
    ) -> Result<(), PackFormatError> {
        if self.store_id != expected_store {
            return Err(PackFormatError::Store {
                expected: expected_store,
                actual: self.store_id,
            });
        }
        if self.pack_id != expected_pack {
            return Err(PackFormatError::Pack {
                expected: expected_pack,
                actual: self.pack_id,
            });
        }
        Ok(())
    }

    pub fn verify_predecessor(
        self,
        previous: PackTerminalIdentityV1,
    ) -> Result<(), PackFormatError> {
        let predecessor = self
            .predecessor
            .ok_or(PackFormatError::GenesisHasNoPredecessor)?;
        if predecessor.pack_id != previous.pack_id {
            return Err(PackFormatError::PredecessorPack {
                expected: previous.pack_id,
                actual: predecessor.pack_id,
            });
        }
        if predecessor.terminal_commit != previous.commit_digest {
            return Err(PackFormatError::PredecessorDigest {
                expected: previous.commit_digest,
                actual: predecessor.terminal_commit,
            });
        }
        let expected_first = previous.last_sequence.successor()?;
        if self.first_sequence != expected_first {
            return Err(PackFormatError::FirstSequence {
                expected: expected_first,
                actual: self.first_sequence,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackFormatError {
    LogPack(LogPackFormatError),
    Length {
        expected: u64,
        actual: u64,
    },
    LengthOutOfRange,
    InvalidDomain,
    UnsupportedVersion(u64),
    DeclaredLength(u64),
    Checksum {
        expected: u32,
        actual: u32,
    },
    ChecksumOutOfRange(u64),
    UnsupportedFlags(u64),
    NonzeroReserved(u64),
    InvalidGenesis {
        pack_id: u64,
        first_sequence: u64,
        previous_terminal_commit: PhysicalDigest,
    },
    NonconsecutivePack {
        previous: u64,
        current: u64,
    },
    SuccessorStartsAtGenesisSequence,
    Store {
        expected: StoreId,
        actual: StoreId,
    },
    Pack {
        expected: PackId,
        actual: PackId,
    },
    GenesisHasNoPredecessor,
    PredecessorPack {
        expected: PackId,
        actual: PackId,
    },
    PredecessorDigest {
        expected: PhysicalDigest,
        actual: PhysicalDigest,
    },
    FirstSequence {
        expected: LogPackSequence,
        actual: LogPackSequence,
    },
}

impl From<LogPackFormatError> for PackFormatError {
    fn from(error: LogPackFormatError) -> Self { Self::LogPack(error) }
}

impl Display for PackFormatError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LogPack(error) => Display::fmt(error, formatter),
            Self::Length { expected, actual } => {
                write!(formatter, "pack header length {actual} does not equal {expected}")
            }
            Self::LengthOutOfRange => {
                formatter.write_str("pack-header byte length does not fit the portable u64 domain")
            }
            Self::InvalidDomain => formatter.write_str("invalid RSpace pack domain"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported RSpace pack version {version}")
            }
            Self::DeclaredLength(length) => {
                write!(formatter, "RSpace pack declares header length {length}")
            }
            Self::Checksum { expected, actual } => write!(
                formatter,
                "RSpace pack CRC32C mismatch: expected {expected:#010x}, got {actual:#010x}"
            ),
            Self::ChecksumOutOfRange(encoded) => write!(
                formatter,
                "RSpace pack checksum {encoded:#018x} is not a zero-extended CRC32C"
            ),
            Self::UnsupportedFlags(flags) => {
                write!(formatter, "unsupported RSpace pack flags {flags:#x}")
            }
            Self::NonzeroReserved(value) => {
                write!(formatter, "RSpace pack reserved field is nonzero: {value:#x}")
            }
            Self::InvalidGenesis {
                pack_id,
                first_sequence,
                previous_terminal_commit,
            } => write!(
                formatter,
                "invalid genesis pack fields: pack {pack_id}, first sequence {first_sequence}, \
                 predecessor digest {previous_terminal_commit}"
            ),
            Self::NonconsecutivePack { previous, current } => write!(
                formatter,
                "RSpace pack {current} is not the checked successor of pack {previous}"
            ),
            Self::SuccessorStartsAtGenesisSequence => {
                formatter.write_str("a successor pack cannot start at transaction sequence one")
            }
            Self::Store { expected, actual } => {
                write!(formatter, "expected RSpace store {expected}, got {actual}")
            }
            Self::Pack { expected, actual } => {
                write!(formatter, "expected RSpace pack {}, got {}", expected.get(), actual.get())
            }
            Self::GenesisHasNoPredecessor => {
                formatter.write_str("the genesis pack has no predecessor")
            }
            Self::PredecessorPack { expected, actual } => write!(
                formatter,
                "expected predecessor pack {}, got {}",
                expected.get(),
                actual.get()
            ),
            Self::PredecessorDigest { expected, actual } => {
                write!(formatter, "expected predecessor terminal commit {expected}, got {actual}")
            }
            Self::FirstSequence { expected, actual } => write!(
                formatter,
                "expected first transaction sequence {}, got {}",
                expected.get(),
                actual.get()
            ),
        }
    }
}

impl std::error::Error for PackFormatError {}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("fixed pack integer extent"),
    )
}

const _: () = {
    assert!(STORE_ID_LEN == 16);
    assert!(PHYSICAL_DIGEST_LEN == 32);
    assert!(PACK_AUTH_LEN == 120);
};

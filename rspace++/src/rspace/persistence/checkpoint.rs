//! Canonical authority for one independently recoverable pack generation.
//!
//! A generation checkpoint is a logical WAL payload, not a projected RSpace
//! collection and not a synthetic incremental mutation. Its ordered object
//! inventory is derived by walking the structurally admitted closure PathMap
//! in native zipper order and merging the retention-manifest singleton.

use std::cmp::Ordering;
use std::fmt::{Display, Formatter};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};

use super::canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, FORMAT_VERSION_V1, OBJECT_DOMAIN_LEN,
    OBJECT_ENVELOPE_HEADER_LEN, ObjectDigest, ObjectRefV1, ReadCursor, checked_sum, parse_envelope,
    write_envelope_header, write_u64,
};
use super::closure::{RetainedUnionClosureReportV1, StructuralClosureLedgerV1};
use super::manifest::{SEMANTIC_PROFILE_ID_LEN, SemanticProfileId};
use super::mutation::StateVersionV1;

pub(crate) const GENERATION_CHECKPOINT_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceDAG.Chkpt\0";
const ORDERED_INVENTORY_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceInv.RefV1\0";
const OBJECT_REF_LEN: u64 = 32 + 8;
const CHECKPOINT_BODY_LEN: u64 =
    SEMANTIC_PROFILE_ID_LEN as u64 + 8 + OBJECT_REF_LEN + OBJECT_REF_LEN + 5 * 8 + 32;
pub(crate) const GENERATION_CHECKPOINT_CANONICAL_LEN: u64 =
    OBJECT_ENVELOPE_HEADER_LEN + CHECKPOINT_BODY_LEN;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OrderedObjectInventoryDigestV1(ObjectDigest);

impl OrderedObjectInventoryDigestV1 {
    pub(crate) const fn as_bytes(&self) -> &[u8; 32] { self.0.as_array() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OrderedObjectInventoryV1 {
    digest: OrderedObjectInventoryDigestV1,
    objects: u64,
    canonical_bytes: u64,
}

impl OrderedObjectInventoryV1 {
    pub(crate) const fn digest(self) -> OrderedObjectInventoryDigestV1 { self.digest }

    pub(crate) const fn objects(self) -> u64 { self.objects }

    pub(crate) const fn canonical_bytes(self) -> u64 { self.canonical_bytes }
}

pub(crate) struct OrderedObjectInventoryBuilderV1 {
    hasher: Blake2b<U32>,
    previous: Option<ObjectRefV1>,
    objects: u64,
    canonical_bytes: u64,
}

impl OrderedObjectInventoryBuilderV1 {
    pub(crate) fn new() -> Self {
        let mut hasher = Blake2b::<U32>::new();
        hasher.update(ORDERED_INVENTORY_DOMAIN);
        hasher.update(FORMAT_VERSION_V1.to_le_bytes());
        Self {
            hasher,
            previous: None,
            objects: 0,
            canonical_bytes: 0,
        }
    }

    pub(crate) fn push(
        &mut self,
        reference: ObjectRefV1,
    ) -> Result<(), GenerationCheckpointErrorV1> {
        if let Some(previous) = self.previous {
            match previous.digest().cmp(&reference.digest()) {
                Ordering::Less => {}
                Ordering::Equal => {
                    return Err(GenerationCheckpointErrorV1::DuplicateInventoryDigest {
                        digest: reference.digest(),
                        first_len: previous.encoded_len(),
                        second_len: reference.encoded_len(),
                    });
                }
                Ordering::Greater => {
                    return Err(GenerationCheckpointErrorV1::InventoryNotStrict {
                        previous: previous.digest(),
                        current: reference.digest(),
                    });
                }
            }
        }
        let objects = self
            .objects
            .checked_add(1)
            .ok_or(GenerationCheckpointErrorV1::CounterOverflow("checkpoint inventory objects"))?;
        let canonical_bytes = self
            .canonical_bytes
            .checked_add(reference.encoded_len())
            .ok_or(GenerationCheckpointErrorV1::CounterOverflow(
                "checkpoint inventory canonical bytes",
            ))?;
        self.hasher.update(reference.digest().as_bytes());
        self.hasher.update(reference.encoded_len().to_le_bytes());
        self.previous = Some(reference);
        self.objects = objects;
        self.canonical_bytes = canonical_bytes;
        Ok(())
    }

    pub(crate) fn finish(self) -> OrderedObjectInventoryV1 {
        let hash = self.hasher.finalize();
        let mut digest = [0_u8; 32];
        digest.copy_from_slice(&hash);
        OrderedObjectInventoryV1 {
            digest: OrderedObjectInventoryDigestV1(ObjectDigest::from_array(digest)),
            objects: self.objects,
            canonical_bytes: self.canonical_bytes,
        }
    }
}

impl Default for OrderedObjectInventoryBuilderV1 {
    fn default() -> Self { Self::new() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CheckpointObjectInventoryV1 {
    retained_union_objects: u64,
    retained_union_canonical_bytes: u64,
    data_objects: u64,
    data_canonical_bytes: u64,
    ordered_digest: OrderedObjectInventoryDigestV1,
}

impl CheckpointObjectInventoryV1 {
    pub(crate) fn from_retained_union(
        ledger: &StructuralClosureLedgerV1,
        retention_manifest: ObjectRefV1,
    ) -> Result<Self, GenerationCheckpointErrorV1> {
        let report = ledger.retained_union_report();
        let mut builder = OrderedObjectInventoryBuilderV1::new();
        let mut retention_pending = true;
        ledger.try_visit_retained_union(|reference| {
            if retention_pending {
                match retention_manifest.digest().cmp(&reference.digest()) {
                    Ordering::Less => {
                        builder.push(retention_manifest)?;
                        retention_pending = false;
                    }
                    Ordering::Equal => {
                        return Err(GenerationCheckpointErrorV1::DuplicateInventoryDigest {
                            digest: reference.digest(),
                            first_len: retention_manifest.encoded_len(),
                            second_len: reference.encoded_len(),
                        });
                    }
                    Ordering::Greater => {}
                }
            }
            builder.push(reference)
        })?;
        if retention_pending {
            builder.push(retention_manifest)?;
        }
        Self::from_ordered(report, retention_manifest, builder.finish())
    }

    pub(crate) fn from_ordered(
        report: RetainedUnionClosureReportV1,
        retention_manifest: ObjectRefV1,
        ordered: OrderedObjectInventoryV1,
    ) -> Result<Self, GenerationCheckpointErrorV1> {
        let data_objects = report
            .distinct_objects
            .checked_add(1)
            .ok_or(GenerationCheckpointErrorV1::CounterOverflow("checkpoint DATA objects"))?;
        let data_canonical_bytes = report
            .canonical_bytes
            .checked_add(retention_manifest.encoded_len())
            .ok_or(GenerationCheckpointErrorV1::CounterOverflow(
                "checkpoint DATA canonical bytes",
            ))?;
        if ordered.objects() != data_objects || ordered.canonical_bytes() != data_canonical_bytes {
            return Err(GenerationCheckpointErrorV1::InventoryIncoherent {
                expected_objects: data_objects,
                actual_objects: ordered.objects(),
                expected_bytes: data_canonical_bytes,
                actual_bytes: ordered.canonical_bytes(),
            });
        }
        Ok(Self {
            retained_union_objects: report.distinct_objects,
            retained_union_canonical_bytes: report.canonical_bytes,
            data_objects,
            data_canonical_bytes,
            ordered_digest: ordered.digest(),
        })
    }

    pub(crate) const fn retained_union_objects(self) -> u64 { self.retained_union_objects }

    pub(crate) const fn retained_union_canonical_bytes(self) -> u64 {
        self.retained_union_canonical_bytes
    }

    pub(crate) const fn data_objects(self) -> u64 { self.data_objects }

    pub(crate) const fn data_canonical_bytes(self) -> u64 { self.data_canonical_bytes }

    pub(crate) const fn ordered_digest(self) -> OrderedObjectInventoryDigestV1 {
        self.ordered_digest
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GenerationCheckpointDescriptorV1 {
    profile_id: SemanticProfileId,
    state: StateVersionV1,
    retention_manifest: ObjectRefV1,
    current_state_canonical_bytes: u64,
    inventory: CheckpointObjectInventoryV1,
}

impl GenerationCheckpointDescriptorV1 {
    pub(crate) fn new(
        profile_id: SemanticProfileId,
        state: StateVersionV1,
        retention_manifest: ObjectRefV1,
        current_state_canonical_bytes: u64,
        inventory: CheckpointObjectInventoryV1,
    ) -> Result<Self, GenerationCheckpointErrorV1> {
        if current_state_canonical_bytes > inventory.retained_union_canonical_bytes() {
            return Err(GenerationCheckpointErrorV1::CurrentExceedsRetained {
                current: current_state_canonical_bytes,
                retained: inventory.retained_union_canonical_bytes(),
            });
        }
        Ok(Self {
            profile_id,
            state,
            retention_manifest,
            current_state_canonical_bytes,
            inventory,
        })
    }

    pub(crate) const fn profile_id(self) -> SemanticProfileId { self.profile_id }

    pub(crate) const fn state(self) -> StateVersionV1 { self.state }

    pub(crate) const fn retention_manifest(self) -> ObjectRefV1 { self.retention_manifest }

    pub(crate) const fn current_state_canonical_bytes(self) -> u64 {
        self.current_state_canonical_bytes
    }

    pub(crate) const fn inventory(self) -> CheckpointObjectInventoryV1 { self.inventory }
}

impl CanonicalEncode for GenerationCheckpointDescriptorV1 {
    fn canonical_len(&self) -> u64 { GENERATION_CHECKPOINT_CANONICAL_LEN }

    fn write_canonical<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        write_envelope_header(sink, GENERATION_CHECKPOINT_DOMAIN, CHECKPOINT_BODY_LEN)?;
        sink.write_all(self.profile_id.as_bytes())?;
        write_u64(sink, self.state.generation())?;
        self.state.manifest().write(sink)?;
        self.retention_manifest.write(sink)?;
        write_u64(sink, self.current_state_canonical_bytes)?;
        write_u64(sink, self.inventory.retained_union_canonical_bytes())?;
        write_u64(sink, self.inventory.retained_union_objects())?;
        write_u64(sink, self.inventory.data_objects())?;
        write_u64(sink, self.inventory.data_canonical_bytes())?;
        sink.write_all(self.inventory.ordered_digest().as_bytes())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GenerationCheckpointDescriptorViewV1 {
    descriptor: GenerationCheckpointDescriptorV1,
}

impl GenerationCheckpointDescriptorViewV1 {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, GenerationCheckpointErrorV1> {
        let body =
            parse_envelope(bytes, GENERATION_CHECKPOINT_DOMAIN, "RSpace generation checkpoint")?;
        let mut cursor = ReadCursor::new(body);
        let profile_id = SemanticProfileId::from_array(
            cursor
                .read_exact(SEMANTIC_PROFILE_ID_LEN)?
                .try_into()
                .expect("semantic profile width was preflighted"),
        );
        let state = StateVersionV1::new(cursor.read_u64()?, ObjectRefV1::parse(&mut cursor)?);
        let retention_manifest = ObjectRefV1::parse(&mut cursor)?;
        let current_state_canonical_bytes = cursor.read_u64()?;
        let retained_union_canonical_bytes = cursor.read_u64()?;
        let retained_union_objects = cursor.read_u64()?;
        let data_objects = cursor.read_u64()?;
        let data_canonical_bytes = cursor.read_u64()?;
        let ordered_digest = OrderedObjectInventoryDigestV1(cursor.read_digest()?);
        cursor.finish()?;

        let expected_objects = retained_union_objects
            .checked_add(1)
            .ok_or(GenerationCheckpointErrorV1::CounterOverflow("checkpoint DATA objects"))?;
        let expected_bytes = checked_sum(
            retained_union_canonical_bytes,
            retention_manifest.encoded_len(),
            "checkpoint DATA canonical bytes",
        )?;
        if data_objects != expected_objects || data_canonical_bytes != expected_bytes {
            return Err(GenerationCheckpointErrorV1::InventoryIncoherent {
                expected_objects,
                actual_objects: data_objects,
                expected_bytes,
                actual_bytes: data_canonical_bytes,
            });
        }
        let inventory = CheckpointObjectInventoryV1 {
            retained_union_objects,
            retained_union_canonical_bytes,
            data_objects,
            data_canonical_bytes,
            ordered_digest,
        };
        let descriptor = GenerationCheckpointDescriptorV1::new(
            profile_id,
            state,
            retention_manifest,
            current_state_canonical_bytes,
            inventory,
        )?;
        Ok(Self { descriptor })
    }

    pub(crate) const fn descriptor(self) -> GenerationCheckpointDescriptorV1 { self.descriptor }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GenerationCheckpointErrorV1 {
    Canonical(CanonicalFormatError),
    CounterOverflow(&'static str),
    DuplicateInventoryDigest {
        digest: ObjectDigest,
        first_len: u64,
        second_len: u64,
    },
    InventoryNotStrict {
        previous: ObjectDigest,
        current: ObjectDigest,
    },
    InventoryIncoherent {
        expected_objects: u64,
        actual_objects: u64,
        expected_bytes: u64,
        actual_bytes: u64,
    },
    CurrentExceedsRetained {
        current: u64,
        retained: u64,
    },
}

impl Display for GenerationCheckpointErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Canonical(error) => Display::fmt(error, formatter),
            Self::CounterOverflow(field) => write!(formatter, "{field} overflows u64"),
            Self::DuplicateInventoryDigest {
                digest,
                first_len,
                second_len,
            } => write!(
                formatter,
                "checkpoint inventory repeats digest {digest} with lengths {first_len} and \
                 {second_len}"
            ),
            Self::InventoryNotStrict { previous, current } => write!(
                formatter,
                "checkpoint inventory digest {current} does not strictly follow {previous}"
            ),
            Self::InventoryIncoherent {
                expected_objects,
                actual_objects,
                expected_bytes,
                actual_bytes,
            } => write!(
                formatter,
                "checkpoint inventory expected {expected_objects} objects/{expected_bytes} bytes, \
                 got {actual_objects} objects/{actual_bytes} bytes"
            ),
            Self::CurrentExceedsRetained { current, retained } => write!(
                formatter,
                "checkpoint current-state inventory {current} exceeds retained inventory \
                 {retained}"
            ),
        }
    }
}

impl std::error::Error for GenerationCheckpointErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Canonical(error) => Some(error),
            _ => None,
        }
    }
}

impl From<CanonicalFormatError> for GenerationCheckpointErrorV1 {
    fn from(error: CanonicalFormatError) -> Self { Self::Canonical(error) }
}

const _: () = assert!(CHECKPOINT_BODY_LEN == 176);
const _: () = assert!(GENERATION_CHECKPOINT_CANONICAL_LEN == 208);

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
use super::retention::v2_format::{
    GENERATION_CHECKPOINT_V2_DOMAIN, RETENTION_ANCHOR_CANONICAL_LEN,
    RETENTION_METADATA_MIN_CANONICAL_BYTES, RETENTION_METADATA_MIN_OBJECTS, RetentionFormat,
    RetentionReader,
};

pub(crate) const GENERATION_CHECKPOINT_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceDAG.Chkpt\0";
const ORDERED_INVENTORY_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceInv.RefV1\0";
const OBJECT_REF_LEN: u64 = 32 + 8;
const CHECKPOINT_BODY_LEN: u64 =
    SEMANTIC_PROFILE_ID_LEN as u64 + 8 + OBJECT_REF_LEN + OBJECT_REF_LEN + 6 * 8 + 32;
pub(crate) const GENERATION_CHECKPOINT_CANONICAL_LEN: u64 =
    OBJECT_ENVELOPE_HEADER_LEN + CHECKPOINT_BODY_LEN;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OrderedObjectInventoryDigestV1([u8; 32]);

impl OrderedObjectInventoryDigestV1 {
    pub(crate) const fn from_array(bytes: [u8; 32]) -> Self { Self(bytes) }

    pub(crate) const fn as_bytes(&self) -> &[u8; 32] { &self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CurrentClosureCanonicalBytesV1(u64);

impl CurrentClosureCanonicalBytesV1 {
    pub(crate) const fn new(bytes: u64) -> Self { Self(bytes) }

    pub(crate) const fn get(self) -> u64 { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RetainedUnionCanonicalBytesV1(u64);

impl RetainedUnionCanonicalBytesV1 {
    pub(crate) const fn new(bytes: u64) -> Self { Self(bytes) }

    pub(crate) const fn get(self) -> u64 { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CheckpointInlineCanonicalBytesV1(u64);

impl CheckpointInlineCanonicalBytesV1 {
    pub(crate) const fn new(bytes: u64) -> Self { Self(bytes) }

    pub(crate) const fn get(self) -> u64 { self.0 }
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
    expected_objects: u64,
    expected_canonical_bytes: u64,
    previous: Option<ObjectRefV1>,
    objects: u64,
    canonical_bytes: u64,
}

impl OrderedObjectInventoryBuilderV1 {
    pub(crate) fn new(expected_objects: u64, expected_canonical_bytes: u64) -> Self {
        let mut hasher = Blake2b::<U32>::new();
        hasher.update(ORDERED_INVENTORY_DOMAIN);
        hasher.update(FORMAT_VERSION_V1.to_le_bytes());
        hasher.update(expected_objects.to_le_bytes());
        hasher.update(expected_canonical_bytes.to_le_bytes());
        Self {
            hasher,
            expected_objects,
            expected_canonical_bytes,
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
        if objects > self.expected_objects || canonical_bytes > self.expected_canonical_bytes {
            return Err(GenerationCheckpointErrorV1::InventoryIncoherent {
                expected_objects: self.expected_objects,
                actual_objects: objects,
                expected_bytes: self.expected_canonical_bytes,
                actual_bytes: canonical_bytes,
            });
        }
        self.hasher.update(reference.digest().as_bytes());
        self.hasher.update(reference.encoded_len().to_le_bytes());
        self.previous = Some(reference);
        self.objects = objects;
        self.canonical_bytes = canonical_bytes;
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<OrderedObjectInventoryV1, GenerationCheckpointErrorV1> {
        if self.objects != self.expected_objects ||
            self.canonical_bytes != self.expected_canonical_bytes
        {
            return Err(GenerationCheckpointErrorV1::InventoryIncoherent {
                expected_objects: self.expected_objects,
                actual_objects: self.objects,
                expected_bytes: self.expected_canonical_bytes,
                actual_bytes: self.canonical_bytes,
            });
        }
        let hash = self.hasher.finalize();
        let mut digest = [0_u8; 32];
        digest.copy_from_slice(&hash);
        Ok(OrderedObjectInventoryV1 {
            digest: OrderedObjectInventoryDigestV1(digest),
            objects: self.objects,
            canonical_bytes: self.canonical_bytes,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CheckpointObjectInventoryV1 {
    retained_union_objects: u64,
    retained_union_canonical_bytes: RetainedUnionCanonicalBytesV1,
    data_objects: u64,
    data_canonical_bytes: CheckpointInlineCanonicalBytesV1,
    ordered_digest: OrderedObjectInventoryDigestV1,
    format: RetentionFormat,
}

impl CheckpointObjectInventoryV1 {
    pub(crate) fn from_retained_union(
        ledger: &StructuralClosureLedgerV1,
        retention_manifest: ObjectRefV1,
    ) -> Result<Self, GenerationCheckpointErrorV1> {
        let report = ledger.retained_union_report();
        if report.distinct_objects == 0 {
            return Err(GenerationCheckpointErrorV1::EmptyRetainedUnion);
        }
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
        let mut builder = OrderedObjectInventoryBuilderV1::new(data_objects, data_canonical_bytes);
        try_visit_checkpoint_references(ledger, retention_manifest, |reference| {
            builder.push(reference)
        })
        .map_err(|error| match error {
            CheckpointReferenceVisitErrorV1::Checkpoint(error) |
            CheckpointReferenceVisitErrorV1::Visitor(error) => error,
        })?;
        Self::from_ordered(report, retention_manifest, builder.finish()?)
    }

    fn from_ordered(
        report: RetainedUnionClosureReportV1,
        retention_manifest: ObjectRefV1,
        ordered: OrderedObjectInventoryV1,
    ) -> Result<Self, GenerationCheckpointErrorV1> {
        if report.distinct_objects == 0 {
            return Err(GenerationCheckpointErrorV1::EmptyRetainedUnion);
        }
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
            retained_union_canonical_bytes: RetainedUnionCanonicalBytesV1::new(
                report.canonical_bytes,
            ),
            data_objects,
            data_canonical_bytes: CheckpointInlineCanonicalBytesV1::new(data_canonical_bytes),
            ordered_digest: ordered.digest(),
            format: RetentionFormat::V1,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_test_ordered(
        report: RetainedUnionClosureReportV1,
        retention_manifest: ObjectRefV1,
        ordered: OrderedObjectInventoryV1,
    ) -> Result<Self, GenerationCheckpointErrorV1> {
        Self::from_ordered(report, retention_manifest, ordered)
    }

    /// Test-only V2 inventory: DATA is the retained union plus the metadata
    /// closure. Production V2 emission belongs to task 7926.
    #[cfg(test)]
    pub(crate) fn from_test_ordered_v2(
        report: RetainedUnionClosureReportV1,
        metadata_objects: u64,
        metadata_canonical_bytes: u64,
        ordered: OrderedObjectInventoryV1,
    ) -> Result<Self, GenerationCheckpointErrorV1> {
        if report.distinct_objects == 0 {
            return Err(GenerationCheckpointErrorV1::EmptyRetainedUnion);
        }
        let data_objects = report
            .distinct_objects
            .checked_add(metadata_objects)
            .ok_or(GenerationCheckpointErrorV1::CounterOverflow("checkpoint DATA objects"))?;
        let data_canonical_bytes = report
            .canonical_bytes
            .checked_add(metadata_canonical_bytes)
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
            retained_union_canonical_bytes: RetainedUnionCanonicalBytesV1::new(
                report.canonical_bytes,
            ),
            data_objects,
            data_canonical_bytes: CheckpointInlineCanonicalBytesV1::new(data_canonical_bytes),
            ordered_digest: ordered.digest(),
            format: RetentionFormat::V2,
        })
    }

    pub(crate) const fn retained_union_objects(self) -> u64 { self.retained_union_objects }

    pub(crate) const fn retained_union_canonical_bytes(self) -> RetainedUnionCanonicalBytesV1 {
        self.retained_union_canonical_bytes
    }

    pub(crate) const fn data_objects(self) -> u64 { self.data_objects }

    pub(crate) const fn data_canonical_bytes(self) -> CheckpointInlineCanonicalBytesV1 {
        self.data_canonical_bytes
    }

    pub(crate) const fn ordered_digest(self) -> OrderedObjectInventoryDigestV1 {
        self.ordered_digest
    }

    pub(crate) const fn format(self) -> RetentionFormat { self.format }
}

/// The V2 inventory rule: the appointed retention object is a 136-byte anchor
/// and DATA minus the retained union holds at least the anchor and the radix
/// root. The exact metadata closure is authenticated by cold admission, the
/// only place it is computed.
fn require_v2_metadata(
    retention_anchor: ObjectRefV1,
    retained_objects: u64,
    retained_bytes: u64,
    data_objects: u64,
    data_bytes: u64,
) -> Result<(), GenerationCheckpointErrorV1> {
    if retention_anchor.encoded_len() != RETENTION_ANCHOR_CANONICAL_LEN {
        return Err(GenerationCheckpointErrorV1::RetentionAnchorLength {
            actual: retention_anchor.encoded_len(),
        });
    }
    match (data_objects.checked_sub(retained_objects), data_bytes.checked_sub(retained_bytes)) {
        (Some(objects), Some(bytes))
            if objects >= RETENTION_METADATA_MIN_OBJECTS &&
                bytes >= RETENTION_METADATA_MIN_CANONICAL_BYTES =>
        {
            Ok(())
        }
        _ => Err(GenerationCheckpointErrorV1::RetentionMetadataBelowMinimum {
            data_objects,
            retained_objects,
            data_bytes,
            retained_bytes,
        }),
    }
}

#[derive(Debug)]
pub(crate) enum CheckpointReferenceVisitErrorV1<E> {
    Checkpoint(GenerationCheckpointErrorV1),
    Visitor(E),
}

impl<E: Display> Display for CheckpointReferenceVisitErrorV1<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Checkpoint(error) => Display::fmt(error, formatter),
            Self::Visitor(error) => {
                write!(formatter, "checkpoint reference visitor failed: {error}")
            }
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for CheckpointReferenceVisitErrorV1<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Checkpoint(error) => Some(error),
            Self::Visitor(error) => Some(error),
        }
    }
}

/// Merge the digest-ordered retained closure with the retention-manifest
/// singleton without materializing or sorting either input.
pub(crate) fn try_visit_checkpoint_references<E>(
    ledger: &StructuralClosureLedgerV1,
    retention_manifest: ObjectRefV1,
    mut visitor: impl FnMut(ObjectRefV1) -> Result<(), E>,
) -> Result<(), CheckpointReferenceVisitErrorV1<E>> {
    let mut retention_pending = true;
    ledger.try_visit_retained_union(|reference| {
        if retention_pending {
            match retention_manifest.digest().cmp(&reference.digest()) {
                Ordering::Less => {
                    visitor(retention_manifest)
                        .map_err(CheckpointReferenceVisitErrorV1::Visitor)?;
                    retention_pending = false;
                }
                Ordering::Equal => {
                    return Err(CheckpointReferenceVisitErrorV1::Checkpoint(
                        GenerationCheckpointErrorV1::DuplicateInventoryDigest {
                            digest: reference.digest(),
                            first_len: retention_manifest.encoded_len(),
                            second_len: reference.encoded_len(),
                        },
                    ));
                }
                Ordering::Greater => {}
            }
        }
        visitor(reference).map_err(CheckpointReferenceVisitErrorV1::Visitor)
    })?;
    if retention_pending {
        visitor(retention_manifest).map_err(CheckpointReferenceVisitErrorV1::Visitor)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GenerationCheckpointDescriptorV1 {
    profile_id: SemanticProfileId,
    state: StateVersionV1,
    retention_manifest: ObjectRefV1,
    current_state_objects: u64,
    current_state_canonical_bytes: CurrentClosureCanonicalBytesV1,
    inventory: CheckpointObjectInventoryV1,
}

impl GenerationCheckpointDescriptorV1 {
    pub(crate) fn from_retained_union(
        profile_id: SemanticProfileId,
        state: StateVersionV1,
        retention_manifest: ObjectRefV1,
        current_state_objects: u64,
        current_state_canonical_bytes: u64,
        ledger: &StructuralClosureLedgerV1,
    ) -> Result<Self, GenerationCheckpointErrorV1> {
        let inventory =
            CheckpointObjectInventoryV1::from_retained_union(ledger, retention_manifest)?;
        Self::from_admitted_inventory(
            profile_id,
            state,
            retention_manifest,
            current_state_objects,
            current_state_canonical_bytes,
            inventory,
        )
    }

    fn from_admitted_inventory(
        profile_id: SemanticProfileId,
        state: StateVersionV1,
        retention_manifest: ObjectRefV1,
        current_state_objects: u64,
        current_state_canonical_bytes: u64,
        inventory: CheckpointObjectInventoryV1,
    ) -> Result<Self, GenerationCheckpointErrorV1> {
        if let RetentionFormat::V2 = inventory.format() {
            require_v2_metadata(
                retention_manifest,
                inventory.retained_union_objects(),
                inventory.retained_union_canonical_bytes().get(),
                inventory.data_objects(),
                inventory.data_canonical_bytes().get(),
            )?;
        }
        if current_state_objects == 0 {
            return Err(GenerationCheckpointErrorV1::CurrentObjectsBelowManifest {
                actual: current_state_objects,
            });
        }
        if current_state_canonical_bytes < state.manifest().encoded_len() {
            return Err(GenerationCheckpointErrorV1::CurrentBelowManifest {
                minimum: state.manifest().encoded_len(),
                actual: current_state_canonical_bytes,
            });
        }
        if inventory.retained_union_objects() == 0 {
            return Err(GenerationCheckpointErrorV1::EmptyRetainedUnion);
        }
        if current_state_objects > inventory.retained_union_objects() {
            return Err(GenerationCheckpointErrorV1::CurrentObjectsExceedRetained {
                current: current_state_objects,
                retained: inventory.retained_union_objects(),
            });
        }
        if current_state_canonical_bytes > inventory.retained_union_canonical_bytes().get() {
            return Err(GenerationCheckpointErrorV1::CurrentExceedsRetained {
                current: current_state_canonical_bytes,
                retained: inventory.retained_union_canonical_bytes().get(),
            });
        }
        Ok(Self {
            profile_id,
            state,
            retention_manifest,
            current_state_objects,
            current_state_canonical_bytes: CurrentClosureCanonicalBytesV1::new(
                current_state_canonical_bytes,
            ),
            inventory,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_test_inventory(
        profile_id: SemanticProfileId,
        state: StateVersionV1,
        retention_manifest: ObjectRefV1,
        current_state_objects: u64,
        current_state_canonical_bytes: u64,
        inventory: CheckpointObjectInventoryV1,
    ) -> Result<Self, GenerationCheckpointErrorV1> {
        Self::from_admitted_inventory(
            profile_id,
            state,
            retention_manifest,
            current_state_objects,
            current_state_canonical_bytes,
            inventory,
        )
    }

    pub(crate) const fn profile_id(self) -> SemanticProfileId { self.profile_id }

    pub(crate) const fn state(self) -> StateVersionV1 { self.state }

    pub(crate) const fn retention_manifest(self) -> ObjectRefV1 { self.retention_manifest }

    pub(crate) const fn current_state_objects(self) -> u64 { self.current_state_objects }

    pub(crate) const fn current_state_canonical_bytes(self) -> CurrentClosureCanonicalBytesV1 {
        self.current_state_canonical_bytes
    }

    pub(crate) const fn inventory(self) -> CheckpointObjectInventoryV1 { self.inventory }

    pub(crate) const fn as_view(self) -> GenerationCheckpointDescriptorViewV1 {
        GenerationCheckpointDescriptorViewV1 {
            profile_id: self.profile_id,
            state: self.state,
            retention_manifest: self.retention_manifest,
            current_state_objects: self.current_state_objects,
            current_state_canonical_bytes: self.current_state_canonical_bytes,
            retained_union_canonical_bytes: self.inventory.retained_union_canonical_bytes,
            retained_union_objects: self.inventory.retained_union_objects,
            data_objects: self.inventory.data_objects,
            data_canonical_bytes: self.inventory.data_canonical_bytes,
            ordered_digest: self.inventory.ordered_digest,
            format: self.inventory.format,
        }
    }

    pub(crate) const fn from_view(view: GenerationCheckpointDescriptorViewV1) -> Self {
        Self {
            profile_id: view.profile_id,
            state: view.state,
            retention_manifest: view.retention_manifest,
            current_state_objects: view.current_state_objects,
            current_state_canonical_bytes: view.current_state_canonical_bytes,
            inventory: CheckpointObjectInventoryV1 {
                retained_union_objects: view.retained_union_objects,
                retained_union_canonical_bytes: view.retained_union_canonical_bytes,
                data_objects: view.data_objects,
                data_canonical_bytes: view.data_canonical_bytes,
                ordered_digest: view.ordered_digest,
                format: view.format,
            },
        }
    }
}

impl CanonicalEncode for GenerationCheckpointDescriptorV1 {
    fn canonical_len(&self) -> u64 { GENERATION_CHECKPOINT_CANONICAL_LEN }

    fn write_canonical<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        let domain = match self.inventory.format {
            RetentionFormat::V1 => GENERATION_CHECKPOINT_DOMAIN,
            RetentionFormat::V2 => GENERATION_CHECKPOINT_V2_DOMAIN,
        };
        write_envelope_header(sink, domain, CHECKPOINT_BODY_LEN)?;
        sink.write_all(self.profile_id.as_bytes())?;
        write_u64(sink, self.state.generation())?;
        self.state.manifest().write(sink)?;
        self.retention_manifest.write(sink)?;
        write_u64(sink, self.current_state_objects)?;
        write_u64(sink, self.current_state_canonical_bytes.get())?;
        write_u64(sink, self.inventory.retained_union_canonical_bytes().get())?;
        write_u64(sink, self.inventory.retained_union_objects())?;
        write_u64(sink, self.inventory.data_objects())?;
        write_u64(sink, self.inventory.data_canonical_bytes().get())?;
        sink.write_all(self.inventory.ordered_digest().as_bytes())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GenerationCheckpointDescriptorViewV1 {
    profile_id: SemanticProfileId,
    state: StateVersionV1,
    retention_manifest: ObjectRefV1,
    current_state_objects: u64,
    current_state_canonical_bytes: CurrentClosureCanonicalBytesV1,
    retained_union_canonical_bytes: RetainedUnionCanonicalBytesV1,
    retained_union_objects: u64,
    data_objects: u64,
    data_canonical_bytes: CheckpointInlineCanonicalBytesV1,
    ordered_digest: OrderedObjectInventoryDigestV1,
    format: RetentionFormat,
}

impl GenerationCheckpointDescriptorViewV1 {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, GenerationCheckpointErrorV1> {
        let body =
            parse_envelope(bytes, GENERATION_CHECKPOINT_DOMAIN, "RSpace generation checkpoint")?;
        Self::parse_body(body, RetentionFormat::V1)
    }

    /// Parse a descriptor for a reader with the given retention capability.
    /// A V1-only reader is exactly [`Self::parse`]; a dual reader parses the
    /// V2 domain under the V2 inventory rule and every other domain strictly.
    pub(crate) fn parse_for(
        bytes: &[u8],
        reader: RetentionReader,
    ) -> Result<Self, GenerationCheckpointErrorV1> {
        match bytes.get(..OBJECT_DOMAIN_LEN) {
            Some(domain)
                if domain == GENERATION_CHECKPOINT_V2_DOMAIN &&
                    reader.admits(RetentionFormat::V2) =>
            {
                let body = parse_envelope(
                    bytes,
                    GENERATION_CHECKPOINT_V2_DOMAIN,
                    "RSpace generation checkpoint V2",
                )?;
                Self::parse_body(body, RetentionFormat::V2)
            }
            _ => Self::parse(bytes),
        }
    }

    fn parse_body(
        body: &[u8],
        format: RetentionFormat,
    ) -> Result<Self, GenerationCheckpointErrorV1> {
        let mut cursor = ReadCursor::new(body);
        let profile_id = SemanticProfileId::from_array(
            cursor
                .read_exact(SEMANTIC_PROFILE_ID_LEN)?
                .try_into()
                .expect("semantic profile width was preflighted"),
        );
        let state = StateVersionV1::new(cursor.read_u64()?, ObjectRefV1::parse(&mut cursor)?);
        let retention_manifest = ObjectRefV1::parse(&mut cursor)?;
        let current_state_objects = cursor.read_u64()?;
        let current_state_canonical_bytes = cursor.read_u64()?;
        let retained_union_canonical_bytes = cursor.read_u64()?;
        let retained_union_objects = cursor.read_u64()?;
        let data_objects = cursor.read_u64()?;
        let data_canonical_bytes = cursor.read_u64()?;
        let ordered_digest =
            OrderedObjectInventoryDigestV1::from_array(*cursor.read_digest()?.as_array());
        cursor.finish()?;

        match format {
            RetentionFormat::V1 => {
                let expected_objects = retained_union_objects.checked_add(1).ok_or(
                    GenerationCheckpointErrorV1::CounterOverflow("checkpoint DATA objects"),
                )?;
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
            }
            RetentionFormat::V2 => require_v2_metadata(
                retention_manifest,
                retained_union_objects,
                retained_union_canonical_bytes,
                data_objects,
                data_canonical_bytes,
            )?,
        }
        if current_state_objects == 0 {
            return Err(GenerationCheckpointErrorV1::CurrentObjectsBelowManifest {
                actual: current_state_objects,
            });
        }
        if current_state_canonical_bytes < state.manifest().encoded_len() {
            return Err(GenerationCheckpointErrorV1::CurrentBelowManifest {
                minimum: state.manifest().encoded_len(),
                actual: current_state_canonical_bytes,
            });
        }
        if retained_union_objects == 0 {
            return Err(GenerationCheckpointErrorV1::EmptyRetainedUnion);
        }
        if current_state_objects > retained_union_objects {
            return Err(GenerationCheckpointErrorV1::CurrentObjectsExceedRetained {
                current: current_state_objects,
                retained: retained_union_objects,
            });
        }
        if current_state_canonical_bytes > retained_union_canonical_bytes {
            return Err(GenerationCheckpointErrorV1::CurrentExceedsRetained {
                current: current_state_canonical_bytes,
                retained: retained_union_canonical_bytes,
            });
        }
        Ok(Self {
            profile_id,
            state,
            retention_manifest,
            current_state_objects,
            current_state_canonical_bytes: CurrentClosureCanonicalBytesV1::new(
                current_state_canonical_bytes,
            ),
            retained_union_canonical_bytes: RetainedUnionCanonicalBytesV1::new(
                retained_union_canonical_bytes,
            ),
            retained_union_objects,
            data_objects,
            data_canonical_bytes: CheckpointInlineCanonicalBytesV1::new(data_canonical_bytes),
            ordered_digest,
            format,
        })
    }

    pub(crate) const fn profile_id(self) -> SemanticProfileId { self.profile_id }

    pub(crate) const fn state(self) -> StateVersionV1 { self.state }

    pub(crate) const fn retention_manifest(self) -> ObjectRefV1 { self.retention_manifest }

    pub(crate) const fn current_state_objects(self) -> u64 { self.current_state_objects }

    pub(crate) const fn current_state_canonical_bytes(self) -> CurrentClosureCanonicalBytesV1 {
        self.current_state_canonical_bytes
    }

    pub(crate) const fn retained_union_canonical_bytes(self) -> RetainedUnionCanonicalBytesV1 {
        self.retained_union_canonical_bytes
    }

    pub(crate) const fn retained_union_objects(self) -> u64 { self.retained_union_objects }

    pub(crate) const fn data_objects(self) -> u64 { self.data_objects }

    pub(crate) const fn data_canonical_bytes(self) -> CheckpointInlineCanonicalBytesV1 {
        self.data_canonical_bytes
    }

    pub(crate) const fn ordered_digest(self) -> OrderedObjectInventoryDigestV1 {
        self.ordered_digest
    }

    pub(crate) const fn format(self) -> RetentionFormat { self.format }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GenerationCheckpointErrorV1 {
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
    CurrentObjectsExceedRetained {
        current: u64,
        retained: u64,
    },
    CurrentObjectsBelowManifest {
        actual: u64,
    },
    CurrentExceedsRetained {
        current: u64,
        retained: u64,
    },
    CurrentBelowManifest {
        minimum: u64,
        actual: u64,
    },
    EmptyRetainedUnion,
    RetentionAnchorLength {
        actual: u64,
    },
    RetentionMetadataBelowMinimum {
        data_objects: u64,
        retained_objects: u64,
        data_bytes: u64,
        retained_bytes: u64,
    },
    SourceRetentionFormatNotWritable(RetentionFormat),
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
            Self::CurrentObjectsExceedRetained { current, retained } => write!(
                formatter,
                "checkpoint current-state inventory has {current} objects, exceeding the retained \
                 inventory's {retained} objects"
            ),
            Self::CurrentObjectsBelowManifest { actual } => write!(
                formatter,
                "checkpoint current-state inventory has {actual} objects but must contain its \
                 manifest"
            ),
            Self::CurrentExceedsRetained { current, retained } => write!(
                formatter,
                "checkpoint current-state inventory {current} exceeds retained inventory \
                 {retained}"
            ),
            Self::CurrentBelowManifest { minimum, actual } => write!(
                formatter,
                "checkpoint current-state inventory {actual} is below its {minimum}-byte manifest"
            ),
            Self::EmptyRetainedUnion => {
                formatter.write_str("checkpoint retained closure cannot be empty")
            }
            Self::RetentionAnchorLength { actual } => write!(
                formatter,
                "V2 checkpoint appoints a {actual}-byte retention object instead of the \
                 {RETENTION_ANCHOR_CANONICAL_LEN}-byte retention anchor"
            ),
            Self::RetentionMetadataBelowMinimum {
                data_objects,
                retained_objects,
                data_bytes,
                retained_bytes,
            } => write!(
                formatter,
                "V2 checkpoint DATA {data_objects} objects/{data_bytes} bytes leaves less than \
                 the {RETENTION_METADATA_MIN_OBJECTS}-object/\
                 {RETENTION_METADATA_MIN_CANONICAL_BYTES}-byte metadata minimum beside the \
                 {retained_objects}-object/{retained_bytes}-byte retained union"
            ),
            Self::SourceRetentionFormatNotWritable(format) => write!(
                formatter,
                "an admitted {format:?} retention source cannot derive a checkpoint before task \
                 7926 adds the V2 writer"
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

const _: () = assert!(CHECKPOINT_BODY_LEN == 184);
const _: () = assert!(GENERATION_CHECKPOINT_CANONICAL_LEN == 216);

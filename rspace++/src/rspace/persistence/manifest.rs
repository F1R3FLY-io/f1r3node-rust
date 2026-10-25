use super::canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, OBJECT_DIGEST_LEN, OBJECT_DOMAIN_LEN,
    OBJECT_ENVELOPE_HEADER_LEN, ObjectDigest, ObjectRefV1, ReadCursor, checked_len, checked_sum,
    parse_envelope, write_envelope_header, write_u64,
};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;

pub(crate) const SEMANTIC_MANIFEST_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceDAG.SMan\0\0";
pub(crate) const ACCELERATOR_MANIFEST_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceDAG.AMan\0\0";
const SEMANTIC_ROOT_COUNT: u64 = 3;
pub(crate) const SEMANTIC_SCHEMA_EPOCH_V1: u64 = 1;
pub(crate) const SEMANTIC_PROFILE_ID_LEN: usize = 16;
const OBJECT_REF_LEN: u64 = (OBJECT_DIGEST_LEN + 8) as u64;
pub(crate) const SEMANTIC_MANIFEST_MAX_CANONICAL_LEN: u64 = OBJECT_ENVELOPE_HEADER_LEN +
    8 +
    SEMANTIC_PROFILE_ID_LEN as u64 +
    8 +
    SEMANTIC_ROOT_COUNT * (8 + OBJECT_REF_LEN) +
    1 +
    OBJECT_DIGEST_LEN as u64;

pub type NamespaceRoot = ObjectRefV1;

/// Stable identity of the concrete channel/pattern/datum/continuation codec
/// profile used to interpret semantic keys and continuation-source payloads.
///
/// The profile is part of the authenticated manifest.  A caller therefore
/// cannot validate identical DAG bytes under one Rust type family and later
/// publish them under another interpretation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticProfileId([u8; SEMANTIC_PROFILE_ID_LEN]);

impl SemanticProfileId {
    pub const fn from_array(bytes: [u8; SEMANTIC_PROFILE_ID_LEN]) -> Self { Self(bytes) }

    pub const fn as_bytes(&self) -> &[u8; SEMANTIC_PROFILE_ID_LEN] { &self.0 }
}

/// Provenance for the frozen legacy history root from which a semantic
/// manifest was imported.
///
/// The mutable legacy-root-to-manifest mapping belongs to the store catalog;
/// keeping only this one-way value avoids a content-addressing cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LegacyHistoryRoot([u8; OBJECT_DIGEST_LEN]);

impl LegacyHistoryRoot {
    pub const fn from_array(bytes: [u8; OBJECT_DIGEST_LEN]) -> Self { Self(bytes) }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this conversion yet"
    )]
    pub fn from_legacy_hash(hash: &Blake2b256Hash) -> Result<Self, CanonicalFormatError> {
        let actual = hash.0.len();
        let bytes = hash
            .0
            .as_slice()
            .try_into()
            .map_err(|_| CanonicalFormatError::InvalidDigestLength { actual })?;
        Ok(Self(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; OBJECT_DIGEST_LEN] { &self.0 }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this conversion yet"
    )]
    pub fn to_legacy_hash(self) -> Blake2b256Hash { Blake2b256Hash::from_bytes(self.0.to_vec()) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u64)]
pub enum SemanticNamespace {
    DataBag = 1,
    ContinuationMap = 2,
    JoinBag = 3,
}

impl SemanticNamespace {
    pub const ALL: [Self; SEMANTIC_ROOT_COUNT as usize] =
        [Self::DataBag, Self::ContinuationMap, Self::JoinBag];

    pub const fn name(self) -> &'static str {
        match self {
            Self::DataBag => "data-bag",
            Self::ContinuationMap => "continuation-map",
            Self::JoinBag => "join-bag",
        }
    }

    pub(crate) fn parse(tag: u64) -> Result<Self, CanonicalFormatError> {
        match tag {
            1 => Ok(Self::DataBag),
            2 => Ok(Self::ContinuationMap),
            3 => Ok(Self::JoinBag),
            value => Err(CanonicalFormatError::InvalidSemanticNamespace(value)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticRoots {
    pub data_bag: NamespaceRoot,
    pub continuation_map: NamespaceRoot,
    pub join_bag: NamespaceRoot,
}

impl SemanticRoots {
    pub const fn iter(self) -> [(SemanticNamespace, NamespaceRoot); SEMANTIC_ROOT_COUNT as usize] {
        [
            (SemanticNamespace::DataBag, self.data_bag),
            (SemanticNamespace::ContinuationMap, self.continuation_map),
            (SemanticNamespace::JoinBag, self.join_bag),
        ]
    }

    fn from_ordered(roots: [NamespaceRoot; SEMANTIC_ROOT_COUNT as usize]) -> Self {
        Self {
            data_bag: roots[0],
            continuation_map: roots[1],
            join_bag: roots[2],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticStateManifestV1 {
    schema_epoch: u64,
    profile_id: SemanticProfileId,
    roots: SemanticRoots,
    legacy_history_root: Option<LegacyHistoryRoot>,
    body_len: u64,
    canonical_len: u64,
}

impl SemanticStateManifestV1 {
    pub fn new(
        schema_epoch: u64,
        profile_id: SemanticProfileId,
        roots: SemanticRoots,
        legacy_history_root: Option<LegacyHistoryRoot>,
    ) -> Result<Self, CanonicalFormatError> {
        if schema_epoch != SEMANTIC_SCHEMA_EPOCH_V1 {
            return Err(CanonicalFormatError::UnsupportedVersion {
                domain: "RSpace semantic schema",
                version: schema_epoch,
            });
        }
        let roots_len = SEMANTIC_ROOT_COUNT.checked_mul(8 + OBJECT_REF_LEN).ok_or(
            CanonicalFormatError::LengthOverflow {
                field: "semantic roots",
            },
        )?;
        let body_len = 8 +
            SEMANTIC_PROFILE_ID_LEN as u64 +
            8 +
            roots_len +
            1 +
            u64::from(legacy_history_root.is_some()) * 32;
        let canonical_len =
            checked_sum(OBJECT_ENVELOPE_HEADER_LEN, body_len, "semantic manifest envelope")?;
        Ok(Self {
            schema_epoch,
            profile_id,
            roots,
            legacy_history_root,
            body_len,
            canonical_len,
        })
    }

    #[expect(
        dead_code,
        reason = "unit tests read this field through SemanticStateManifestView instead"
    )]
    pub const fn schema_epoch(self) -> u64 { self.schema_epoch }

    #[expect(
        dead_code,
        reason = "unit tests read this field through SemanticStateManifestView instead"
    )]
    pub const fn profile_id(self) -> SemanticProfileId { self.profile_id }

    pub const fn roots(self) -> SemanticRoots { self.roots }

    #[expect(
        dead_code,
        reason = "unit tests read this field through SemanticStateManifestView instead"
    )]
    pub const fn legacy_history_root(self) -> Option<LegacyHistoryRoot> { self.legacy_history_root }

    pub fn object_ref(&self) -> ObjectRefV1 {
        ObjectRefV1::new(self.canonical_digest(), self.canonical_len)
            .expect("validated manifest length includes the canonical envelope")
    }
}

impl CanonicalEncode for SemanticStateManifestV1 {
    fn canonical_len(&self) -> u64 { self.canonical_len }

    fn write_canonical<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        write_envelope_header(sink, SEMANTIC_MANIFEST_DOMAIN, self.body_len)?;
        write_u64(sink, self.schema_epoch)?;
        sink.write_all(self.profile_id.as_bytes())?;
        write_u64(sink, SEMANTIC_ROOT_COUNT)?;
        for (namespace, root) in self.roots.iter() {
            write_u64(sink, namespace as u64)?;
            root.write(sink)?;
        }
        match self.legacy_history_root {
            None => sink.write_all(&[0]),
            Some(root) => {
                sink.write_all(&[1])?;
                sink.write_all(root.as_bytes())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticStateManifestView {
    bytes_len: u64,
    schema_epoch: u64,
    profile_id: SemanticProfileId,
    roots: SemanticRoots,
    legacy_history_root: Option<LegacyHistoryRoot>,
    digest: ObjectDigest,
}

impl SemanticStateManifestView {
    pub fn parse(bytes: &[u8]) -> Result<Self, CanonicalFormatError> {
        let body = parse_envelope(bytes, SEMANTIC_MANIFEST_DOMAIN, "RSpace semantic manifest")?;
        let mut cursor = ReadCursor::new(body);
        let schema_epoch = cursor.read_u64()?;
        if schema_epoch != SEMANTIC_SCHEMA_EPOCH_V1 {
            return Err(CanonicalFormatError::UnsupportedVersion {
                domain: "RSpace semantic schema",
                version: schema_epoch,
            });
        }
        let profile_id = SemanticProfileId::from_array(
            cursor
                .read_exact(SEMANTIC_PROFILE_ID_LEN)?
                .try_into()
                .expect("read exact semantic profile identity length"),
        );
        let root_count = cursor.read_u64()?;
        if root_count != SEMANTIC_ROOT_COUNT {
            return Err(CanonicalFormatError::SemanticRootCount(root_count));
        }
        let mut roots = [None; SEMANTIC_ROOT_COUNT as usize];
        for (index, expected) in SemanticNamespace::ALL.into_iter().enumerate() {
            let tag = cursor.read_u64()?;
            if tag != expected as u64 {
                return Err(CanonicalFormatError::SemanticRootOrder {
                    expected: expected as u64,
                    actual: tag,
                });
            }
            let namespace = SemanticNamespace::parse(tag)?;
            debug_assert_eq!(namespace, expected);
            roots[index] = Some(ObjectRefV1::parse(&mut cursor)?);
        }
        let legacy_history_root = match cursor.read_u8()? {
            0 => None,
            1 => {
                let bytes: [u8; OBJECT_DIGEST_LEN] = cursor
                    .read_exact(OBJECT_DIGEST_LEN)?
                    .try_into()
                    .expect("read exact legacy root length");
                Some(LegacyHistoryRoot::from_array(bytes))
            }
            tag => return Err(CanonicalFormatError::InvalidRootPresence(tag)),
        };
        cursor.finish()?;
        let roots = roots.map(|root| root.expect("all three roots were parsed"));
        Ok(Self {
            bytes_len: checked_len(bytes.len(), "semantic manifest")?,
            schema_epoch,
            profile_id,
            roots: SemanticRoots::from_ordered(roots),
            legacy_history_root,
            digest: ObjectDigest::of_canonical_bytes(bytes),
        })
    }

    pub const fn schema_epoch(self) -> u64 { self.schema_epoch }

    pub const fn profile_id(self) -> SemanticProfileId { self.profile_id }

    pub const fn roots(self) -> SemanticRoots { self.roots }

    pub const fn legacy_history_root(self) -> Option<LegacyHistoryRoot> { self.legacy_history_root }

    pub fn object_ref(self) -> ObjectRefV1 {
        ObjectRefV1::new(self.digest, self.bytes_len)
            .expect("parsed manifest length includes the canonical envelope")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u64)]
pub enum AcceleratorArtifactKind {
    ChannelReceives = 1,
    ContinuationIdentity = 2,
    CandidateCatalog = 3,
    SetAutomata = 4,
    ActImage = 5,
    CachedSummaries = 6,
}

impl AcceleratorArtifactKind {
    pub(crate) fn parse(tag: u64) -> Result<Self, CanonicalFormatError> {
        match tag {
            1 => Ok(Self::ChannelReceives),
            2 => Ok(Self::ContinuationIdentity),
            3 => Ok(Self::CandidateCatalog),
            4 => Ok(Self::SetAutomata),
            5 => Ok(Self::ActImage),
            6 => Ok(Self::CachedSummaries),
            value => Err(CanonicalFormatError::InvalidAcceleratorKind(value)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcceleratorArtifact {
    pub kind: AcceleratorArtifactKind,
    pub object: ObjectRefV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceleratorManifestV1 {
    semantic_manifest: ObjectRefV1,
    accelerator_epoch: u64,
    artifacts: Box<[AcceleratorArtifact]>,
    body_len: u64,
    canonical_len: u64,
}

impl AcceleratorManifestV1 {
    pub fn new(
        semantic_manifest: ObjectRefV1,
        accelerator_epoch: u64,
        artifacts: impl Into<Box<[AcceleratorArtifact]>>,
    ) -> Result<Self, CanonicalFormatError> {
        let artifacts = artifacts.into();
        validate_artifact_order(&artifacts)?;
        let count = checked_len(artifacts.len(), "accelerator artifacts")?;
        let artifact_len =
            count
                .checked_mul(8 + OBJECT_REF_LEN)
                .ok_or(CanonicalFormatError::LengthOverflow {
                    field: "accelerator artifacts",
                })?;
        let body_len = checked_sum(OBJECT_REF_LEN + 8 + 8, artifact_len, "accelerator manifest")?;
        let canonical_len =
            checked_sum(OBJECT_ENVELOPE_HEADER_LEN, body_len, "accelerator manifest envelope")?;
        Ok(Self {
            semantic_manifest,
            accelerator_epoch,
            artifacts,
            body_len,
            canonical_len,
        })
    }

    #[expect(
        dead_code,
        reason = "unit tests read this field through AcceleratorManifestView instead"
    )]
    pub const fn semantic_manifest(&self) -> ObjectRefV1 { self.semantic_manifest }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this accessor yet"
    )]
    pub const fn accelerator_epoch(&self) -> u64 { self.accelerator_epoch }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this accessor yet"
    )]
    pub fn artifacts(&self) -> &[AcceleratorArtifact] { &self.artifacts }

    pub fn object_ref(&self) -> ObjectRefV1 {
        ObjectRefV1::new(self.canonical_digest(), self.canonical_len)
            .expect("validated accelerator length includes the canonical envelope")
    }
}

impl CanonicalEncode for AcceleratorManifestV1 {
    fn canonical_len(&self) -> u64 { self.canonical_len }

    fn write_canonical<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        write_envelope_header(sink, ACCELERATOR_MANIFEST_DOMAIN, self.body_len)?;
        self.semantic_manifest.write(sink)?;
        write_u64(sink, self.accelerator_epoch)?;
        write_u64(sink, self.artifacts.len() as u64)?;
        for artifact in &self.artifacts {
            write_u64(sink, artifact.kind as u64)?;
            artifact.object.write(sink)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AcceleratorManifestView<'a> {
    bytes: &'a [u8],
    semantic_manifest: ObjectRefV1,
    accelerator_epoch: u64,
    count: usize,
    artifacts: &'a [u8],
}

impl<'a> AcceleratorManifestView<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, CanonicalFormatError> {
        let body =
            parse_envelope(bytes, ACCELERATOR_MANIFEST_DOMAIN, "RSpace accelerator manifest")?;
        let mut cursor = ReadCursor::new(body);
        let semantic_manifest = ObjectRefV1::parse(&mut cursor)?;
        let accelerator_epoch = cursor.read_u64()?;
        let count = cursor.read_len("accelerator artifact count")?;
        let encoded_len = count.checked_mul((8 + OBJECT_REF_LEN) as usize).ok_or(
            CanonicalFormatError::LengthOverflow {
                field: "accelerator artifacts",
            },
        )?;
        let artifacts = cursor.read_exact(encoded_len)?;
        cursor.finish()?;
        let mut previous = None;
        for artifact in artifacts.chunks_exact((8 + OBJECT_REF_LEN) as usize) {
            let mut artifact_cursor = ReadCursor::new(artifact);
            let tag = artifact_cursor.read_u64()?;
            let _ = AcceleratorArtifactKind::parse(tag)?;
            let _ = ObjectRefV1::parse(&mut artifact_cursor)?;
            if let Some(previous) = previous {
                if previous >= tag {
                    return Err(CanonicalFormatError::AcceleratorKindsNotStrict {
                        previous,
                        current: tag,
                    });
                }
            }
            previous = Some(tag);
        }
        Ok(Self {
            bytes,
            semantic_manifest,
            accelerator_epoch,
            count,
            artifacts,
        })
    }

    pub const fn semantic_manifest(self) -> ObjectRefV1 { self.semantic_manifest }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this accessor yet"
    )]
    pub const fn accelerator_epoch(self) -> u64 { self.accelerator_epoch }

    pub const fn artifact_count(self) -> usize { self.count }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this accessor yet"
    )]
    pub fn artifacts(self) -> AcceleratorArtifactIter<'a> {
        AcceleratorArtifactIter {
            chunks: self.artifacts.chunks_exact((8 + OBJECT_REF_LEN) as usize),
        }
    }

    pub fn object_ref(self) -> ObjectRefV1 {
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(self.bytes), self.bytes.len() as u64)
            .expect("parsed accelerator length includes the canonical envelope")
    }
}

pub struct AcceleratorArtifactIter<'a> {
    chunks: std::slice::ChunksExact<'a, u8>,
}

impl Iterator for AcceleratorArtifactIter<'_> {
    type Item = AcceleratorArtifact;

    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.chunks.next()?;
        let mut cursor = ReadCursor::new(bytes);
        let tag = cursor
            .read_u64()
            .expect("validated accelerator artifact contains its kind");
        let kind = AcceleratorArtifactKind::parse(tag).expect("validated accelerator kind");
        let object = ObjectRefV1::parse(&mut cursor).expect("validated accelerator reference");
        Some(AcceleratorArtifact { kind, object })
    }

    fn size_hint(&self) -> (usize, Option<usize>) { self.chunks.size_hint() }
}

impl ExactSizeIterator for AcceleratorArtifactIter<'_> {}

fn validate_artifact_order(artifacts: &[AcceleratorArtifact]) -> Result<(), CanonicalFormatError> {
    for pair in artifacts.windows(2) {
        let previous = pair[0].kind as u64;
        let current = pair[1].kind as u64;
        if previous >= current {
            return Err(CanonicalFormatError::AcceleratorKindsNotStrict { previous, current });
        }
    }
    Ok(())
}

//! Repository-owned, PathMap-backed representation of Rholang parallel composition.
//!
//! [`Par`] is the target of the `.rhoapi.Par` external-schema cutover. Its one
//! component authority is [`ParBag`], a prefix-compressed
//! `PathMap<Multiplicity>`. Root `locally_free` and `connective_used` values are
//! retained only as normalization summaries; there is no repeated-field or
//! decoded component shadow.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use prost::bytes::BufMut;
use rspace_plus_plus::rspace::bag::BagError;

use super::par_bag::{ParBag, ParBagProtobufError};

#[derive(Debug)]
pub enum ParProtobufError {
    Components(ParBagProtobufError),
    RootMetadataLengthOverflow,
}

impl fmt::Display for ParProtobufError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Components(error) => error.fmt(formatter),
            Self::RootMetadataLengthOverflow => {
                formatter.write_str("Par root metadata overflows the protobuf output length")
            }
        }
    }
}

impl std::error::Error for ParProtobufError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Components(error) => Some(error),
            Self::RootMetadataLengthOverflow => None,
        }
    }
}

impl From<ParBagProtobufError> for ParProtobufError {
    fn from(error: ParBagProtobufError) -> Self { Self::Components(error) }
}

#[repr(C)]
#[derive(Clone, Debug, Default)]
pub struct Par {
    components: ParBag,
    locally_free: Vec<u8>,
    connective_used: bool,
}

impl Par {
    pub fn from_parts(components: ParBag, locally_free: Vec<u8>, connective_used: bool) -> Self {
        Self {
            components,
            locally_free,
            connective_used,
        }
    }

    pub fn import_generated(value: &crate::rhoapi::Par) -> Result<Self, BagError> {
        Ok(Self::from_parts(
            ParBag::import_legacy(value)?,
            value.locally_free.clone(),
            value.connective_used,
        ))
    }

    pub const fn components(&self) -> &ParBag { &self.components }

    pub fn into_components(self) -> ParBag { self.components }

    pub fn locally_free(&self) -> &[u8] { &self.locally_free }

    pub const fn connective_used(&self) -> bool { self.connective_used }

    pub fn is_empty(&self) -> bool { self.components.is_empty() }

    pub fn parallel(self, other: Self) -> Result<Self, BagError> {
        Ok(Self {
            components: self.components.sum(&other.components)?,
            locally_free: super::utils::union(self.locally_free, other.locally_free),
            connective_used: self.connective_used || other.connective_used,
        })
    }

    pub fn protobuf_encoded_len(&self) -> Result<usize, ParProtobufError> {
        let mut total = 0usize;
        self.components
            .add_protobuf_components_before_root_metadata_len(&mut total)?;
        if !self.locally_free.is_empty() {
            total = total
                .checked_add(prost::encoding::bytes::encoded_len(9, &self.locally_free))
                .ok_or(ParProtobufError::RootMetadataLengthOverflow)?;
        }
        if self.connective_used {
            total = total
                .checked_add(prost::encoding::bool::encoded_len(
                    10,
                    &self.connective_used,
                ))
                .ok_or(ParProtobufError::RootMetadataLengthOverflow)?;
        }
        self.components
            .add_protobuf_components_after_root_metadata_len(&mut total)?;
        Ok(total)
    }

    pub fn encode_protobuf(&self, output: &mut impl BufMut) -> Result<(), ParProtobufError> {
        self.components
            .encode_protobuf_components_before_root_metadata(output)?;
        if !self.locally_free.is_empty() {
            prost::encoding::bytes::encode(9, &self.locally_free, output);
        }
        if self.connective_used {
            prost::encoding::bool::encode(10, &self.connective_used, output);
        }
        self.components
            .encode_protobuf_components_after_root_metadata(output)?;
        Ok(())
    }

    pub fn protobuf_bytes(&self) -> Result<Vec<u8>, ParProtobufError> {
        let mut output = Vec::with_capacity(self.protobuf_encoded_len()?);
        self.encode_protobuf(&mut output)?;
        Ok(output)
    }
}

impl PartialEq for Par {
    fn eq(&self, other: &Self) -> bool {
        self.components == other.components && self.connective_used == other.connective_used
    }
}

impl Eq for Par {}

impl PartialOrd for Par {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for Par {
    fn cmp(&self, other: &Self) -> Ordering {
        self.components
            .cmp(&other.components)
            .then_with(|| self.connective_used.cmp(&other.connective_used))
    }
}

impl Hash for Par {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.components.hash(state);
        self.connective_used.hash(state);
    }
}

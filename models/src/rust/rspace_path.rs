use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::ordered_receive::ReceivePatternLayout;
use rspace_plus_plus::rspace::rho_path::{RhoPathEncode, RhoPathV2, StructuralSequenceEncoder};

use super::canonical_path::encode_trie_path;
use super::channel_image::{ChannelImageError, ChannelImageV2};
use super::rholang::match_path::encode_par_match_atom;
use crate::rhoapi::{BindPattern, ListParWithRandom, Par};

const BIND_PATTERN_FREE_COUNT_WIDTH: usize = 4;
const BIND_PATTERN_MINIMUM_HEAD_WIDTH: usize = 9;

/// V2 datum adapter: the first envelope field is the canonical RCI2 image,
/// followed by caller-supplied canonical RMP2 structure and datum identity.
/// The trie codec receives bytes directly; no Par projection is performed.
pub fn rho_path_v2_datum_key(
    channel: &Par,
    datum_structure: &[u8],
    datum_identity: &[u8],
) -> Result<Vec<u8>, ChannelImageError> {
    let image = ChannelImageV2::from_par(channel)?;
    Ok(RhoPathV2::datum_key(
        image.as_bytes(),
        datum_structure,
        datum_identity,
    ))
}

/// V2 continuation adapter using canonical RCI2 images as channel members.
pub fn rho_path_v2_continuation_key(
    channels: &[Par],
    continuation_identity: &[u8],
) -> Result<Vec<u8>, ChannelImageError> {
    let images = channels
        .iter()
        .map(ChannelImageV2::from_par)
        .collect::<Result<Vec<_>, _>>()?;
    let refs = images
        .iter()
        .map(ChannelImageV2::as_bytes)
        .collect::<Vec<_>>();
    RhoPathV2::continuation_key(&refs, continuation_identity).map_err(ChannelImageError::OuterPath)
}

/// Build only the V2 continuation group prefix for PathMap subtree lookup.
pub fn rho_path_v2_continuation_prefix(channels: &[Par]) -> Result<Vec<u8>, ChannelImageError> {
    let images = channels
        .iter()
        .map(ChannelImageV2::from_par)
        .collect::<Result<Vec<_>, _>>()?;
    let refs = images
        .iter()
        .map(ChannelImageV2::as_bytes)
        .collect::<Vec<_>>();
    RhoPathV2::continuation_prefix(&refs).map_err(ChannelImageError::OuterPath)
}

/// V2 join adapter with a framed RCI2 indexed-channel image and framed join
/// identity.  The identity bytes remain opaque to this layer.
pub fn rho_path_v2_join_key(
    channel: &Par,
    join_identity: &[u8],
) -> Result<Vec<u8>, ChannelImageError> {
    let image = ChannelImageV2::from_par(channel)?;
    Ok(RhoPathV2::join_key(image.as_bytes(), join_identity))
}

/// Build only the V2 join group prefix for indexed-channel subtree lookup.
pub fn rho_path_v2_join_prefix(channel: &Par) -> Result<Vec<u8>, ChannelImageError> {
    let image = ChannelImageV2::from_par(channel)?;
    Ok(RhoPathV2::join_prefix(image.as_bytes()))
}

impl RhoPathEncode for Par {
    fn rho_path_structure(&self) -> Vec<u8> { encode_trie_path(self) }

    fn rho_path_identity(&self) -> Vec<u8> { self.stable_hash_bytes() }
}

impl rspace_plus_plus::rspace::rho_path::RhoPathChannelImage for Par {
    fn rho_path_channel_image(&self) -> Vec<u8> {
        ChannelImageV2::from_par(self)
            .expect("a resident Par has a canonical RCI2 channel image")
            .as_bytes()
            .to_vec()
    }

    fn from_rho_path_channel_image(image: &[u8]) -> Result<Self, String> {
        ChannelImageV2::try_from_bytes(image)
            .and_then(|admitted| admitted.decode_par())
            .map_err(|error| error.to_string())
    }
}

impl RhoPathEncode for ListParWithRandom {
    fn rho_path_structure(&self) -> Vec<u8> {
        let mut output = StructuralSequenceEncoder::with_capacity(self.pars.len());
        for par in &self.pars {
            output.push(&encode_par_match_atom(par));
        }
        output.finish()
    }

    fn rho_path_identity(&self) -> Vec<u8> { self.stable_hash_bytes() }
}

impl ReceivePatternLayout for BindPattern {
    fn trailing_free_count(pattern: &[u8]) -> Option<i32> {
        let (head, free_count) = pattern.split_last_chunk::<BIND_PATTERN_FREE_COUNT_WIDTH>()?;
        match head.len() {
            0..BIND_PATTERN_MINIMUM_HEAD_WIDTH => None,
            _ => Some(i32::from_le_bytes(*free_count)),
        }
    }
}

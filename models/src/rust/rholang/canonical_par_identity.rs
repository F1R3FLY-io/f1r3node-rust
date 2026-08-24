//! Trie-native identity indexing for canonical raw `Par` images.
//!
//! Rholang `Par` equality observes its component bag and root connective bit,
//! but deliberately ignores the root `locally_free` cache. Callers provide the
//! canonical protobuf byte stream with only that outer field omitted. The
//! stream descends a `PathMap` directly: no identity `Vec`, hash surrogate, or
//! decoded `Par` is introduced.

use pathmap::zipper::{ZipperMoving, ZipperValues, ZipperWriting};
use pathmap::PathMap;

/// Insert one exact canonical semantic-`Par` identity unless it is present.
///
/// Returning the resident value without replacing it is the primitive needed
/// by collection canonicalization: Rust's legacy `HashSet<&Par>` and
/// `HashMap<&Par, _>` retain the first equal key representative, even when two
/// representatives differ in the root `locally_free` cache ignored by `Par`
/// equality.
pub(crate) fn insert_semantic_par_identity<V: Clone + Send + Sync + Unpin>(
    identities: &mut PathMap<V>,
    bytes: impl IntoIterator<Item = u8>,
    value: V,
) -> Option<V> {
    let mut zipper = identities.write_zipper();
    for byte in bytes {
        zipper.descend_to_byte(byte);
    }
    if let Some(resident) = zipper.val() {
        Some(resident.clone())
    } else {
        debug_assert!(zipper.set_val(value).is_none());
        None
    }
}

/// Insert or replace one exact canonical semantic-`Par` identity.
///
/// The empty identity is represented by the PathMap root value. Returning the
/// replaced value makes the same primitive serve set membership, deterministic
/// winner selection, and admission duplicate detection.
pub(crate) fn replace_semantic_par_identity<V: Clone + Send + Sync + Unpin>(
    identities: &mut PathMap<V>,
    bytes: impl IntoIterator<Item = u8>,
    value: V,
) -> Option<V> {
    let mut zipper = identities.write_zipper();
    for byte in bytes {
        zipper.descend_to_byte(byte);
    }
    zipper.set_val(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_identity_uses_and_replaces_the_root_endpoint() {
        let mut identities = PathMap::new();
        assert_eq!(
            replace_semantic_par_identity(&mut identities, [], 3u8),
            None
        );
        assert_eq!(
            replace_semantic_par_identity(&mut identities, [], 7u8),
            Some(3)
        );
        assert_eq!(identities.get_val_at(&[]), Some(&7));
    }

    #[test]
    fn insert_preserves_the_first_root_endpoint_value() {
        let mut identities = PathMap::new();
        assert_eq!(insert_semantic_par_identity(&mut identities, [], 3u8), None);
        assert_eq!(
            insert_semantic_par_identity(&mut identities, [], 7u8),
            Some(3)
        );
        assert_eq!(identities.get_val_at(&[]), Some(&3));
    }
}

//! Integration layer between Rholang Par types and the PathMap crate.
//!
//! EPathMap wire W2b-1 (the global trie re-key): trie keys are now the
//! CANONICAL PATH CODEC bytes (`canonical_path::encode_trie_path`), not the
//! former `ParToSExpr` + `SExpr::encode` segments joined with a `0xFF`
//! separator. The codec is capless, injective, and prefix-free, with the
//! `0x0F` escape arm for non-`eval_stable` trie entries (R3F-2) and a depth
//! limit of 32 collection levels (R3F-7). Consequences of the flip
//! (design §3.1-§3.2): trie keys re-key globally, formerly-colliding paths
//! (`"(expr)"`/`"Nil"` degenerate keys) SEPARATE (a D-2 disclosed fix), and
//! the `0xFF`-separator machinery (this file's flatten, the
//! `pathmap_native_query` separator special-case, `SExpr::decode`
//! reconstruction) retires.
//!
//! `par_to_path` now returns the PER-ELEMENT codec segments (each element's
//! `encode_trie_segment`); full trie keys are built with [`segments_to_key`]
//! (concatenation + the split-list `0x00` terminator) or, for a whole entry
//! Par, directly with `canonical_path::encode_trie_path`. Callers that store
//! `current_path` segments are unaffected by the return type; only the
//! key-BUILD sites (the former `0xFF` flatten) change to the codec
//! concatenation.

use pathmap::PathMap;

use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Par, Var};
use crate::rust::canonical_path::{encode_trie_path, encode_trie_segment, tag};

/// Type alias for our standard use case: PathMap from bytes to Rholang Par.
pub type RholangPathMap = PathMap<Par>;

/// The per-element codec SEGMENTS of a path Par (W2b-1).
///
/// - A ground `EList` entry yields one segment per element
///   (`encode_trie_segment` of each), so a `current_path` of these segments
///   keeps the segment-count == element-count invariant the zipper relies on
///   (`existing_list.ps[..current_path.len()]` indexing, `starts_with`
///   prefix matching).
/// - Any other Par yields a single segment (`encode_trie_segment(par)`).
///
/// The FULL trie key of the entry is [`segments_to_key`] over these segments
/// (with the terminator for the split-list form) — equivalently
/// `canonical_path::encode_trie_path` applied to the whole entry.
pub fn par_to_path(par: &Par) -> Vec<Vec<u8>> {
    if par.exprs.len() == 1 {
        if let Some(ExprInstance::EListBody(list)) = &par.exprs[0].expr_instance {
            // Only a ground list splits into per-element segments; a list
            // carrying a remainder/free vars is not a ground path and falls
            // through to the single-segment (escape-capable) arm.
            if list.remainder.is_none() && list.locally_free.is_empty() && !list.connective_used {
                return list.ps.iter().map(|p| encode_trie_segment(p)).collect();
            }
        }
    }
    vec![encode_trie_segment(par)]
}

/// Build a trie key from per-element codec segments (W2b-1): concatenation,
/// with the split-list terminator (`0x00`) appended iff `terminate`.
///
/// - A FULL entry key (getLeaf/setLeaf/removeLeaf exact addressing) passes
///   `terminate = true` — matching `encode_trie_path` of the list path.
/// - A PREFIX key (getSubtrie/childCount/descend/pathExists) passes
///   `terminate = false`; codec segments are prefix-free, so the raw
///   concatenation is a clean trie prefix boundary.
pub fn segments_to_key(segments: &[Vec<u8>], terminate: bool) -> Vec<u8> {
    let total: usize = segments.iter().map(|s| s.len()).sum::<usize>() + terminate as usize;
    let mut key = Vec::with_capacity(total);
    for segment in segments {
        key.extend_from_slice(segment);
    }
    if terminate {
        key.push(tag::TERM);
    }
    key
}

/// Convenience return type—including the constructed map and related Rholang metadata.
pub struct PathMapCreationResult {
    pub map: RholangPathMap,
    pub connective_used: bool,
    pub locally_free: Vec<u8>,
}

/// Construct a RholangPathMap from a list of Par elements and an optional remainder.
/// This mirrors what the normalizer does when producing EPathMap from parsed elements.
///
/// W2b-1: each entry's key is `canonical_path::encode_trie_path(par)` — the
/// capless, injective, prefix-free codec path (split form for ground lists,
/// bare form otherwise, `0x0F` escape for non-`eval_stable` entries). This
/// replaces the former `par_to_path` + `0xFF` flatten; formerly-colliding
/// degenerate keys now separate.
pub fn create_pathmap_from_elements(
    elements: &[Par],
    remainder: Option<Var>,
) -> PathMapCreationResult {
    let mut map = RholangPathMap::new();
    let mut connective_used = false;
    let mut locally_free = Vec::new();

    for par in elements {
        // Update connective metadata
        if par.connective_used {
            connective_used = true;
        }
        locally_free = crate::rust::utils::union(locally_free.clone(), par.locally_free.clone());

        // The codec trie key (capless, injective, prefix-free). No separator
        // — the path is self-delimiting.
        let key = encode_trie_path(par);
        map.insert(key, par.clone());
    }

    if remainder.is_some() {
        connective_used = true;
    }

    PathMapCreationResult {
        map,
        connective_used,
        locally_free,
    }
}

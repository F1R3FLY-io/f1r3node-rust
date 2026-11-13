// Shared binary-Merkle-tree primitives over Blake2b256.
//
// Two existing hash constructions use a Merkle tree:
//
//   * Snapshot chunk-fetch (rholang/src/rust/interpreter/io/snapshot_chunk.rs)
//   * Casper block-hash computation (casper/src/rust/util/proto_util.rs)
//
// Both want: domain-separated leaves and interior nodes, odd-tail
// promotion (no duplicate-and-hash), an EMPTY sentinel = [0u8;32].
// This module provides those primitives parameterized on caller-chosen
// domain bytes so each caller's hard-fork surface stays its own.
//
// # Shape
//
// Binary Merkle tree with:
//   * Leaves       = `Blake2b256(leaf_domain || leaf_bytes)`.  The
//                    `leaf_domain` is passed per call to `hash_leaf_tagged`
//                    because some callers (block hashing) use multiple
//                    leaf-domain bytes to distinguish leaf kinds.
//   * Interior     = `Blake2b256(interior_domain || left || right)`,
//                    where `interior_domain` lives on `MerkleParams`.
//   * Odd-tail     = promoted unchanged (no duplicate-and-hash), matching
//                    Bitcoin/Ethereum shape.
//   * Empty tree   = `EMPTY_ROOT` sentinel.
//
// The domain-separation prefixes are the standard second-preimage-
// resistance construction (NEAR, Certificate Transparency, RFC 6962) —
// leaf and interior hashes come from disjoint input spaces, so no two
// structurally-different trees can share a root through the
// promote-unchanged step even in the (infeasible) preimage-collision case.

use crate::rust::hash::blake2b256::Blake2b256;

/// Sentinel root for an empty tree (0-leaf case).  A well-formed non-empty
/// tree cannot produce this root because Blake2b256 is preimage-resistant
/// AND every real leaf/interior hash is domain-tagged.
pub const EMPTY_ROOT: [u8; 32] = [0u8; 32];

/// Caller-chosen parameters.  `interior_domain` is the byte prepended to
/// `left || right` before hashing an interior node.  Leaf-domain bytes
/// are passed per call to `hash_leaf_tagged` so a caller that wants
/// multiple leaf kinds (e.g. block hashing) can use several domain bytes
/// without needing separate `MerkleParams` instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MerkleParams {
    pub interior_domain: u8,
}

impl MerkleParams {
    pub const fn new(interior_domain: u8) -> Self { Self { interior_domain } }
}

/// Merkle inclusion proof for a leaf.  `siblings` is bottom-up
/// (leaf-level first, root's-child last).  Each entry is
/// `(sibling_hash, sibling_is_right)` where `sibling_is_right` indicates
/// whether the sibling is the RIGHT child of its parent (in which case
/// the verifier concatenates `current || sibling`) or LEFT
/// (`sibling || current`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerkleProof {
    pub index: u32,
    pub siblings: Vec<([u8; 32], bool)>,
}

/// Domain-tagged leaf hash: `Blake2b256(leaf_domain || bytes)`.
pub fn hash_leaf_tagged(leaf_domain: u8, bytes: &[u8]) -> [u8; 32] {
    let mut input = Vec::with_capacity(1 + bytes.len());
    input.push(leaf_domain);
    input.extend_from_slice(bytes);
    hash_owned(input)
}

/// Domain-tagged interior hash:
/// `Blake2b256(params.interior_domain || left || right)`.
pub fn hash_interior(params: &MerkleParams, left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut input = [0u8; 1 + 32 + 32];
    input[0] = params.interior_domain;
    input[1..33].copy_from_slice(left);
    input[33..].copy_from_slice(right);
    hash_owned(input.to_vec())
}

/// Compute the Merkle root over the given leaf hashes.  The empty case
/// returns `EMPTY_ROOT`; a single-leaf tree returns the leaf hash
/// unchanged (consistent with the "promote odd tail" rule applied all
/// the way up).
pub fn merkle_root(params: &MerkleParams, leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return EMPTY_ROOT;
    }
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut i = 0;
        while i + 1 < level.len() {
            next.push(hash_interior(params, &level[i], &level[i + 1]));
            i += 2;
        }
        // Odd tail: promote unchanged (no duplicate-and-hash).
        if i < level.len() {
            next.push(level[i]);
        }
        level = next;
    }
    level[0]
}

/// Build the inclusion proof for `index` against the given leaf
/// sequence.  Returns `None` if `index` is out of range or the leaf
/// sequence is empty.
pub fn build_proof(params: &MerkleParams, leaves: &[[u8; 32]], index: u32) -> Option<MerkleProof> {
    let index_usize = usize::try_from(index).ok()?;
    if leaves.is_empty() || index_usize >= leaves.len() {
        return None;
    }
    let mut siblings = Vec::new();
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    let mut idx = index_usize;
    while level.len() > 1 {
        let is_idx_left = idx % 2 == 0;
        let sib_idx = if is_idx_left { idx + 1 } else { idx - 1 };
        if sib_idx < level.len() {
            siblings.push((level[sib_idx], is_idx_left));
        }
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut i = 0;
        while i + 1 < level.len() {
            next.push(hash_interior(params, &level[i], &level[i + 1]));
            i += 2;
        }
        if i < level.len() {
            next.push(level[i]);
        }
        idx /= 2;
        level = next;
    }
    Some(MerkleProof { index, siblings })
}

/// Verify that `leaf_hash` at `proof.index` participates in the tree
/// rooted at `root`.  Constant work per proof step.
pub fn verify_proof(
    params: &MerkleParams,
    root: &[u8; 32],
    leaf_hash: &[u8; 32],
    proof: &MerkleProof,
) -> bool {
    let mut current = *leaf_hash;
    for (sibling, sibling_is_right) in &proof.siblings {
        current = if *sibling_is_right {
            hash_interior(params, &current, sibling)
        } else {
            hash_interior(params, sibling, &current)
        };
    }
    current == *root
}

fn hash_owned(input: Vec<u8>) -> [u8; 32] {
    let h = Blake2b256::hash(input);
    assert_eq!(h.len(), 32, "Blake2b256 must produce 32-byte digest");
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAPSHOT: MerkleParams = MerkleParams::new(0x01);

    fn leaf(domain: u8, b: u8) -> [u8; 32] { hash_leaf_tagged(domain, &[b]) }

    #[test]
    fn empty_tree_returns_empty_root() {
        assert_eq!(merkle_root(&SNAPSHOT, &[]), EMPTY_ROOT);
    }

    #[test]
    fn single_leaf_root_equals_leaf() {
        let only = leaf(0x00, 0xAA);
        assert_eq!(merkle_root(&SNAPSHOT, &[only]), only);
    }

    #[test]
    fn two_leaf_root_matches_manual_interior() {
        let a = leaf(0x00, 0x01);
        let b = leaf(0x00, 0x02);
        let expected = hash_interior(&SNAPSHOT, &a, &b);
        assert_eq!(merkle_root(&SNAPSHOT, &[a, b]), expected);
    }

    #[test]
    fn three_leaf_root_promotes_odd_tail() {
        // Level 0: [a, b, c]  → Level 1: [H(a,b), c]  → Root: H(H(a,b), c)
        let a = leaf(0x00, 0x01);
        let b = leaf(0x00, 0x02);
        let c = leaf(0x00, 0x03);
        let ab = hash_interior(&SNAPSHOT, &a, &b);
        let expected = hash_interior(&SNAPSHOT, &ab, &c);
        assert_eq!(merkle_root(&SNAPSHOT, &[a, b, c]), expected);
    }

    #[test]
    fn seven_leaf_root_computed_manually() {
        // Level 0: 7 leaves
        // Level 1: H(0,1) H(2,3) H(4,5) 6
        // Level 2: H(H01,H23) H(H45,6)
        // Root  : H(..., ...)
        let leaves: Vec<[u8; 32]> = (0u8..7).map(|i| leaf(0x00, i)).collect();
        let l1_0 = hash_interior(&SNAPSHOT, &leaves[0], &leaves[1]);
        let l1_1 = hash_interior(&SNAPSHOT, &leaves[2], &leaves[3]);
        let l1_2 = hash_interior(&SNAPSHOT, &leaves[4], &leaves[5]);
        let l1_3 = leaves[6];
        let l2_0 = hash_interior(&SNAPSHOT, &l1_0, &l1_1);
        let l2_1 = hash_interior(&SNAPSHOT, &l1_2, &l1_3);
        let expected = hash_interior(&SNAPSHOT, &l2_0, &l2_1);
        assert_eq!(merkle_root(&SNAPSHOT, &leaves), expected);
    }

    #[test]
    fn eight_leaf_root_is_balanced_tree() {
        let leaves: Vec<[u8; 32]> = (0u8..8).map(|i| leaf(0x00, i)).collect();
        let l1: Vec<[u8; 32]> = (0..4)
            .map(|k| hash_interior(&SNAPSHOT, &leaves[2 * k], &leaves[2 * k + 1]))
            .collect();
        let l2: Vec<[u8; 32]> = (0..2)
            .map(|k| hash_interior(&SNAPSHOT, &l1[2 * k], &l1[2 * k + 1]))
            .collect();
        let expected = hash_interior(&SNAPSHOT, &l2[0], &l2[1]);
        assert_eq!(merkle_root(&SNAPSHOT, &leaves), expected);
    }

    #[test]
    fn proof_round_trip_for_every_index() {
        for n in [1usize, 2, 3, 4, 5, 7, 8, 15, 16, 33] {
            let leaves: Vec<[u8; 32]> = (0..n)
                .map(|i| leaf(0x00, (i as u8).wrapping_add(1)))
                .collect();
            let root = merkle_root(&SNAPSHOT, &leaves);
            for (i, leaf_hash) in leaves.iter().enumerate() {
                let proof = build_proof(&SNAPSHOT, &leaves, i as u32)
                    .unwrap_or_else(|| panic!("proof for i={i} n={n}"));
                assert!(
                    verify_proof(&SNAPSHOT, &root, leaf_hash, &proof),
                    "proof for leaf {i} in tree of {n} did not verify",
                );
            }
        }
    }

    #[test]
    fn proof_rejects_wrong_leaf() {
        let leaves: Vec<[u8; 32]> = (0u8..4).map(|i| leaf(0x00, i)).collect();
        let root = merkle_root(&SNAPSHOT, &leaves);
        let proof = build_proof(&SNAPSHOT, &leaves, 1).unwrap();
        assert!(!verify_proof(&SNAPSHOT, &root, &leaves[0], &proof));
    }

    #[test]
    fn proof_rejects_bogus_root() {
        let leaves: Vec<[u8; 32]> = (0u8..4).map(|i| leaf(0x00, i)).collect();
        let proof = build_proof(&SNAPSHOT, &leaves, 0).unwrap();
        let bogus = [0xFFu8; 32];
        assert!(!verify_proof(&SNAPSHOT, &bogus, &leaves[0], &proof));
    }

    #[test]
    fn build_proof_out_of_range_returns_none() {
        let leaves: Vec<[u8; 32]> = (0u8..3).map(|i| leaf(0x00, i)).collect();
        assert!(build_proof(&SNAPSHOT, &leaves, 99).is_none());
        assert!(build_proof(&SNAPSHOT, &[], 0).is_none());
    }

    #[test]
    fn leaf_tag_changes_hash() {
        let a = hash_leaf_tagged(0x00, b"hello");
        let b = hash_leaf_tagged(0x01, b"hello");
        assert_ne!(a, b);
    }

    #[test]
    fn interior_domain_changes_root() {
        let leaves: Vec<[u8; 32]> = (0u8..4).map(|i| leaf(0x00, i)).collect();
        let r0 = merkle_root(&MerkleParams::new(0x01), &leaves);
        let r1 = merkle_root(&MerkleParams::new(0x02), &leaves);
        assert_ne!(r0, r1);
    }

    // Note: no module-level golden-hex pin here. Shape drift is
    // caught byte-identically by the snapshot_chunk callers'
    // existing `merkle_tree_shape_pinned_by_golden_hex` test (which
    // routes through this module after extraction), and by the
    // casper block-hash callers' own golden fixtures. The manual
    // interior-recomputation tests above cover shape at this layer.
}

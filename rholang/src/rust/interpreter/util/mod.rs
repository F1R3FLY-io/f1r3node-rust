use models::rhoapi::{Bundle, Connective, Expr, If, Match, New, Par, Receive, Send};
use models::rust::utils::union;

use super::matcher::has_locally_free::{
    connective_connective_used_ref, connective_locally_free_ref, expr_connective_used_ref,
    expr_locally_free_ref,
};

pub mod address_tools;
pub mod base58;
pub mod vault_address;
#[cfg(feature = "chromadb")]
pub mod sbert_embeddings;

// Helper enum. This is 'GeneratedMessage' in Scala
#[derive(Clone, Debug)]
pub enum GeneratedMessage {
    Send(Send),
    Receive(Receive),
    New(New),
    Match(Match),
    Bundle(Bundle),
    Expr(Expr),
    If(If),
}

// These two functions need to be under 'rholang' dir because of HasLocallyFree Trait.
// This trait should, I think, be moved to models

// ===========================================================================
// LEG-1 (the `prepend_*` amplifier).
//
// Every one of these functions used to perform FOUR deep clones per call:
//
//     let mut new_exprs = vec![e.clone()];                                    // 1
//     locally_free: union(p.locally_free.clone(), e.locally_free(e.clone(), depth)), // 2, 3
//     ..p.clone()                                                             // 4  <- p is OWNED
//
// `<Par as Clone>::clone` and `<Expr as Clone>::clone` are themselves
// Theta(depth) NATIVE-STACK traversals (measured 15.50 KiB/level debug,
// 2.78 KiB/level release), and `prepend_expr` is called once per expression at
// EVERY level of `sub_exp`'s fold — so a term of depth D paid O(D^2) heap copying
// and stacked a second depth-linear consumer on top of substitution's own.
//
// The rewrite below is a pure Leg-1 change: `p` is owned, so its untouched
// fields are simply KEPT (no `..p.clone()` struct-update is needed at all), the
// prepended node is MOVED into the vector rather than cloned, and the cached
// `locally_free` / `connective_used` fields are read through the by-reference
// readers in `matcher::has_locally_free` instead of through the by-value trait
// methods that deep-clone their argument.
//
// Observationally identical by construction: same vector order, same bitset
// union, same boolean, same residual fields. Nothing here charges — the
// substitution charge is levied once, on the RESULT, in
// `Substitute::substitute_and_charge`. See
// `docs/design/audits/theta-depth-traversals-2026-07-26.md`.
// ===========================================================================

// See models/src/main/scala/coop/rchain/models/rholang/implicits.scala - prepend
pub fn prepend_connective(mut p: Par, c: Connective, depth: i32) -> Par {
    let locally_free = connective_locally_free_ref(&c, depth);
    let connective_used = p.connective_used || connective_connective_used_ref(&c);

    let mut new_connectives = Vec::with_capacity(p.connectives.len() + 1);
    new_connectives.push(c);
    new_connectives.append(&mut p.connectives);

    p.connectives = new_connectives;
    p.locally_free = locally_free;
    p.connective_used = connective_used;
    p
}

pub fn prepend_expr(mut p: Par, e: Expr, depth: i32) -> Par {
    let locally_free = union(
        std::mem::take(&mut p.locally_free),
        expr_locally_free_ref(&e, depth),
    );
    let connective_used = p.connective_used || expr_connective_used_ref(&e);

    let mut new_exprs = Vec::with_capacity(p.exprs.len() + 1);
    new_exprs.push(e);
    new_exprs.append(&mut p.exprs);

    p.exprs = new_exprs;
    p.locally_free = locally_free;
    p.connective_used = connective_used;
    p
}

pub fn prepend_new(mut p: Par, n: New) -> Par {
    // `<New as HasLocallyFree<New>>::connective_used` reads `n.p.connective_used`
    // (a cached field on the immediate child), so it is O(1) once `n` is not
    // cloned; `locally_free` is `n`'s own cached bitset.
    let locally_free = union(std::mem::take(&mut p.locally_free), n.locally_free.clone());
    let connective_used = p.connective_used
        || n.p
            .as_ref()
            .expect("prepend_new: New with no body")
            .connective_used;

    let mut new_news = Vec::with_capacity(p.news.len() + 1);
    new_news.push(n);
    new_news.append(&mut p.news);

    p.news = new_news;
    p.locally_free = locally_free;
    p.connective_used = connective_used;
    p
}

pub fn prepend_bundle(mut p: Par, b: Bundle) -> Par {
    let locally_free = union(
        std::mem::take(&mut p.locally_free),
        b.body
            .as_ref()
            .expect("prepend_bundle: Bundle with no body")
            .locally_free
            .clone(),
    );

    let mut new_bundles = Vec::with_capacity(p.bundles.len() + 1);
    new_bundles.push(b);
    new_bundles.append(&mut p.bundles);

    p.bundles = new_bundles;
    p.locally_free = locally_free;
    p
}

/// A binder's escape: the indices its body names that the binder does not own,
/// renumbered into the binder's own index space.
///
/// This is the Rust rendering of the Scala the call sites cite,
///
/// ```scala
/// bodyResult.par.locallyFree.from(boundCount).map(x => x - boundCount)
/// ```
///
/// where `locallyFree` is a `scala.collection.immutable.BitSet`: `from(n)` keeps
/// the **members** `>= n` and `map(_ - n)` renumbers them.
///
/// # ⚠ The representation, stated before the law that depends on it
///
/// A `locally_free` bitset in this port is **one byte per de Bruijn index** —
/// not a packed bitmap, and not a list of indices. That is
/// [`models::create_bit_vector`]'s definition (`vec![0; max_index + 1]`, then
/// `bit_vector[index] = 1`) and it is what [`models::rust::utils::union`]'s
/// element-wise `OR` requires. **A member's identity IS its position.** So
/// Scala's two steps collapse into one:
///
/// ```text
///     output[j] = bitset[j + bound_count]
/// ```
///
/// — drop the first `bound_count` bytes and keep the rest, in place. There is
/// nothing to subtract, because nothing is stored that could be subtracted from:
/// a version of this function that mapped `i` to `i - bound_count` would be
/// emitting the shifted POSITION where the VALUE belongs, and would answer `[0]`
/// ("index 0 is not free") for the input `[0, 1]` at `bound_count = 1`, whose
/// correct answer is `[1]` ("index 0 is free").
///
/// # ★ Its twin, so the family can be read in one place
///
/// [`crate::rust::interpreter::substitute_combine::set_bits_until`] is the other
/// half: Scala's `BitSet.until(n)` keeps the members `< n`, which in this
/// representation is the **prefix** `bitset[..n]`, and it is written that way
/// ("truncate the bitvector at `until` positions, preserving bit positions").
/// Prefix and suffix, one representation, one reading — and they **partition**
/// their input, which `binder_shift_law::the_prefix_and_the_suffix_partition_the_bitset`
/// asserts rather than describes.
///
/// # The invariant this preserves, and who depends on it
///
/// No production bitset ends in a zero byte: they are built from `Vec::new()`,
/// from `create_bit_vector(&[i])` for an index that actually occurs (last byte
/// `1`), or from `union`/this function over those. `union` preserves it (the
/// longer operand's last byte is `1`, and `1 | 0 = 1`) and so does a suffix of a
/// vector whose last byte is `1`. That invariant is what makes the four
/// `locally_free.is_empty()` readers in
/// [`crate::rust::interpreter::matcher::fold_match`] and
/// [`crate::rust::interpreter::matcher::list_match`] sound: `[]` is the only
/// spelling of `∅`, so a `Vec` length query IS a set-emptiness query.
pub(crate) fn filter_and_adjust_bitset(mut bitset: Vec<u8>, bound_count: usize) -> Vec<u8> {
    match bound_count >= bitset.len() {
        // Every index the body names is bound HERE: nothing escapes.
        true => Vec::new(),
        // The suffix, in place — the input is owned, so this is one memmove and
        // no allocation.
        false => {
            bitset.drain(..bound_count);
            bitset
        }
    }
}

// ===========================================================================
// ★ THE BINDER SHIFT — the law `filter_and_adjust_bitset` must satisfy
// ===========================================================================
//
// ## The oracle works in INDEX SPACE, which is why it cannot share the defect
//
// Scala computes a binder's escape on a `scala.collection.immutable.BitSet`, a
// SET OF INDICES:
//
// ```scala
// bodyResult.par.locallyFree.from(boundCount).map(x => x - boundCount)
// ```
//
// `from(n)` keeps the MEMBERS `>= n`; `map(_ - n)` renumbers them into the
// parent's index space. The oracle below is that expression, transliterated
// over `&[usize]` index sets, and only then rendered into this port's byte
// representation with `models::create_bit_vector`. It never names a byte
// position, so a reading that confused a position with a member cannot be
// written twice.
#[cfg(test)]
mod binder_shift_law {
    use models::create_bit_vector;

    use super::filter_and_adjust_bitset;

    /// `create_bit_vector` with the empty set mapped to the empty vector.
    ///
    /// ⚠ `create_bit_vector(&[])` is `vec![0; 0 + 1]` = `[0]` — a length-one
    /// vector whose only byte is zero. That is a SECOND spelling of the empty
    /// set, and it is not the one production uses: every production bitset is
    /// built from `Vec::new()`, from `create_bit_vector(&[i])` for a single
    /// occurring index, or from `union`/this function over those, none of which
    /// can produce a trailing zero. So the oracle renders `∅` as `[]`.
    fn bits(indices: &[usize]) -> Vec<u8> {
        match indices {
            [] => Vec::new(),
            some => create_bit_vector(some),
        }
    }

    /// `locallyFree.from(n).map(_ - n)`, on index sets.
    fn escapes(indices: &[usize], bound_count: usize) -> Vec<usize> {
        indices
            .iter()
            .copied()
            .filter(|&i| i >= bound_count)
            .map(|i| i - bound_count)
            .collect()
    }

    /// The corpus: every member set over indices `0..=3`, against every binder
    /// arity `0..=4`. 16 × 5 = 80 rows, so the law is checked on the whole
    /// lattice rather than on a chosen example.
    fn corpus() -> Vec<(Vec<usize>, usize)> {
        let mut rows = Vec::with_capacity(16 * 5);
        for mask in 0u32..16 {
            let members: Vec<usize> = (0usize..4).filter(|i| mask & (1 << i) != 0).collect();
            for bound_count in 0usize..=4 {
                rows.push((members.clone(), bound_count));
            }
        }
        rows
    }

    /// ★ THE LAW. `filter_and_adjust_bitset` renders `from(n).map(_ - n)`.
    #[test]
    fn the_shift_keeps_the_members_and_renumbers_them() {
        let rows = corpus();
        // ⚠ Non-vacuity floor: a corpus that degenerated to all-closed inputs,
        // or a renderer that always produced `[]`, would satisfy the law
        // trivially. At least this many rows must have a NON-EMPTY expectation.
        const NON_TRIVIAL_FLOOR: usize = 30;
        let mut non_trivial = 0usize;

        for (members, bound_count) in &rows {
            let expected = bits(&escapes(members, *bound_count));
            if !expected.is_empty() {
                non_trivial += 1;
            }
            assert_eq!(
                filter_and_adjust_bitset(bits(members), *bound_count),
                expected,
                "members {members:?} escaping {bound_count} binder(s): \
                 `from({bound_count}).map(_ - {bound_count})` = {:?}",
                escapes(members, *bound_count)
            );
        }
        assert!(
            non_trivial >= NON_TRIVIAL_FLOOR,
            "the corpus must exercise the SURVIVING case: only {non_trivial} of {} rows had a \
             non-empty expectation (floor {NON_TRIVIAL_FLOOR})",
            rows.len()
        );
    }

    /// The single row the defect report named, spelled out so a reader can see
    /// the two answers side by side without running the lattice.
    ///
    /// One name is bound here; the body names index 1. Index 1 survives and is
    /// renumbered to 0, so the answer is "index 0 is free" = `[1]`. The
    /// pre-fix answer was `[0]` — index 0 is NOT free, i.e. the empty set with
    /// a trailing zero.
    #[test]
    fn index_one_surviving_one_binder_is_index_zero_free() {
        assert_eq!(filter_and_adjust_bitset(vec![0, 1], 1), vec![1]);
    }

    /// The complement: an index the binder OWNS is discharged, not renumbered.
    #[test]
    fn index_zero_under_one_binder_is_discharged() {
        assert_eq!(filter_and_adjust_bitset(vec![1], 1), Vec::<u8>::new());
    }

    /// A zero-arity binder is the identity — nothing to drop, nothing to shift.
    #[test]
    fn a_zero_arity_binder_is_the_identity() {
        for members in [vec![], vec![0], vec![1], vec![0, 2], vec![0, 1, 2, 3]] {
            assert_eq!(
                filter_and_adjust_bitset(bits(&members), 0),
                bits(&members),
                "members {members:?} under zero binders"
            );
        }
    }

    /// ★ The representation invariant the four `Vec::is_empty()` readers in
    /// `matcher::{fold_match, list_match}` depend on: **no trailing zero**.
    ///
    /// Those readers ask `locally_free.is_empty()` and mean "the SET is empty".
    /// That is only sound if `[]` is the sole spelling of `∅`, i.e. if no bitset
    /// ever ends in a zero byte. `union` preserves it (the longer operand's last
    /// byte is `1`, and `1 | 0 = 1`) and so must this function.
    #[test]
    fn the_shift_never_produces_a_trailing_zero() {
        for (members, bound_count) in corpus() {
            let input = bits(&members);
            assert_ne!(
                input.last(),
                Some(&0),
                "the oracle itself must honour the invariant it is checking"
            );
            let output = filter_and_adjust_bitset(input, bound_count);
            assert_ne!(
                output.last(),
                Some(&0),
                "members {members:?} escaping {bound_count} binder(s) produced {output:?}, whose \
                 last byte is zero — `Vec::is_empty()` is then no longer a set-emptiness query"
            );
        }
    }

    /// ⚠ The twin, asserted rather than described. `set_bits_until` is Scala's
    /// `BitSet.until(n)` — the members `< n` — and in this representation that
    /// is the PREFIX. Prefix and suffix must PARTITION the input: the bytes
    /// `set_bits_until` keeps and the bytes `filter_and_adjust_bitset` keeps,
    /// concatenated, are the input again. That is the one statement that pins
    /// both halves of the family to the same reading of the representation.
    #[test]
    fn the_prefix_and_the_suffix_partition_the_bitset() {
        use crate::rust::interpreter::substitute_combine::set_bits_until;

        for (members, bound_count) in corpus() {
            let input = bits(&members);
            let prefix = set_bits_until(input.clone(), bound_count as i32);
            let suffix = filter_and_adjust_bitset(input.clone(), bound_count);
            let mut rejoined = prefix.clone();
            rejoined.extend_from_slice(&suffix);
            // The prefix is `min(bound_count, len)` bytes; the suffix is the
            // rest. Their concatenation is the input, byte for byte.
            assert_eq!(
                rejoined, input,
                "members {members:?} at {bound_count}: until={prefix:?} ++ from={suffix:?} must \
                 rejoin to {input:?}"
            );
        }
    }
}

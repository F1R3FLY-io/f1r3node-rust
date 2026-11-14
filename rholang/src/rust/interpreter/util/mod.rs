use models::rhoapi::{
    Bundle, Connective, CostSignedTerm, Expr, If, Match, New, Par, Receive, Send,
};
use models::rust::utils::union;

use super::matcher::has_locally_free::HasLocallyFree;

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
    CostSignedTerm(CostSignedTerm),
    If(If),
}

// These two functions need to be under 'rholang' dir because of HasLocallyFree Trait.
// This trait should, I think, be moved to models

// See models/src/main/scala/coop/rchain/models/rholang/implicits.scala - prepend
pub fn prepend_connective(mut p: Par, c: Connective, depth: i32) -> Par {
    p.locally_free = c.locally_free(depth);
    p.connective_used |= c.connective_used();
    p.connectives.insert(0, c);
    p
}

pub fn prepend_expr(mut p: Par, e: Expr, depth: i32) -> Par {
    p.locally_free = union(std::mem::take(&mut p.locally_free), e.locally_free(depth));
    p.connective_used |= e.connective_used();
    p.exprs.insert(0, e);
    p
}

pub fn prepend_new(mut p: Par, n: New) -> Par {
    p.locally_free = union(std::mem::take(&mut p.locally_free), n.locally_free.clone());
    p.connective_used |= n.connective_used();
    p.news.insert(0, n);
    p
}

pub fn prepend_bundle(mut p: Par, b: Bundle) -> Par {
    let body = b.body.as_ref().expect("a bundle always has a body");
    p.locally_free = union(
        std::mem::take(&mut p.locally_free),
        body.locally_free.clone(),
    );
    p.bundles.insert(0, b);
    p
}

// Rust port of Scala's `locallyFree.from(boundCount).map(x => x - boundCount)`:
// keep bits at positions >= bound_count, shift them down by bound_count.
//
// The bitset representation here is byte-per-bit (`Vec<u8>`, `bitset_contains`
// returns `v[i] != 0`). An earlier port iterated positions and emitted the
// shifted *index* as the output byte, ignoring each input byte's value —
// losing bits set at position exactly `bound_count` and inventing phantom
// bits above it. Slicing from `bound_count` preserves values byte-for-byte.
pub(crate) fn filter_and_adjust_bitset(bitset: Vec<u8>, bound_count: usize) -> Vec<u8> {
    if bitset.len() <= bound_count {
        return Vec::new();
    }
    bitset[bound_count..].to_vec()
}

#[cfg(test)]
mod filter_and_adjust_bitset_tests {
    use models::create_bit_vector;

    use super::filter_and_adjust_bitset;

    fn is_set(v: &[u8], i: usize) -> bool { v.get(i).copied().unwrap_or(0) != 0 }

    fn bits(v: &[u8]) -> Vec<usize> { (0..v.len()).filter(|&i| is_set(v, i)).collect() }

    #[test]
    fn bit_at_exactly_bound_count_shifts_to_zero() {
        // bit 2 set, bound_count = 2 → bit 0 of output must be set.
        let out = filter_and_adjust_bitset(create_bit_vector(&[2]), 2);
        assert!(is_set(&out, 0), "bit 0 must be set, got {out:?}");
        assert_eq!(bits(&out), vec![0]);
    }

    #[test]
    fn bits_below_bound_count_are_dropped() {
        // bits 0 and 1 set, bound_count = 2 → output has no set bits.
        let out = filter_and_adjust_bitset(create_bit_vector(&[0, 1]), 2);
        assert!(bits(&out).is_empty(), "no bits should survive, got {out:?}");
    }

    #[test]
    fn bits_above_bound_count_shift_down() {
        // bits 1, 3 set, bound_count = 2 → output has bit 1 set only.
        let out = filter_and_adjust_bitset(create_bit_vector(&[1, 3]), 2);
        assert_eq!(bits(&out), vec![1]);
    }

    #[test]
    fn empty_input_stays_empty() {
        let out = filter_and_adjust_bitset(Vec::new(), 2);
        assert!(out.is_empty());
    }

    #[test]
    fn input_shorter_than_bound_is_empty() {
        let out = filter_and_adjust_bitset(vec![1, 0], 5);
        assert!(out.is_empty());
    }

    #[test]
    fn chain_of_binders_preserves_bit() {
        // bit 5 set → filter_and_adjust(_, 5) → bit 0 set → filter_and_adjust(_, 1) → empty.
        // That's correct: bit 5 at the leaf shifts to bit 0 after crossing 5 binders,
        // then is captured (dropped) by the 6th binder.
        let after5 = filter_and_adjust_bitset(create_bit_vector(&[5]), 5);
        assert_eq!(bits(&after5), vec![0]);
        let after6 = filter_and_adjust_bitset(after5, 1);
        assert!(bits(&after6).is_empty());

        // bit 5 → cross 4 binders → bit 1 remains set; cross 1 more → bit 0 set.
        let after4 = filter_and_adjust_bitset(create_bit_vector(&[5]), 4);
        assert_eq!(bits(&after4), vec![1]);
        let after5b = filter_and_adjust_bitset(after4, 1);
        assert_eq!(bits(&after5b), vec![0]);
    }
}

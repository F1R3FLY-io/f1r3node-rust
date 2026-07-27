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

// for locally_free parameter, in case when we have (bodyResult.par.locallyFree.from(boundCount).map(x => x - boundCount))
pub(crate) fn filter_and_adjust_bitset(bitset: Vec<u8>, bound_count: usize) -> Vec<u8> {
    bitset
        .into_iter()
        .enumerate()
        .filter_map(|(i, _)| {
            if i >= bound_count {
                Some(i as u8 - bound_count as u8)
            } else {
                None
            }
        })
        .collect()
}

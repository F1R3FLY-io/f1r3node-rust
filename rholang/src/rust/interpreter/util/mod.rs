use models::rhoapi::{Bundle, Connective, Expr, If, Match, New, Par, Receive, Send};
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
